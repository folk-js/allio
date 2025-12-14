/*! Opaque platform handles with safe accessor methods.

All platform-specific unsafe code is encapsulated here.
The rest of the crate can interact with elements using safe methods.
*/

#![allow(unsafe_code)]
#![allow(
  clippy::expect_used, // NonNull::new on stack pointers - never null
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::ref_as_ptr
)]

use super::mapping::{action_from_macos, role_from_macos};
use crate::a11y::{Color, Role, Value};
use crate::platform::ElementAttributes;
use crate::types::Bounds;
use objc2_application_services::{
  AXCopyMultipleAttributeOptions, AXError, AXObserver, AXUIElement, AXValue as AXValueRef,
  AXValueType,
};
use objc2_core_foundation::{
  kCFNull, CFArray, CFBoolean, CFHash, CFNumber, CFRetained, CFString, CFType, CGPoint, CGSize,
};
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;

// FFI binding for CFEqual (not exposed by objc2-core-foundation)
extern "C" {
  fn CFEqual(cf1: *const c_void, cf2: *const c_void) -> u8;
}

/// Opaque handle to a UI element. Clone is cheap (reference counted).
#[derive(Clone)]
pub(crate) struct ElementHandle {
  inner: CFRetained<AXUIElement>,
  /// Cached `CFHash` for fast `HashMap` operations (computed once at construction)
  cached_hash: u64,
  /// Cached PID (extracted once at construction)
  pub(in crate::platform) cached_pid: u32,
}

impl ElementHandle {
  pub(in crate::platform) fn new(element: CFRetained<AXUIElement>) -> Self {
    let cached_hash = CFHash(Some(&*element)) as u64;
    let cached_pid = unsafe {
      let mut pid: i32 = 0;
      let result = element.pid(NonNull::new_unchecked(&raw mut pid));
      if result == AXError::Success {
        pid as u32
      } else {
        0 // Fallback for invalid elements (rare)
      }
    };

    Self {
      inner: element,
      cached_hash,
      cached_pid,
    }
  }

  pub(in crate::platform) fn inner(&self) -> &AXUIElement {
    &self.inner
  }

  /// Compare with another handle using `CFEqual` (local, no IPC).
  pub(crate) fn cf_equal(&self, other: &Self) -> bool {
    // IMPORTANT: Use as_ptr() to get the actual CF pointer, not a pointer to the wrapper struct.
    let self_ptr = CFRetained::as_ptr(&self.inner).as_ptr().cast::<c_void>();
    let other_ptr = CFRetained::as_ptr(&other.inner).as_ptr().cast::<c_void>();
    let result = unsafe { CFEqual(self_ptr, other_ptr) != 0 };

    result
  }

  /// Get string attribute by name.
  pub(crate) fn get_string(&self, attr: &str) -> Option<String> {
    let value = self.get_raw_attr(&CFString::from_str(attr))?;
    let s = value.downcast_ref::<CFString>()?.to_string();
    if s.is_empty() {
      None
    } else {
      Some(s)
    }
  }

  /// Get bounds (position + size).
  pub(crate) fn get_bounds(&self) -> Option<Bounds> {
    let pos = self.get_raw_attr(&CFString::from_static_str("AXPosition"))?;
    let sz = self.get_raw_attr(&CFString::from_static_str("AXSize"))?;
    Self::parse_bounds(Some(&*pos), Some(&*sz))
  }

  /// Get child elements.
  pub(crate) fn get_children(&self) -> Vec<ElementHandle> {
    let Some(value) = self.get_raw_attr(&CFString::from_static_str("AXChildren")) else {
      return Vec::new();
    };
    let Some(array) = value.downcast::<CFArray>().ok() else {
      return Vec::new();
    };
    // SAFETY: AXChildren always returns array of AXUIElements
    let typed_array: CFRetained<CFArray<AXUIElement>> =
      unsafe { CFRetained::cast_unchecked(array) };

    let len = typed_array.len();
    let mut children = Vec::with_capacity(len);
    for i in 0..len {
      if let Some(child) = typed_array.get(i) {
        children.push(ElementHandle::new(child));
      }
    }
    children
  }

  // TODO: rename fetch_element_with_attr? (merge with batch func?)
  /// Get element attribute (returns another `ElementHandle`).
  pub(crate) fn get_element(&self, attr: &str) -> Option<ElementHandle> {
    let value = self.get_raw_attr(&CFString::from_str(attr))?;
    let element = value.downcast::<AXUIElement>().ok()?;
    Some(ElementHandle::new(element))
  }

  /// Get action names supported by this element.
  pub(crate) fn get_actions(&self) -> Vec<String> {
    unsafe {
      let mut actions_ref: *const CFArray<CFString> = std::ptr::null();
      let result = self.inner.copy_action_names(
        NonNull::new((&raw mut actions_ref).cast::<*const CFArray>()).expect("actions ptr"),
      );
      if result != AXError::Success || actions_ref.is_null() {
        return Vec::new();
      }
      let actions =
        CFRetained::<CFArray<CFString>>::from_raw(NonNull::new_unchecked(actions_ref.cast_mut()));
      let len = actions.len();
      let mut result = Vec::with_capacity(len);
      for i in 0..len {
        if let Some(s) = actions.get(i) {
          result.push(s.to_string());
        }
      }
      result
    }
  }

  /// Perform an action on this element (internal - returns `AXError`).
  pub(in crate::platform) fn perform_action_internal(&self, action: &str) -> Result<(), AXError> {
    let action_name = CFString::from_str(action);
    unsafe {
      let result = self.inner.perform_action(&action_name);
      if result == AXError::Success {
        Ok(())
      } else {
        Err(result)
      }
    }
  }

  /// Set typed value (string, boolean, number, or color).
  pub(crate) fn set_typed_value(&self, value: &Value) -> Result<(), AXError> {
    let attr = CFString::from_static_str("AXValue");
    unsafe {
      let result = match value {
        Value::String(s) => {
          let cf_value = CFString::from_str(s);
          self.inner.set_attribute_value(&attr, &cf_value)
        }
        Value::Boolean(b) => {
          // macOS checkboxes use CFNumber 0/1, not CFBoolean
          let cf_value = CFNumber::new_i32(i32::from(*b));
          self.inner.set_attribute_value(&attr, &cf_value)
        }
        Value::Number(n) => {
          let cf_value = CFNumber::new_f64(*n);
          self.inner.set_attribute_value(&attr, &cf_value)
        }
        Value::Color(c) => {
          // AXColorWell uses "rgb R G B A" string format (space-separated 0.0-1.0 floats)
          // Use explicit precision to ensure consistent format (e.g., "1.0" not "1")
          let color_str = format!("rgb {:.6} {:.6} {:.6} {:.6}", c.r, c.g, c.b, c.a);
          let cf_value = CFString::from_str(&color_str);
          self.inner.set_attribute_value(&attr, &cf_value)
        }
      };
      if result == AXError::Success {
        Ok(())
      } else {
        Err(result)
      }
    }
  }

  /// Get element at position (for app-level elements only).
  pub(crate) fn element_at_position(&self, x: f64, y: f64) -> Option<ElementHandle> {
    unsafe {
      let mut element_ptr: *const AXUIElement = std::ptr::null();
      let result = self.inner.copy_element_at_position(
        x as f32,
        y as f32,
        NonNull::new(&raw mut element_ptr)?,
      );
      if result != AXError::Success || element_ptr.is_null() {
        return None;
      }
      let element = CFRetained::from_raw(NonNull::new_unchecked(element_ptr.cast_mut()));
      Some(ElementHandle::new(element))
    }
  }

  /// Fetch all common attributes in a single batch call.
  #[allow(clippy::too_many_lines)]
  pub(in crate::platform) fn fetch_attributes_internal(
    &self,
    role_hint: Option<&str>,
  ) -> crate::platform::ElementAttributes {
    // Fetch attributes in batch
    let role = CFString::from_static_str("AXRole");
    let subrole = CFString::from_static_str("AXSubrole");
    let title = CFString::from_static_str("AXTitle");
    let value = CFString::from_static_str("AXValue");
    let description = CFString::from_static_str("AXDescription");
    let placeholder = CFString::from_static_str("AXPlaceholderValue");
    let url = CFString::from_static_str("AXURL");
    let position = CFString::from_static_str("AXPosition");
    let size = CFString::from_static_str("AXSize");
    let focused = CFString::from_static_str("AXFocused");
    let enabled = CFString::from_static_str("AXEnabled");
    let selected = CFString::from_static_str("AXSelected");
    let expanded = CFString::from_static_str("AXExpanded");
    let row_index = CFString::from_static_str("AXRowIndex");
    let column_index = CFString::from_static_str("AXColumnIndex");
    let row_count = CFString::from_static_str("AXRowCount");
    let column_count = CFString::from_static_str("AXColumnCount");
    let identifier = CFString::from_static_str("AXIdentifier");

    let attr_refs: [&CFString; 18] = [
      &role,         // 0
      &subrole,      // 1
      &title,        // 2
      &value,        // 3
      &description,  // 4
      &placeholder,  // 5
      &url,          // 6
      &position,     // 7
      &size,         // 8
      &focused,      // 9
      &enabled,      // 10
      &selected,     // 11
      &expanded,     // 12
      &row_index,    // 13
      &column_index, // 14
      &row_count,    // 15
      &column_count, // 16
      &identifier,   // 17
    ];
    let attrs = CFArray::from_objects(&attr_refs);

    let values = unsafe {
      let mut values_ptr: *const CFArray<CFType> = std::ptr::null();
      let result = self.inner.copy_multiple_attribute_values(
        // Cast to untyped CFArray for the API
        &*(CFRetained::as_ptr(&attrs).as_ptr() as *const CFArray),
        AXCopyMultipleAttributeOptions::empty(),
        NonNull::new((&raw mut values_ptr).cast::<*const CFArray>()).expect("values ptr"),
      );
      if result != AXError::Success || values_ptr.is_null() {
        return ElementAttributes::default();
      }
      CFRetained::<CFArray<CFType>>::from_raw(NonNull::new_unchecked(values_ptr.cast_mut()))
    };

    let len = values.len();

    // Helper to extract value at index, filtering out kCFNull
    let get_val = |idx: usize| -> Option<CFRetained<CFType>> {
      if idx >= len {
        return None;
      }
      let retained = values.get(idx)?;
      // Check for kCFNull (accessing extern static requires unsafe)
      if let Some(null_ref) = unsafe { kCFNull } {
        let null_ptr: *const CFType = (null_ref as *const objc2_core_foundation::CFNull).cast();
        if std::ptr::eq(CFRetained::as_ptr(&retained).as_ptr(), null_ptr) {
          return None;
        }
      }
      Some(retained)
    };

    // Helper to parse non-empty string from CFType
    let parse_str = |v: &CFType| -> Option<String> {
      let s = v.downcast_ref::<CFString>()?.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    };

    // Helper to parse boolean
    let parse_bool = |v: &CFType| -> Option<bool> {
      v.downcast_ref::<CFBoolean>()
        .map(objc2_core_foundation::CFBoolean::as_bool)
    };

    // Helper to parse usize from CFNumber
    let parse_usize =
      |v: &CFType| -> Option<usize> { v.downcast_ref::<CFNumber>()?.as_i64().map(|n| n as usize) };

    let role_str = get_val(0).and_then(|v| parse_str(&v));
    let subrole_str = get_val(1).and_then(|v| parse_str(&v));
    let title_str = get_val(2).and_then(|v| parse_str(&v));
    let value_parsed =
      get_val(3).and_then(|v| Self::extract_value(&v, role_hint.or(role_str.as_deref())));
    let desc_str = get_val(4).and_then(|v| parse_str(&v));
    let placeholder_str = get_val(5).and_then(|v| parse_str(&v));
    let url_str = get_val(6).and_then(|v| parse_str(&v));
    let bounds = Self::parse_bounds(get_val(7).as_deref(), get_val(8).as_deref());
    let focused_bool = get_val(9).and_then(|v| parse_bool(&v));
    let enabled_bool = get_val(10).and_then(|v| parse_bool(&v));
    let selected_bool = get_val(11).and_then(|v| parse_bool(&v));
    let expanded_bool = get_val(12).and_then(|v| parse_bool(&v));
    let row_index_val = get_val(13).and_then(|v| parse_usize(&v));
    let column_index_val = get_val(14).and_then(|v| parse_usize(&v));
    let row_count_val = get_val(15).and_then(|v| parse_usize(&v));
    let column_count_val = get_val(16).and_then(|v| parse_usize(&v));
    let identifier_str = get_val(17).and_then(|v| parse_str(&v));

    let action_strs = self.get_actions();
    let actions = action_strs
      .into_iter()
      .filter_map(|s| action_from_macos(&s))
      .collect();

    // Convert enabled to disabled (inverted)
    let disabled = enabled_bool.is_some_and(|e| !e);

    // Map raw role string to semantic Role enum
    let raw_role = role_str.as_deref().unwrap_or("AXUnknown");
    let mut role = role_from_macos(raw_role);

    // Plain AXGroup with no label/value → GenericGroup (for pruning)
    if role == Role::Group && raw_role == "AXGroup" && title_str.is_none() && value_parsed.is_none()
    {
      role = Role::GenericGroup;
    }

    // Build platform_role string for debugging (e.g., "AXButton/AXMenuItem")
    let platform_role = match &subrole_str {
      Some(sr) => format!("{raw_role}/{sr}"),
      None => raw_role.to_string(),
    };

    ElementAttributes {
      role,
      platform_role,
      title: title_str,
      value: value_parsed,
      description: desc_str,
      placeholder: placeholder_str,
      url: url_str,
      bounds,
      focused: focused_bool,
      disabled,
      selected: selected_bool,
      expanded: expanded_bool,
      row_index: row_index_val,
      column_index: column_index_val,
      row_count: row_count_val,
      column_count: column_count_val,
      actions,
      identifier: identifier_str,
    }
  }

  /// Fetch raw `CFType` attribute (for internal platform code).
  pub(in crate::platform) fn get_raw_attr_internal(
    &self,
    attr: &CFString,
  ) -> Option<CFRetained<CFType>> {
    unsafe {
      let mut value: *const CFType = std::ptr::null();
      let result = self
        .inner
        .copy_attribute_value(attr, NonNull::new(&raw mut value)?);
      if result != AXError::Success || value.is_null() {
        return None;
      }
      Some(CFRetained::from_raw(NonNull::new_unchecked(
        value.cast_mut(),
      )))
    }
  }

  fn get_raw_attr(&self, attr: &CFString) -> Option<CFRetained<CFType>> {
    self.get_raw_attr_internal(attr)
  }

  fn extract_value(cf_value: &CFType, role: Option<&str>) -> Option<Value> {
    if let Some(cf_string) = cf_value.downcast_ref::<CFString>() {
      let s = cf_string.to_string();

      // For ColorWell, macOS may return color as "rgb R G B A" string format
      if role == Some("AXColorWell") {
        if let Some(color) = Self::parse_color_string(&s) {
          return Some(Value::Color(color));
        }
      }

      return Some(Value::String(s));
    }

    if let Some(cf_number) = cf_value.downcast_ref::<CFNumber>() {
      // For toggle-like elements, convert 0/1 integers to booleans
      if let Some(r) = role {
        if r == "AXToggle"
          || r == "AXCheckBox"
          || r == "AXRadioButton"
          || r.contains("Toggle")
          || r.contains("CheckBox")
          || r.contains("RadioButton")
        {
          if let Some(int_val) = cf_number.as_i64() {
            return Some(Value::Boolean(int_val != 0));
          }
        }
      }
      if let Some(float_val) = cf_number.as_f64() {
        return Some(Value::Number(float_val));
      }
    }

    if let Some(cf_bool) = cf_value.downcast_ref::<CFBoolean>() {
      return Some(Value::Boolean(cf_bool.as_bool()));
    }

    None
  }

  /// Parse macOS color string format: "rgb R G B A" (space-separated 0.0-1.0 floats)
  fn parse_color_string(s: &str) -> Option<Color> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    // Format: "rgb R G B A" with 5 parts total
    if parts.len() >= 5 && parts[0] == "rgb" {
      let r = parts[1].parse::<f64>().ok()?;
      let g = parts[2].parse::<f64>().ok()?;
      let b = parts[3].parse::<f64>().ok()?;
      let a = parts[4].parse::<f64>().ok()?;
      return Some(Color::new(r, g, b, a));
    }
    log::debug!("Unknown ColorWell value format: {:?}", s);
    None
  }

  fn parse_bounds(position: Option<&CFType>, size: Option<&CFType>) -> Option<Bounds> {
    let pos = position?.downcast_ref::<AXValueRef>()?;
    let sz = size?.downcast_ref::<AXValueRef>()?;

    unsafe {
      if pos.r#type() != AXValueType::CGPoint || sz.r#type() != AXValueType::CGSize {
        return None;
      }
      let mut point = CGPoint { x: 0.0, y: 0.0 };
      let mut size_val = CGSize {
        width: 0.0,
        height: 0.0,
      };

      if !pos.value(
        AXValueType::CGPoint,
        NonNull::new((&raw mut point).cast::<c_void>())?,
      ) {
        return None;
      }
      if !sz.value(
        AXValueType::CGSize,
        NonNull::new((&raw mut size_val).cast::<c_void>())?,
      ) {
        return None;
      }

      Some(Bounds {
        x: point.x,
        y: point.y,
        w: size_val.width,
        h: size_val.height,
      })
    }
  }
}

impl Hash for ElementHandle {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.cached_hash.hash(state);
  }
}

impl PartialEq for ElementHandle {
  fn eq(&self, other: &Self) -> bool {
    if self.cached_hash != other.cached_hash {
      return false;
    }
    let result = self.cf_equal(other);
    // Log hash collisions where CFEqual determines the outcome
    if result {
      log::trace!(
        "ElementHandle::eq: hash={:#x} matched, CFEqual=true (same element)",
        self.cached_hash
      );
    } else {
      log::debug!(
        "ElementHandle::eq: hash={:#x} collision, CFEqual=false (different elements)",
        self.cached_hash
      );
    }
    result
  }
}

impl Eq for ElementHandle {}

unsafe impl Send for ElementHandle {}
unsafe impl Sync for ElementHandle {}

/// Opaque handle to an observer.
#[derive(Clone)]
pub(crate) struct ObserverHandle(pub(in crate::platform) CFRetained<AXObserver>);

impl ObserverHandle {
  pub(in crate::platform) const fn new(observer: CFRetained<AXObserver>) -> Self {
    Self(observer)
  }

  pub(in crate::platform) fn inner(&self) -> &AXObserver {
    &self.0
  }
}

unsafe impl Send for ObserverHandle {}
unsafe impl Sync for ObserverHandle {}
