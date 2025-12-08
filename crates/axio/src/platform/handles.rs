/*! Opaque platform handles with safe accessor methods.

All platform-specific unsafe code is encapsulated here.
The rest of the crate can interact with elements using safe methods.
*/

use crate::accessibility::{Action, Value};
use crate::types::Bounds;

/// All commonly-needed element attributes, fetched in a batch for performance.
#[derive(Debug, Default)]
pub struct ElementAttributes {
  pub role: Option<String>,
  pub subrole: Option<String>,
  pub title: Option<String>,
  pub value: Option<Value>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub bounds: Option<Bounds>,
  pub focused: Option<bool>,
  pub enabled: Option<bool>,
  pub actions: Vec<Action>,
}

#[cfg(target_os = "macos")]
mod macos_impl {
  use super::ElementAttributes;
  use crate::accessibility::Value;
  use crate::platform::macos::mapping::action_from_macos;
  use crate::types::Bounds;
  use objc2_application_services::{
    AXCopyMultipleAttributeOptions, AXError, AXObserver, AXUIElement, AXValue as AXValueRef,
    AXValueType,
  };
  use objc2_core_foundation::{CFArray, CFBoolean, CFNumber, CFRetained, CFString, CFType};
  use std::ffi::c_void;
  use std::ptr::NonNull;

  /// Opaque handle to a UI element.
  ///
  /// On macOS this wraps an AXUIElement reference.
  /// Clone is cheap (reference counted via CFRetained).
  #[derive(Clone)]
  pub struct ElementHandle(pub(in crate::platform) CFRetained<AXUIElement>);

  impl ElementHandle {
    pub(in crate::platform) fn new(element: CFRetained<AXUIElement>) -> Self {
      Self(element)
    }

    pub(in crate::platform) fn inner(&self) -> &AXUIElement {
      &self.0
    }

    /// Get string attribute by name.
    pub fn get_string(&self, attr: &str) -> Option<String> {
      let value = self.get_raw_attr(&CFString::from_str(attr))?;
      let s = value.downcast_ref::<CFString>()?.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    }

    /// Get bounds (position + size).
    pub fn get_bounds(&self) -> Option<Bounds> {
      let pos = self.get_raw_attr(&CFString::from_static_str("AXPosition"))?;
      let sz = self.get_raw_attr(&CFString::from_static_str("AXSize"))?;
      Self::parse_bounds(Some(&*pos), Some(&*sz))
    }

    /// Get child elements.
    pub fn get_children(&self) -> Vec<ElementHandle> {
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

    /// Get element attribute (returns another ElementHandle).
    pub fn get_element(&self, attr: &str) -> Option<ElementHandle> {
      let value = self.get_raw_attr(&CFString::from_str(attr))?;
      let element = value.downcast::<AXUIElement>().ok()?;
      Some(ElementHandle::new(element))
    }

    /// Get action names supported by this element.
    pub fn get_actions(&self) -> Vec<String> {
      unsafe {
        let mut actions_ref: *const CFArray<CFString> = std::ptr::null();
        let result = self.0.copy_action_names(
          NonNull::new(&mut actions_ref as *mut *const CFArray<CFString> as *mut *const CFArray)
            .expect("actions ptr"),
        );
        if result != AXError::Success || actions_ref.is_null() {
          return Vec::new();
        }
        let actions =
          CFRetained::<CFArray<CFString>>::from_raw(NonNull::new_unchecked(actions_ref as *mut _));
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

    /// Perform an action on this element.
    pub fn perform_action(&self, action: &str) -> Result<(), AXError> {
      let action_name = CFString::from_str(action);
      unsafe {
        let result = self.0.perform_action(&action_name);
        if result == AXError::Success {
          Ok(())
        } else {
          Err(result)
        }
      }
    }

    /// Set typed value (string, boolean, integer, or float).
    pub fn set_typed_value(&self, value: &Value) -> Result<(), AXError> {
      let attr = CFString::from_static_str("AXValue");
      unsafe {
        let result = match value {
          Value::String(s) => {
            let cf_value = CFString::from_str(s);
            self.0.set_attribute_value(&attr, &cf_value)
          }
          Value::Boolean(b) => {
            // macOS checkboxes use CFNumber 0/1, not CFBoolean
            let cf_value = CFNumber::new_i32(if *b { 1 } else { 0 });
            self.0.set_attribute_value(&attr, &cf_value)
          }
          Value::Integer(i) => {
            let cf_value = CFNumber::new_i64(*i);
            self.0.set_attribute_value(&attr, &cf_value)
          }
          Value::Float(f) => {
            let cf_value = CFNumber::new_f64(*f);
            self.0.set_attribute_value(&attr, &cf_value)
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
    pub fn element_at_position(&self, x: f64, y: f64) -> Option<ElementHandle> {
      unsafe {
        let mut element_ptr: *const AXUIElement = std::ptr::null();
        let result =
          self
            .0
            .copy_element_at_position(x as f32, y as f32, NonNull::new(&mut element_ptr)?);
        if result != AXError::Success || element_ptr.is_null() {
          return None;
        }
        let element = CFRetained::from_raw(NonNull::new_unchecked(element_ptr as *mut _));
        Some(ElementHandle::new(element))
      }
    }

    /// Fetch all common attributes in a single batch call (10x faster).
    /// This is the recommended way to build an AXElement.
    pub fn get_attributes(&self, role_hint: Option<&str>) -> ElementAttributes {
      // Fetch attributes in batch
      let role = CFString::from_static_str("AXRole");
      let subrole = CFString::from_static_str("AXSubrole");
      let title = CFString::from_static_str("AXTitle");
      let value = CFString::from_static_str("AXValue");
      let description = CFString::from_static_str("AXDescription");
      let placeholder = CFString::from_static_str("AXPlaceholderValue");
      let position = CFString::from_static_str("AXPosition");
      let size = CFString::from_static_str("AXSize");
      let focused = CFString::from_static_str("AXFocused");
      let enabled = CFString::from_static_str("AXEnabled");

      let attr_refs: [&CFString; 10] = [
        &role,
        &subrole,
        &title,
        &value,
        &description,
        &placeholder,
        &position,
        &size,
        &focused,
        &enabled,
      ];
      let attrs = CFArray::from_objects(&attr_refs);

      let values = unsafe {
        let mut values_ptr: *const CFArray<CFType> = std::ptr::null();
        let result = self.0.copy_multiple_attribute_values(
          // Cast to untyped CFArray for the API
          &*(CFRetained::as_ptr(&attrs).as_ptr() as *const CFArray),
          AXCopyMultipleAttributeOptions::empty(),
          NonNull::new(&mut values_ptr as *mut *const CFArray<CFType> as *mut *const CFArray)
            .expect("values ptr"),
        );
        if result != AXError::Success || values_ptr.is_null() {
          return ElementAttributes::default();
        }
        CFRetained::<CFArray<CFType>>::from_raw(NonNull::new_unchecked(values_ptr as *mut _))
      };

      let len = values.len();

      // Helper to extract value at index, filtering out kCFNull
      let get_val = |idx: usize| -> Option<CFRetained<CFType>> {
        if idx >= len {
          return None;
        }
        let retained = values.get(idx)?;
        // Check for kCFNull
        extern "C" {
          static kCFNull: *const CFType;
        }
        if unsafe { std::ptr::eq(CFRetained::as_ptr(&retained).as_ptr(), kCFNull) } {
          return None;
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

      let role_str = get_val(0).and_then(|v| parse_str(&v));
      let subrole_str = get_val(1).and_then(|v| parse_str(&v));
      let title_str = get_val(2).and_then(|v| parse_str(&v));
      let value_parsed =
        get_val(3).and_then(|v| Self::extract_value(&v, role_hint.or(role_str.as_deref())));
      let desc_str = get_val(4).and_then(|v| parse_str(&v));
      let placeholder_str = get_val(5).and_then(|v| parse_str(&v));
      let bounds = Self::parse_bounds(get_val(6).as_deref(), get_val(7).as_deref());
      let focused_bool =
        get_val(8).and_then(|v| v.downcast_ref::<CFBoolean>().map(|b| b.as_bool()));
      let enabled_bool =
        get_val(9).and_then(|v| v.downcast_ref::<CFBoolean>().map(|b| b.as_bool()));

      let action_strs = self.get_actions();
      let actions = action_strs
        .into_iter()
        .filter_map(|s| action_from_macos(&s))
        .collect();

      ElementAttributes {
        role: role_str,
        subrole: subrole_str,
        title: title_str,
        value: value_parsed,
        description: desc_str,
        placeholder: placeholder_str,
        bounds,
        focused: focused_bool,
        enabled: enabled_bool,
        actions,
      }
    }

    /// Fetch raw CFType attribute (for internal platform code).
    pub(in crate::platform) fn get_raw_attr_internal(
      &self,
      attr: &CFString,
    ) -> Option<CFRetained<CFType>> {
      unsafe {
        let mut value: *const CFType = std::ptr::null();
        let result = self.0.copy_attribute_value(attr, NonNull::new(&mut value)?);
        if result != AXError::Success || value.is_null() {
          return None;
        }
        Some(CFRetained::from_raw(NonNull::new_unchecked(
          value as *mut _,
        )))
      }
    }

    // Private: alias for internal use
    fn get_raw_attr(&self, attr: &CFString) -> Option<CFRetained<CFType>> {
      self.get_raw_attr_internal(attr)
    }

    fn extract_value(cf_value: &CFType, role: Option<&str>) -> Option<Value> {
      if let Some(cf_string) = cf_value.downcast_ref::<CFString>() {
        return Some(Value::String(cf_string.to_string()));
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
        if let Some(int_val) = cf_number.as_i64() {
          return Some(Value::Integer(int_val));
        } else if let Some(float_val) = cf_number.as_f64() {
          return Some(Value::Float(float_val));
        }
      }

      if let Some(cf_bool) = cf_value.downcast_ref::<CFBoolean>() {
        return Some(Value::Boolean(cf_bool.as_bool()));
      }

      None
    }

    fn parse_bounds(position: Option<&CFType>, size: Option<&CFType>) -> Option<Bounds> {
      let pos = position?.downcast_ref::<AXValueRef>()?;
      let sz = size?.downcast_ref::<AXValueRef>()?;

      #[repr(C)]
      struct CGPoint {
        x: f64,
        y: f64,
      }
      #[repr(C)]
      struct CGSize {
        width: f64,
        height: f64,
      }

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
          NonNull::new(&mut point as *mut _ as *mut c_void)?,
        ) {
          return None;
        }
        if !sz.value(
          AXValueType::CGSize,
          NonNull::new(&mut size_val as *mut _ as *mut c_void)?,
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

  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}

  /// Opaque handle to an observer (for watching element changes).
  ///
  /// On macOS this wraps an AXObserver.
  #[derive(Clone)]
  pub struct ObserverHandle(pub(in crate::platform) CFRetained<AXObserver>);

  impl ObserverHandle {
    pub(in crate::platform) fn new(observer: CFRetained<AXObserver>) -> Self {
      Self(observer)
    }

    pub(in crate::platform) fn inner(&self) -> &AXObserver {
      &self.0
    }
  }

  unsafe impl Send for ObserverHandle {}
  unsafe impl Sync for ObserverHandle {}
}

#[cfg(target_os = "macos")]
pub use macos_impl::*;

#[cfg(target_os = "windows")]
compile_error!("Windows support is not yet implemented");

#[cfg(target_os = "linux")]
compile_error!("Linux support is not yet implemented");

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("Unsupported platform - AXIO only supports macOS currently");
