/**
 * macOS Platform Implementation
 *
 * Converts macOS Accessibility API elements to AXElement format.
 * All macOS-specific knowledge is encapsulated here.
 *
 * Uses objc2-application-services for modern, safe FFI bindings.
 */
use objc2_application_services::{
  AXError, AXIsProcessTrusted, AXObserver, AXObserverCallback, AXUIElement, AXValue as AXValueRef,
  AXValueType,
};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFArray, CFBoolean, CFHash, CFNumber, CFRange, CFRetained, CFRunLoop,
  CFString, CFType,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use uuid::Uuid;

use super::handles::{ElementHandle, ObserverHandle};
use crate::types::{
  AXAction, AXElement, AXRole, AXValue, AxioError, AxioResult, Bounds, ElementId, WindowId,
};

/// Fetch a raw CFType attribute from an AXUIElement.
/// This is the core primitive - all other attribute helpers build on this.
fn get_raw_attribute(element: &AXUIElement, attr_name: &CFString) -> Option<CFRetained<CFType>> {
  unsafe {
    let mut value: *const CFType = std::ptr::null();
    let result = element.copy_attribute_value(attr_name, NonNull::new(&mut value)?);
    if result != AXError::Success || value.is_null() {
      return None;
    }
    Some(CFRetained::from_raw(NonNull::new_unchecked(
      value as *mut _,
    )))
  }
}

// ============================================================================
// Accessibility Permission Check
// ============================================================================

/// Check if accessibility permissions are granted.
/// Returns true if trusted, false otherwise.
pub fn check_accessibility_permissions() -> bool {
  unsafe { AXIsProcessTrusted() }
}

/// Check accessibility permissions and log a warning if not granted.
/// Call this at app startup to help debug permission issues.
pub fn verify_accessibility_permissions() {
  if check_accessibility_permissions() {
    println!("[axio] ✓ Accessibility permissions granted");
  } else {
    eprintln!("[axio] ⚠️  WARNING: Accessibility permissions NOT granted!");
    eprintln!("[axio]    Go to System Preferences > Privacy & Security > Accessibility");
    eprintln!("[axio]    and add this application to the list.");
    eprintln!("[axio]    You may need to remove and re-add the app after rebuilding.");
  }
}

// ============================================================================
// macOS Accessibility Notifications
// ============================================================================

/// Type-safe representation of macOS accessibility notifications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AXNotification {
  /// Element's value attribute changed (text fields, sliders, etc.)
  ValueChanged,
  /// Element's title attribute changed (windows, buttons with labels)
  TitleChanged,
  /// Element was destroyed (removed from the UI)
  UIElementDestroyed,
  /// Focus moved to this element
  FocusedUIElementChanged,
  /// Selected children changed (lists, tables)
  SelectedChildrenChanged,
}

impl AXNotification {
  /// Get the macOS notification name string
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::ValueChanged => "AXValueChanged",
      Self::TitleChanged => "AXTitleChanged",
      Self::UIElementDestroyed => "AXUIElementDestroyed",
      Self::FocusedUIElementChanged => "AXFocusedUIElementChanged",
      Self::SelectedChildrenChanged => "AXSelectedChildrenChanged",
    }
  }

  /// Get notifications appropriate for a given macOS accessibility role
  pub fn for_role(role: &str) -> Vec<Self> {
    match role {
      "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" => {
        vec![Self::ValueChanged, Self::UIElementDestroyed]
      }
      "AXWindow" => vec![Self::TitleChanged, Self::UIElementDestroyed],
      _ => vec![],
    }
  }

  /// Parse from notification name string
  pub fn from_str(s: &str) -> Option<Self> {
    match s {
      "AXValueChanged" => Some(Self::ValueChanged),
      "AXTitleChanged" => Some(Self::TitleChanged),
      "AXUIElementDestroyed" => Some(Self::UIElementDestroyed),
      "AXFocusedUIElementChanged" => Some(Self::FocusedUIElementChanged),
      "AXSelectedChildrenChanged" => Some(Self::SelectedChildrenChanged),
      _ => None,
    }
  }
}

// ============================================================================
// Observer Context Registry - Safe callback handling
// ============================================================================

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap as StdHashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// Next available context ID
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Registry mapping context IDs to element IDs
static OBSERVER_CONTEXT_REGISTRY: Lazy<Mutex<StdHashMap<u64, ElementId>>> =
  Lazy::new(|| Mutex::new(StdHashMap::new()));

/// Handle passed to macOS callbacks - contains only an ID, no heap data
#[repr(C)]
pub struct ObserverContextHandle {
  context_id: u64,
}

/// Register an element for observation and get a context handle.
pub fn register_observer_context(element_id: ElementId) -> *mut ObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  OBSERVER_CONTEXT_REGISTRY
    .lock()
    .insert(context_id, element_id);
  Box::into_raw(Box::new(ObserverContextHandle { context_id }))
}

/// Unregister an element's observer context.
pub fn unregister_observer_context(handle_ptr: *mut ObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    OBSERVER_CONTEXT_REGISTRY.lock().remove(&handle.context_id);
  }
}

/// Look up element ID from context handle.
fn lookup_observer_context(handle_ptr: *const ObserverContextHandle) -> Option<ElementId> {
  if handle_ptr.is_null() {
    return None;
  }
  unsafe {
    let handle = &*handle_ptr;
    OBSERVER_CONTEXT_REGISTRY
      .lock()
      .get(&handle.context_id)
      .cloned()
  }
}

// ============================================================================
// App Observer Context Registry
// ============================================================================

/// Registry mapping app context IDs to PIDs
static APP_CONTEXT_REGISTRY: Lazy<Mutex<StdHashMap<u64, u32>>> =
  Lazy::new(|| Mutex::new(StdHashMap::new()));

/// Handle passed to app-level macOS callbacks
#[repr(C)]
pub struct AppObserverContextHandle {
  context_id: u64,
}

fn register_app_context(pid: u32) -> *mut AppObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  APP_CONTEXT_REGISTRY.lock().insert(context_id, pid);
  Box::into_raw(Box::new(AppObserverContextHandle { context_id }))
}

fn unregister_app_context(handle_ptr: *mut AppObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    APP_CONTEXT_REGISTRY.lock().remove(&handle.context_id);
  }
}

fn lookup_app_context(handle_ptr: *const AppObserverContextHandle) -> Option<u32> {
  if handle_ptr.is_null() {
    return None;
  }
  unsafe {
    let handle = &*handle_ptr;
    APP_CONTEXT_REGISTRY.lock().get(&handle.context_id).copied()
  }
}

// ============================================================================
// App-Level Observer State (Tier 1)
// ============================================================================

/// Per-app state for Tier 1 tracking
struct AppState {
  #[allow(dead_code)]
  observer: CFRetained<AXObserver>,
  context_handle: *mut AppObserverContextHandle,
  focused_element_id: Option<ElementId>,
  focused_is_watchable: bool,
}

unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

static APP_OBSERVERS: Lazy<Mutex<StdHashMap<u32, AppState>>> =
  Lazy::new(|| Mutex::new(StdHashMap::new()));

/// Clean up app observers for PIDs that are no longer running.
pub fn cleanup_dead_observers(active_pids: &std::collections::HashSet<crate::ProcessId>) -> usize {
  let mut observers = APP_OBSERVERS.lock();
  let dead_pids: Vec<u32> = observers
    .keys()
    .filter(|pid| !active_pids.iter().any(|p| p.as_u32() == **pid))
    .copied()
    .collect();

  let count = dead_pids.len();
  for pid in dead_pids {
    if let Some(state) = observers.remove(&pid) {
      // Remove observer from run loop
      unsafe {
        let run_loop_source = state.observer.run_loop_source();
        if let Some(main_run_loop) = CFRunLoop::main() {
          main_run_loop.remove_source(Some(&run_loop_source), kCFRunLoopDefaultMode);
        }
      }
      unregister_app_context(state.context_handle);
    }
  }
  count
}

/// Ensure app-level observer is set up for a PID (Tier 1).
pub fn ensure_app_observer(pid: u32) {
  let mut observers = APP_OBSERVERS.lock();
  if observers.contains_key(&pid) {
    return;
  }

  match create_app_observer(pid) {
    Ok((observer, context_handle)) => {
      observers.insert(
        pid,
        AppState {
          observer,
          context_handle,
          focused_element_id: None,
          focused_is_watchable: false,
        },
      );
    }
    Err(e) => {
      eprintln!(
        "[axio] Failed to create app observer for PID {}: {:?}",
        pid, e
      );
    }
  }
}

/// Create an app-level observer for Tier 1 notifications.
fn create_app_observer(
  pid: u32,
) -> AxioResult<(CFRetained<AXObserver>, *mut AppObserverContextHandle)> {
  let observer = unsafe {
    let mut observer_ptr: *mut AXObserver = std::ptr::null_mut();
    let callback: AXObserverCallback = Some(app_observer_callback);
    let result = AXObserver::create(
      pid as i32,
      callback,
      NonNull::new_unchecked(&mut observer_ptr),
    );

    if result != AXError::Success {
      return Err(AxioError::ObserverError(format!(
        "AXObserverCreate failed for app PID {} with code {:?}",
        pid, result
      )));
    }

    CFRetained::from_raw(
      NonNull::new(observer_ptr)
        .ok_or_else(|| AxioError::ObserverError("AXObserverCreate returned null".to_string()))?,
    )
  };

  // Add to main run loop
  unsafe {
    let run_loop_source = observer.run_loop_source();
    if let Some(main_run_loop) = CFRunLoop::main() {
      main_run_loop.add_source(Some(&run_loop_source), kCFRunLoopDefaultMode);
    }
  }

  // Subscribe to app-level notifications on the application element
  let app_element = unsafe { AXUIElement::new_application(pid as i32) };

  let context_handle = register_app_context(pid);

  // Subscribe to focus changes
  let focus_notif = CFString::from_static_str("AXFocusedUIElementChanged");
  unsafe {
    let _ = observer.add_notification(&app_element, &focus_notif, context_handle as *mut c_void);
  }

  // Subscribe to selection changes
  let selection_notif = CFString::from_static_str("AXSelectedTextChanged");
  unsafe {
    let _ = observer.add_notification(
      &app_element,
      &selection_notif,
      context_handle as *mut c_void,
    );
  }

  Ok((observer, context_handle))
}

/// Callback for app-level notifications (Tier 1)
unsafe extern "C-unwind" fn app_observer_callback(
  _observer: NonNull<AXObserver>,
  element: NonNull<AXUIElement>,
  notification: NonNull<CFString>,
  refcon: *mut c_void,
) {
  if refcon.is_null() {
    return;
  }

  let Some(pid) = lookup_app_context(refcon as *const AppObserverContextHandle) else {
    return;
  };

  let notification_name = notification.as_ref().to_string();
  let element_ref = CFRetained::retain(element);

  match notification_name.as_str() {
    "AXFocusedUIElementChanged" => {
      handle_app_focus_changed(pid, element_ref);
    }
    "AXSelectedTextChanged" => {
      handle_app_selection_changed(pid, element_ref);
    }
    _ => {}
  }
}

fn should_auto_watch(role: &crate::types::AXRole) -> bool {
  use crate::types::AXRole;
  matches!(
    role,
    AXRole::Textbox
      | AXRole::Searchbox
      | AXRole::Checkbox
      | AXRole::Radio
      | AXRole::Toggle
      | AXRole::Slider
  )
}

fn handle_app_focus_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let window_id = match get_window_id_from_ax_element(&element) {
    Some(id) => id,
    None => return,
  };

  let ax_element = build_element(&element, &window_id, pid, None);
  let new_is_watchable = should_auto_watch(&ax_element.role);

  let (previous_element_id, previous_was_watchable) = {
    let mut observers = APP_OBSERVERS.lock();
    if let Some(state) = observers.get_mut(&pid) {
      let prev_id = state.focused_element_id.clone();
      let prev_was_watchable = state.focused_is_watchable;
      state.focused_element_id = Some(ax_element.id.clone());
      state.focused_is_watchable = new_is_watchable;
      (prev_id, prev_was_watchable)
    } else {
      (None, false)
    }
  };

  let same_element = previous_element_id.as_ref() == Some(&ax_element.id);

  if previous_was_watchable && !same_element {
    if let Some(ref prev_id) = previous_element_id {
      crate::element_registry::ElementRegistry::unwatch(prev_id);
    }
  }

  if new_is_watchable && !same_element {
    let _ = crate::element_registry::ElementRegistry::watch(&ax_element.id);
  }

  crate::events::emit_focus_element(
    &window_id,
    &ax_element.id,
    &ax_element,
    previous_element_id.as_ref(),
  );
}

fn handle_app_selection_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let window_id = match get_window_id_from_ax_element(&element) {
    Some(id) => id,
    None => return,
  };

  let ax_element = build_element(&element, &window_id, pid, None);

  let selected_text = get_string_attribute(&element, "AXSelectedText").unwrap_or_default();
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&element)
  };

  crate::events::emit_selection_changed(&window_id, &ax_element.id, &selected_text, range.as_ref());
}

fn get_selected_text_range(element: &AXUIElement) -> Option<crate::types::TextRange> {
  let attr_name = CFString::from_static_str("AXSelectedTextRange");
  let value = get_raw_attribute(element, &attr_name)?;

  let ax_value = value.downcast_ref::<AXValueRef>()?;

  unsafe {
    let mut range = CFRange {
      location: 0,
      length: 0,
    };
    if ax_value.value(
      AXValueType::CFRange,
      NonNull::new(&mut range as *mut _ as *mut c_void)?,
    ) {
      Some(crate::types::TextRange {
        start: range.location as u32,
        length: range.length as u32,
      })
    } else {
      None
    }
  }
}

/// Query the currently focused element and selection for an app.
pub fn get_current_focus(
  pid: u32,
) -> (
  Option<crate::types::AXElement>,
  Option<crate::types::Selection>,
) {
  let app_element = unsafe { AXUIElement::new_application(pid as i32) };
  let attr_name = CFString::from_static_str("AXFocusedUIElement");

  let Some(value) = get_raw_attribute(&app_element, &attr_name) else {
    return (None, None);
  };

  let focused_element = unsafe {
    let ptr = CFRetained::as_ptr(&value).as_ptr() as *mut AXUIElement;
    std::mem::forget(value);
    CFRetained::<AXUIElement>::from_raw(NonNull::new_unchecked(ptr))
  };

  let window_id = match get_window_id_from_ax_element(&focused_element) {
    Some(id) => id,
    None => return (None, None),
  };

  let element = build_element(&focused_element, &window_id, pid, None);

  let selection =
    get_element_selected_text(&focused_element).map(|(text, range)| crate::types::Selection {
      element_id: element.id.clone(),
      text,
      range,
    });

  (Some(element), selection)
}

fn get_element_selected_text(
  element: &AXUIElement,
) -> Option<(String, Option<crate::types::TextRange>)> {
  let selected_text = get_string_attribute(element, "AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  let range = get_selected_text_range(element);
  Some((selected_text, range))
}

fn get_window_id_from_ax_element(element: &AXUIElement) -> Option<WindowId> {
  let attr_name = CFString::from_static_str("AXWindow");
  let value = get_raw_attribute(element, &attr_name)?;

  let window_element = unsafe {
    let ptr = CFRetained::as_ptr(&value).as_ptr() as *mut AXUIElement;
    std::mem::forget(value);
    CFRetained::<AXUIElement>::from_raw(NonNull::new_unchecked(ptr))
  };

  let bounds = get_element_bounds(&window_element)?;
  crate::window_registry::find_by_bounds(&bounds)
}

/// Create an observer for a process and add it to the main run loop.
pub fn create_observer_for_pid(pid: u32) -> AxioResult<ObserverHandle> {
  let observer = unsafe {
    let mut observer_ptr: *mut AXObserver = std::ptr::null_mut();
    let callback: AXObserverCallback = Some(observer_callback);
    let result = AXObserver::create(
      pid as i32,
      callback,
      NonNull::new_unchecked(&mut observer_ptr),
    );

    if result != AXError::Success {
      return Err(AxioError::ObserverError(format!(
        "AXObserverCreate failed with code {:?}",
        result
      )));
    }

    CFRetained::from_raw(
      NonNull::new(observer_ptr)
        .ok_or_else(|| AxioError::ObserverError("AXObserverCreate returned null".to_string()))?,
    )
  };

  // Must add to MAIN run loop for callbacks to fire
  unsafe {
    let run_loop_source = observer.run_loop_source();
    if let Some(main_run_loop) = CFRunLoop::main() {
      main_run_loop.add_source(Some(&run_loop_source), kCFRunLoopDefaultMode);
    }
  }

  Ok(ObserverHandle::new(observer))
}

unsafe extern "C-unwind" fn observer_callback(
  _observer: NonNull<AXObserver>,
  element: NonNull<AXUIElement>,
  notification: NonNull<CFString>,
  refcon: *mut c_void,
) {
  use std::panic::AssertUnwindSafe;

  let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
    if refcon.is_null() {
      return;
    }

    let Some(element_id) = lookup_observer_context(refcon as *const ObserverContextHandle) else {
      return;
    };

    let notification_name = notification.as_ref().to_string();
    let element_ref = CFRetained::retain(element);

    handle_notification(&element_id, &notification_name, &element_ref);
  }));

  if result.is_err() {
    eprintln!("[axio] ⚠️  Accessibility notification handler panicked (possibly invalid element)");
  }
}

fn handle_notification(element_id: &ElementId, notification: &str, ax_element: &AXUIElement) {
  use crate::element_registry::ElementRegistry;

  let Some(notification_type) = AXNotification::from_str(notification) else {
    return;
  };

  match notification_type {
    AXNotification::ValueChanged => {
      let role = get_string_attribute(ax_element, "AXRole");
      if let Some(value) = get_typed_attribute(ax_element, "AXValue", role.as_deref()) {
        if let Ok(mut element) = ElementRegistry::get(element_id) {
          element.value = Some(value);
          let _ = ElementRegistry::update(element_id, element.clone());
          crate::events::emit_element_changed(&element);
        }
      }
    }

    AXNotification::TitleChanged => {
      if let Some(label) = get_string_attribute(ax_element, "AXTitle") {
        if !label.is_empty() {
          if let Ok(mut element) = ElementRegistry::get(element_id) {
            element.label = Some(label);
            let _ = ElementRegistry::update(element_id, element.clone());
            crate::events::emit_element_changed(&element);
          }
        }
      }
    }

    AXNotification::UIElementDestroyed => {
      if let Ok(element) = ElementRegistry::get(element_id) {
        ElementRegistry::remove_element(element_id);
        crate::events::emit_element_removed(&element);
      } else {
        ElementRegistry::remove_element(element_id);
      }
    }

    _ => {}
  }
}

// ============================================================================
// Attribute Helpers
// ============================================================================

fn get_string_attribute(element: &AXUIElement, attr_name: &str) -> Option<String> {
  let attr = CFString::from_str(attr_name);
  let value = get_raw_attribute(element, &attr)?;
  let s = value.downcast_ref::<CFString>()?.to_string();
  if s.is_empty() {
    None
  } else {
    Some(s)
  }
}

fn get_bool_attribute(element: &AXUIElement, attr_name: &str) -> Option<bool> {
  let attr = CFString::from_str(attr_name);
  let value = get_raw_attribute(element, &attr)?;
  Some(value.downcast_ref::<CFBoolean>()?.as_bool())
}

fn get_typed_attribute(
  element: &AXUIElement,
  attr_name: &str,
  role: Option<&str>,
) -> Option<AXValue> {
  let attr = CFString::from_str(attr_name);
  let value = get_raw_attribute(element, &attr)?;
  extract_value(&value, role)
}

// ============================================================================
// AXUIElement to AXElement Conversion
// ============================================================================

/// Build an AXElement from a macOS AXUIElement and register it.
pub fn build_element(
  ax_element: &AXUIElement,
  window_id: &WindowId,
  pid: u32,
  parent_id: Option<&ElementId>,
) -> AXElement {
  use crate::element_registry::ElementRegistry;

  ensure_app_observer(pid);

  let platform_role =
    get_string_attribute(ax_element, "AXRole").unwrap_or_else(|| "Unknown".to_string());
  let role = map_platform_role(&platform_role);

  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.clone())
  } else {
    get_string_attribute(ax_element, "AXSubrole")
  };

  let label = get_string_attribute(ax_element, "AXTitle");
  let value = get_typed_attribute(ax_element, "AXValue", Some(&platform_role));
  let description = get_string_attribute(ax_element, "AXDescription");
  let placeholder = get_string_attribute(ax_element, "AXPlaceholderValue");
  let bounds = get_element_bounds(ax_element);
  let focused = get_bool_attribute(ax_element, "AXFocused");
  let enabled = get_bool_attribute(ax_element, "AXEnabled");
  let actions = get_element_actions(ax_element);

  let element = AXElement {
    id: ElementId::new(Uuid::new_v4().to_string()),
    window_id: window_id.clone(),
    parent_id: parent_id.cloned(),
    children: None,
    role,
    subrole,
    label,
    value,
    description,
    placeholder,
    bounds,
    focused,
    enabled,
    actions,
  };

  // Create handle by retaining the element
  let handle = unsafe {
    let retained = CFRetained::retain(NonNull::new_unchecked(
      ax_element as *const _ as *mut AXUIElement,
    ));
    ElementHandle::new(retained)
  };

  ElementRegistry::register(element, handle, pid, &platform_role)
}

/// Discover and register children of an element.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  use crate::element_registry::ElementRegistry;

  let (ax_element, window_id, pid) = ElementRegistry::with_stored(parent_id, |stored| {
    (
      stored.handle.retained(),
      stored.element.window_id.clone(),
      stored.pid,
    )
  })?;

  let children_array = get_children_array(&ax_element);
  let Some(children_array) = children_array else {
    ElementRegistry::set_children(parent_id, vec![])?;
    return Ok(vec![]);
  };

  let child_count = children_array.len();
  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for i in 0..(child_count as usize).min(max_children) {
    if let Some(child_ref) = children_array.get(i) {
      let child = build_element(&child_ref, &window_id, pid, Some(parent_id));
      child_ids.push(child.id.clone());
      children.push(child);
    }
  }

  ElementRegistry::set_children(parent_id, child_ids.clone())?;

  for child in &children {
    crate::events::emit_element_added(child);
  }

  if let Ok(updated_parent) = ElementRegistry::get(parent_id) {
    crate::events::emit_element_changed(&updated_parent);
  }

  Ok(children)
}

fn get_children_array(element: &AXUIElement) -> Option<CFRetained<CFArray<AXUIElement>>> {
  let attr = CFString::from_static_str("AXChildren");
  let value = get_raw_attribute(element, &attr)?;

  unsafe {
    let ptr = CFRetained::as_ptr(&value).as_ptr() as *mut CFArray<AXUIElement>;
    // Transfer ownership - don't let `value` release the reference
    std::mem::forget(value);
    Some(CFRetained::from_raw(NonNull::new_unchecked(ptr)))
  }
}

/// Refresh an element's attributes from the platform.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
  use crate::element_registry::ElementRegistry;

  let (ax_element, window_id, _pid, parent_id, children, platform_role) =
    ElementRegistry::with_stored(element_id, |stored| {
      (
        stored.handle.retained(),
        stored.element.window_id.clone(),
        stored.pid,
        stored.element.parent_id.clone(),
        stored.element.children.clone(),
        stored.platform_role.clone(),
      )
    })?;

  let role = map_platform_role(&platform_role);

  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.to_string())
  } else {
    get_string_attribute(&ax_element, "AXSubrole")
  };

  let label = get_string_attribute(&ax_element, "AXTitle");
  let value = get_typed_attribute(&ax_element, "AXValue", Some(&platform_role));
  let description = get_string_attribute(&ax_element, "AXDescription");
  let placeholder = get_string_attribute(&ax_element, "AXPlaceholderValue");
  let bounds = get_element_bounds(&ax_element);
  let focused = get_bool_attribute(&ax_element, "AXFocused");
  let enabled = get_bool_attribute(&ax_element, "AXEnabled");
  let actions = get_element_actions(&ax_element);

  let updated = AXElement {
    id: element_id.clone(),
    window_id,
    parent_id,
    children,
    role,
    subrole,
    label,
    value,
    description,
    placeholder,
    bounds,
    focused,
    enabled,
    actions,
  };

  ElementRegistry::update(element_id, updated.clone())?;
  Ok(updated)
}

/// Map macOS AX* roles to ARIA-based AXIO roles
fn map_platform_role(platform_role: &str) -> AXRole {
  let role = platform_role
    .strip_prefix("AX")
    .unwrap_or(platform_role)
    .to_lowercase();

  match role.as_str() {
    "application" => AXRole::Application,
    "window" | "standardwindow" => AXRole::Window,
    "group" | "scrollarea" => AXRole::Group,
    "button" | "defaultbutton" => AXRole::Button,
    "checkbox" => AXRole::Checkbox,
    "radiobutton" => AXRole::Radio,
    "toggle" => AXRole::Toggle,
    "textfield" | "textarea" | "textbox" | "securetextfield" | "combobox" => AXRole::Textbox,
    "searchfield" => AXRole::Searchbox,
    "slider" => AXRole::Slider,
    "menu" => AXRole::Menu,
    "menuitem" => AXRole::Menuitem,
    "menubar" => AXRole::Menubar,
    "link" => AXRole::Link,
    "tab" => AXRole::Tab,
    "tabgroup" => AXRole::Tablist,
    "statictext" | "text" => AXRole::Text,
    "heading" => AXRole::Heading,
    "image" => AXRole::Image,
    "list" => AXRole::List,
    "listitem" | "row" => AXRole::Listitem,
    "table" => AXRole::Table,
    "cell" => AXRole::Cell,
    "progressindicator" => AXRole::Progressbar,
    "scrollbar" => AXRole::Scrollbar,
    _ => AXRole::Unknown,
  }
}

fn map_platform_action(action: &str) -> Option<AXAction> {
  match action {
    "AXPress" => Some(AXAction::Press),
    "AXShowMenu" => Some(AXAction::ShowMenu),
    "AXIncrement" => Some(AXAction::Increment),
    "AXDecrement" => Some(AXAction::Decrement),
    "AXConfirm" => Some(AXAction::Confirm),
    "AXCancel" => Some(AXAction::Cancel),
    "AXRaise" => Some(AXAction::Raise),
    "AXPick" => Some(AXAction::Pick),
    _ => None,
  }
}

// ============================================================================
// Element Actions
// ============================================================================

fn get_element_actions(element: &AXUIElement) -> Vec<AXAction> {
  unsafe {
    let mut actions_ref: *const CFArray = std::ptr::null();

    let Some(ptr) = NonNull::new(&mut actions_ref) else {
      return vec![];
    };
    let result = element.copy_action_names(ptr);
    if result != AXError::Success || actions_ref.is_null() {
      return vec![];
    }

    let actions_array =
      CFRetained::<CFArray<CFString>>::from_raw(NonNull::new_unchecked(actions_ref as *mut _));

    let mut actions = Vec::new();
    for i in 0..actions_array.len() {
      if let Some(action_cf) = actions_array.get(i) {
        let action_str = action_cf.to_string();
        if let Some(action) = map_platform_action(&action_str) {
          actions.push(action);
        }
      }
    }
    actions
  }
}

// ============================================================================
// Geometry Extraction
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
  x: f64,
  y: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
  width: f64,
  height: f64,
}

fn get_element_bounds(element: &AXUIElement) -> Option<Bounds> {
  let position = get_position_attribute(element)?;
  let size = get_size_attribute(element)?;

  Some(Bounds {
    x: position.0,
    y: position.1,
    w: size.0,
    h: size.1,
  })
}

fn get_position_attribute(element: &AXUIElement) -> Option<(f64, f64)> {
  let attr = CFString::from_static_str("AXPosition");
  let value = get_raw_attribute(element, &attr)?;
  let ax_value = value.downcast_ref::<AXValueRef>()?;

  unsafe {
    if ax_value.r#type() != AXValueType::CGPoint {
      return None;
    }
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    if ax_value.value(
      AXValueType::CGPoint,
      NonNull::new(&mut point as *mut _ as *mut c_void)?,
    ) {
      Some((point.x, point.y))
    } else {
      None
    }
  }
}

fn get_size_attribute(element: &AXUIElement) -> Option<(f64, f64)> {
  let attr = CFString::from_static_str("AXSize");
  let value = get_raw_attribute(element, &attr)?;
  let ax_value = value.downcast_ref::<AXValueRef>()?;

  unsafe {
    if ax_value.r#type() != AXValueType::CGSize {
      return None;
    }
    let mut size = CGSize {
      width: 0.0,
      height: 0.0,
    };
    if ax_value.value(
      AXValueType::CGSize,
      NonNull::new(&mut size as *mut _ as *mut c_void)?,
    ) {
      Some((size.width, size.height))
    } else {
      None
    }
  }
}

// ============================================================================
// Window Elements
// ============================================================================

/// Get all window AXUIElements for a given PID
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<CFRetained<AXUIElement>>> {
  let app_element = unsafe { AXUIElement::new_application(pid as i32) };

  let children = get_children_array(&app_element);
  let Some(children) = children else {
    return Ok(Vec::new());
  };

  let mut result = Vec::new();

  for i in 0..children.len() {
    if let Some(child_element) = children.get(i) {
      let role_str = get_string_attribute(&child_element, "AXRole");
      if role_str.as_deref() == Some("AXWindow") {
        // Retain the element for the result
        let retained = unsafe {
          CFRetained::retain(NonNull::new_unchecked(
            &*child_element as *const AXUIElement as *mut AXUIElement,
          ))
        };
        result.push(retained);
      }
    }
  }

  Ok(result)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  let (window, handle) = crate::window_registry::get_with_handle(window_id)
    .ok_or_else(|| AxioError::WindowNotFound(window_id.clone()))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {} has no AX element", window_id)))?;

  Ok(build_element(
    window_handle.inner(),
    window_id,
    window.process_id.as_u32(),
    None,
  ))
}

/// Enable accessibility for an Electron app
pub fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  let raw_pid = pid.as_u32();
  let app_element = unsafe { AXUIElement::new_application(raw_pid as i32) };
  let attr_name = CFString::from_static_str("AXManualAccessibility");
  let value = CFBoolean::new(true);

  unsafe {
    let result = app_element.set_attribute_value(&attr_name, &*value);

    if result == AXError::Success {
      eprintln!("[axio] ✓ Enabled accessibility for PID {}", raw_pid);
    } else if result != AXError::AttributeUnsupported {
      eprintln!(
        "[axio] ⚠️  Failed to enable accessibility for PID {} (error: {:?})",
        raw_pid, result
      );
    }
  }
}

/// Get the accessibility element at a specific screen position.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXElement> {
  let window = crate::window_registry::find_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!(
      "No tracked window found at position ({}, {})",
      x, y
    ))
  })?;

  let window_id = window.id.clone();
  let pid = window.process_id.as_u32();

  let app_element = unsafe { AXUIElement::new_application(pid as i32) };

  unsafe {
    let mut element_ptr: *const AXUIElement = std::ptr::null();
    let Some(ptr) = NonNull::new(&mut element_ptr) else {
      return Err(AxioError::AccessibilityError(
        "Failed to create pointer".to_string(),
      ));
    };
    let result = app_element.copy_element_at_position(x as f32, y as f32, ptr);

    if result != AXError::Success || element_ptr.is_null() {
      return Err(AxioError::AccessibilityError(format!(
        "No element found at ({}, {}) in app {}",
        x, y, pid
      )));
    }

    let ax_element = CFRetained::from_raw(NonNull::new_unchecked(element_ptr as *mut _));
    Ok(build_element(&ax_element, &window_id, pid, None))
  }
}

// ============================================================================
// Value Extraction
// ============================================================================

fn extract_value(cf_value: &CFType, role: Option<&str>) -> Option<AXValue> {
  // Try CFString
  if let Some(cf_string) = cf_value.downcast_ref::<CFString>() {
    let s = cf_string.to_string();
    return if s.is_empty() {
      None
    } else {
      Some(AXValue::String(s))
    };
  }

  // Try CFNumber
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
          return Some(AXValue::Boolean(int_val != 0));
        }
      }
    }

    if let Some(int_val) = cf_number.as_i64() {
      return Some(AXValue::Integer(int_val));
    } else if let Some(float_val) = cf_number.as_f64() {
      return Some(AXValue::Float(float_val));
    }
  }

  // Try CFBoolean
  if let Some(cf_bool) = cf_value.downcast_ref::<CFBoolean>() {
    return Some(AXValue::Boolean(cf_bool.as_bool()));
  }

  None
}

// ============================================================================
// Element Operations
// ============================================================================

const WRITABLE_ROLES: &[&str] = &[
  "AXTextField",
  "AXTextArea",
  "AXComboBox",
  "AXSecureTextField",
  "AXSearchField",
];

/// Get hash for element handle (for O(1) dedup lookup).
pub fn element_hash(handle: &ElementHandle) -> u64 {
  CFHash(Some(handle.inner())) as u64
}

/// Write a text value to an element.
pub fn write_element_value(
  handle: &ElementHandle,
  text: &str,
  platform_role: &str,
) -> AxioResult<()> {
  if !WRITABLE_ROLES.contains(&platform_role) {
    return Err(AxioError::NotSupported(format!(
      "Element with role '{}' is not writable",
      platform_role
    )));
  }

  let cf_string = CFString::from_str(text);
  let attr_name = CFString::from_static_str("AXValue");

  unsafe {
    let result = handle.inner().set_attribute_value(&attr_name, &*cf_string);
    if result != AXError::Success {
      return Err(AxioError::AccessibilityError(format!(
        "Failed to set value: {:?}",
        result
      )));
    }
  }

  Ok(())
}

/// Perform a click (press) action on an element.
pub fn click_element(handle: &ElementHandle) -> AxioResult<()> {
  let action = CFString::from_static_str("AXPress");

  unsafe {
    let result = handle.inner().perform_action(&action);
    if result != AXError::Success {
      return Err(AxioError::AccessibilityError(format!(
        "AXUIElementPerformAction failed with code {:?}",
        result
      )));
    }
  }

  Ok(())
}

/// Register notifications for an element.
pub fn subscribe_element_notifications(
  element_id: &ElementId,
  handle: &ElementHandle,
  platform_role: &str,
  observer: ObserverHandle,
) -> AxioResult<(*mut ObserverContextHandle, Vec<AXNotification>)> {
  let notifications = AXNotification::for_role(platform_role);
  if notifications.is_empty() {
    return Ok((std::ptr::null_mut(), Vec::new()));
  }

  let context_handle = register_observer_context(element_id.clone());

  let mut registered = Vec::new();
  for notification in &notifications {
    let notif_cfstring = CFString::from_str(notification.as_str());
    unsafe {
      let result = observer.inner().add_notification(
        handle.inner(),
        &notif_cfstring,
        context_handle as *mut c_void,
      );
      if result == AXError::Success {
        registered.push(*notification);
      }
    }
  }

  if registered.is_empty() {
    unregister_observer_context(context_handle);
    return Err(AxioError::ObserverError(format!(
      "Failed to register notifications for element (role: {})",
      platform_role
    )));
  }

  Ok((context_handle, registered))
}

/// Unsubscribe from element notifications.
pub fn unsubscribe_element_notifications(
  handle: &ElementHandle,
  observer: ObserverHandle,
  context_handle: *mut ObserverContextHandle,
  notifications: &[AXNotification],
) {
  for notification in notifications {
    let notif_cfstring = CFString::from_str(notification.as_str());
    unsafe {
      let _ = observer
        .inner()
        .remove_notification(handle.inner(), &notif_cfstring);
    }
  }

  unregister_observer_context(context_handle);
}

/// Fetch an element handle for a window by matching bounds.
pub fn fetch_window_handle(window: &crate::AXWindow) -> Option<ElementHandle> {
  let window_elements = get_window_elements(window.process_id.as_u32()).ok()?;

  if window_elements.is_empty() {
    return None;
  }

  const MARGIN: f64 = 2.0;

  for element in window_elements.iter() {
    let element_bounds = get_element_bounds(element);

    if let Some(element_bounds) = element_bounds {
      if window.bounds.matches(&element_bounds, MARGIN) {
        return Some(ElementHandle::new(element.clone()));
      }
    }
  }

  // Fallback: use only element if there's just one
  if window_elements.len() == 1 {
    return Some(ElementHandle::new(window_elements[0].clone()));
  }

  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cgpoint_size() {
    assert_eq!(std::mem::size_of::<CGPoint>(), 16);
  }

  #[test]
  fn test_cgsize_size() {
    assert_eq!(std::mem::size_of::<CGSize>(), 16);
  }
}
