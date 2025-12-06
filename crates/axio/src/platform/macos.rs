/**
 * macOS Platform Implementation
 *
 * Converts macOS Accessibility API elements to AXElement format.
 * All macOS-specific knowledge is encapsulated here.
 */
use accessibility::*;
use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use uuid::Uuid;

use crate::types::{
  AXAction, AXElement, AXRole, AXValue, AxioError, AxioResult, Bounds, ElementId, WindowId,
};

// ============================================================================
// Accessibility Permission Check
// ============================================================================

/// Check if accessibility permissions are granted.
/// Returns true if trusted, false otherwise.
pub fn check_accessibility_permissions() -> bool {
  unsafe {
    // AXIsProcessTrusted returns true if the app has accessibility permissions
    accessibility_sys::AXIsProcessTrusted()
  }
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
///
/// These map to `kAX*Notification` constants from the Accessibility API.
/// Using an enum prevents typos and enables compile-time checking.
///
/// Note: This is macOS-specific. Other platforms will have their own
/// notification types in their respective platform modules.
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
  ///
  /// These strings match the `kAX*Notification` constants.
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
  ///
  /// Conservative approach: only subscribe to essential, reliable notifications.
  pub fn for_role(role: &str) -> Vec<Self> {
    match role {
      // Text input elements - watch value changes
      "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" => {
        vec![Self::ValueChanged, Self::UIElementDestroyed]
      }
      // Windows - watch title changes
      "AXWindow" => vec![Self::TitleChanged, Self::UIElementDestroyed],
      // Everything else - no subscriptions
      // Note: AXStaticText does NOT reliably emit value changed notifications
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
// AXObserver Creation and Callbacks
// ============================================================================

use accessibility_sys::{AXObserverCreate, AXObserverGetRunLoopSource, AXObserverRef};
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap as StdHashMap;

// ============================================================================
// Observer Context Registry - Safe callback handling
// ============================================================================
//
// Instead of passing actual data (ElementId) to macOS callbacks, we pass only
// a numeric ID. The callback looks up the ID in a thread-safe registry.
// If the ID is found, we use the data; if not, the element was cleaned up.
// This avoids all use-after-free issues with macOS callbacks.

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
/// Returns a boxed handle that can be passed to macOS.
pub fn register_observer_context(element_id: ElementId) -> *mut ObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  OBSERVER_CONTEXT_REGISTRY
    .lock()
    .insert(context_id, element_id);

  Box::into_raw(Box::new(ObserverContextHandle { context_id }))
}

/// Unregister an element's observer context. Called during cleanup.
pub fn unregister_observer_context(handle_ptr: *mut ObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    OBSERVER_CONTEXT_REGISTRY.lock().remove(&handle.context_id);
  }
}

/// Look up element ID from context handle. Returns None if element was cleaned up.
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

/// Get the count of active observer contexts (for diagnostics)
pub fn observer_context_count() -> usize {
  OBSERVER_CONTEXT_REGISTRY.lock().len()
}

// Legacy type alias for compatibility
pub type ObserverContext = ObserverContextHandle;

// ============================================================================
// App Observer Context Registry
// ============================================================================

/// Registry mapping app context IDs to PIDs
static APP_CONTEXT_REGISTRY: Lazy<Mutex<StdHashMap<u64, u32>>> =
  Lazy::new(|| Mutex::new(StdHashMap::new()));

/// Handle passed to app-level macOS callbacks - contains only an ID
#[repr(C)]
pub struct AppObserverContextHandle {
  context_id: u64,
}

/// Register an app for observation and get a context handle.
fn register_app_context(pid: u32) -> *mut AppObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  APP_CONTEXT_REGISTRY.lock().insert(context_id, pid);
  Box::into_raw(Box::new(AppObserverContextHandle { context_id }))
}

/// Unregister an app's observer context.
fn unregister_app_context(handle_ptr: *mut AppObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    APP_CONTEXT_REGISTRY.lock().remove(&handle.context_id);
  }
}

/// Look up PID from app context handle. Returns None if app was cleaned up.
fn lookup_app_context(handle_ptr: *const AppObserverContextHandle) -> Option<u32> {
  if handle_ptr.is_null() {
    return None;
  }
  unsafe {
    let handle = &*handle_ptr;
    APP_CONTEXT_REGISTRY.lock().get(&handle.context_id).copied()
  }
}

// Legacy alias
pub type AppObserverContext = AppObserverContextHandle;

// ============================================================================
// App-Level Observer State (Tier 1)
// ============================================================================

/// Per-app state for Tier 1 tracking
struct AppState {
  #[allow(dead_code)] // Kept alive to maintain observer subscription
  observer: AXObserverRef,
  /// Handle pointer for cleanup
  context_handle: *mut AppObserverContextHandle,
  focused_element_id: Option<ElementId>,
  /// For Tier 2: track if focused element should be auto-watched
  focused_is_watchable: bool,
}

// SAFETY: AXObserverRef is a CFTypeRef (thread-safe with proper retain/release)
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

static APP_OBSERVERS: Lazy<Mutex<StdHashMap<u32, AppState>>> =
  Lazy::new(|| Mutex::new(StdHashMap::new()));

/// Clean up app observers for PIDs that are no longer running.
/// Called periodically from the polling loop.
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
        let run_loop_source_ref = AXObserverGetRunLoopSource(state.observer);
        if !run_loop_source_ref.is_null() {
          let run_loop_source = CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
          let main_run_loop = CFRunLoop::get_main();
          main_run_loop.remove_source(&run_loop_source, kCFRunLoopDefaultMode);
        }
      }

      // Unregister from context registry.
      // If a callback is in-flight, it will safely fail the lookup and return early.
      unregister_app_context(state.context_handle);
    }
  }
  count
}

/// Get the number of active app observers (for diagnostics).
pub fn app_observer_count() -> usize {
  APP_OBSERVERS.lock().len()
}

/// Ensure app-level observer is set up for a PID (Tier 1).
/// Called when first element from an app is registered.
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
/// Returns (observer, context_handle) for storage in AppState.
fn create_app_observer(pid: u32) -> AxioResult<(AXObserverRef, *mut AppObserverContextHandle)> {
  use accessibility_sys::AXObserverAddNotification;

  let mut observer_ref: AXObserverRef = std::ptr::null_mut();

  let result =
    unsafe { AXObserverCreate(pid as i32, app_observer_callback as _, &mut observer_ref) };

  if result != 0 {
    return Err(AxioError::ObserverError(format!(
      "AXObserverCreate failed for app PID {} with code {}",
      pid, result
    )));
  }

  // Add to main run loop
  unsafe {
    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
    if !run_loop_source_ref.is_null() {
      let run_loop_source = CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
      let main_run_loop = CFRunLoop::get_main();
      main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);
    }
  }

  // Subscribe to app-level notifications on the application element
  let app_element = AXUIElement::application(pid as i32);

  // Register in context registry - returns a handle pointer
  let context_handle = register_app_context(pid);

  // Subscribe to focus changes
  let focus_notif = CFString::new("AXFocusedUIElementChanged");
  unsafe {
    let _ = AXObserverAddNotification(
      observer_ref,
      app_element.as_concrete_TypeRef(),
      focus_notif.as_concrete_TypeRef() as _,
      context_handle as *mut std::ffi::c_void,
    );
  }

  // Subscribe to selection changes
  let selection_notif = CFString::new("AXSelectedTextChanged");
  unsafe {
    let _ = AXObserverAddNotification(
      observer_ref,
      app_element.as_concrete_TypeRef(),
      selection_notif.as_concrete_TypeRef() as _,
      context_handle as *mut std::ffi::c_void,
    );
  }

  Ok((observer_ref, context_handle))
}

/// Callback for app-level notifications (Tier 1)
unsafe extern "C" fn app_observer_callback(
  _observer: AXObserverRef,
  element: accessibility_sys::AXUIElementRef,
  notification: core_foundation::string::CFStringRef,
  refcon: *mut std::ffi::c_void,
) {
  if refcon.is_null() || element.is_null() {
    return;
  }

  // Look up PID from the registry.
  // If not found, the app was already cleaned up - just return.
  let Some(pid) = lookup_app_context(refcon as *const AppObserverContextHandle) else {
    return;
  };

  let notif_cfstring = CFString::wrap_under_get_rule(notification);
  let notification_name = notif_cfstring.to_string();

  match notification_name.as_str() {
    "AXFocusedUIElementChanged" => {
      handle_app_focus_changed(pid, element);
    }
    "AXSelectedTextChanged" => {
      handle_app_selection_changed(pid, element);
    }
    _ => {}
  }
}

/// Check if an element should be auto-watched (Tier 2).
/// These are interactive elements whose value can change while focused.
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

/// Handle focus change at app level (Tier 1 + Tier 2)
fn handle_app_focus_changed(pid: u32, element_ref: accessibility_sys::AXUIElementRef) {
  let ax_element = unsafe { AXUIElement::wrap_under_get_rule(element_ref) };

  // Find the window this element belongs to
  let window_id = match get_window_id_from_ax_element(&ax_element) {
    Some(id) => id,
    None => return, // Element not in a tracked window
  };

  // Build and register the element
  let element = build_element(&ax_element, &window_id, pid, None);
  let new_is_watchable = should_auto_watch(&element.role);

  // Get previous focused element info and update state
  let (previous_element_id, previous_was_watchable) = {
    let mut observers = APP_OBSERVERS.lock();
    if let Some(state) = observers.get_mut(&pid) {
      let prev_id = state.focused_element_id.clone();
      let prev_was_watchable = state.focused_is_watchable;
      state.focused_element_id = Some(element.id.clone());
      state.focused_is_watchable = new_is_watchable;
      (prev_id, prev_was_watchable)
    } else {
      (None, false)
    }
  };

  // Tier 2: Auto-watch/unwatch interactive elements
  let same_element = previous_element_id.as_ref() == Some(&element.id);

  // Unwatch previous element (if different from new one)
  if previous_was_watchable && !same_element {
    if let Some(ref prev_id) = previous_element_id {
      crate::element_registry::ElementRegistry::unwatch(prev_id);
    }
  }

  // Watch new element (if watchable and different from previous)
  if new_is_watchable && !same_element {
    let _ = crate::element_registry::ElementRegistry::watch(&element.id);
  }

  // Emit focus:element event
  crate::events::emit_focus_element(
    &window_id,
    &element.id,
    &element,
    previous_element_id.as_ref(),
  );
}

/// Handle selection change at app level (Tier 1)
fn handle_app_selection_changed(pid: u32, element_ref: accessibility_sys::AXUIElementRef) {
  let ax_element = unsafe { AXUIElement::wrap_under_get_rule(element_ref) };

  // Find the window this element belongs to
  let window_id = match get_window_id_from_ax_element(&ax_element) {
    Some(id) => id,
    None => return,
  };

  // Build/get the element
  let element = build_element(&ax_element, &window_id, pid, None);

  // Get selected text
  let selected_text = ax_element
    .attribute(&AXAttribute::new(&CFString::new("AXSelectedText")))
    .ok()
    .and_then(|v| {
      // Check if it's a CFString
      unsafe {
        let type_id = core_foundation::base::CFGetTypeID(v.as_CFTypeRef());
        if type_id == CFString::type_id() {
          let cf_string = CFString::wrap_under_get_rule(v.as_CFTypeRef() as *const _);
          Some(cf_string.to_string())
        } else {
          None
        }
      }
    })
    .unwrap_or_default();

  // Get selected text range (if available)
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&ax_element)
  };

  // Always emit - empty text means selection was cleared
  crate::events::emit_selection_changed(&window_id, &element.id, &selected_text, range.as_ref());
}

/// Get selected text range from an element
fn get_selected_text_range(element: &AXUIElement) -> Option<crate::types::TextRange> {
  let range_attr = element
    .attribute(&AXAttribute::new(&CFString::new("AXSelectedTextRange")))
    .ok()?;

  // Try to extract CFRange from the AXValue
  unsafe {
    use core_foundation::base::CFRange;

    let value_ref = range_attr.as_CFTypeRef();
    let mut range = CFRange {
      location: 0,
      length: 0,
    };

    // AXValueGetValue with kAXValueTypeCFRange (4)
    let success = accessibility_sys::AXValueGetValue(
      value_ref as _,
      4, // kAXValueTypeCFRange
      &mut range as *mut _ as *mut std::ffi::c_void,
    );

    if success {
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
/// Used for initial sync state.
pub fn get_current_focus(
  pid: u32,
) -> (
  Option<crate::types::AXElement>,
  Option<crate::types::Selection>,
) {
  let app_element = AXUIElement::application(pid as i32);

  // Get the focused UI element
  let focused_attr = AXAttribute::new(&CFString::new("AXFocusedUIElement"));
  let focused_ref = match app_element.attribute(&focused_attr) {
    Ok(ref_val) => ref_val,
    Err(_) => return (None, None),
  };

  let element_ref = focused_ref.as_CFTypeRef() as accessibility_sys::AXUIElementRef;
  if element_ref.is_null() {
    return (None, None);
  }

  let ax_element = unsafe { AXUIElement::wrap_under_get_rule(element_ref) };

  // Find the window this element belongs to
  let window_id = match get_window_id_from_ax_element(&ax_element) {
    Some(id) => id,
    None => return (None, None),
  };

  // Build the element
  let element = build_element(&ax_element, &window_id, pid, None);

  // Get selected text (if any)
  let selection =
    get_element_selected_text(&ax_element).map(|(text, range)| crate::types::Selection {
      element_id: element.id.clone(),
      text,
      range,
    });

  (Some(element), selection)
}

/// Get selected text and range from an element (if it has a text selection)
fn get_element_selected_text(
  element: &AXUIElement,
) -> Option<(String, Option<crate::types::TextRange>)> {
  let selected_text = element
    .attribute(&AXAttribute::new(&CFString::new("AXSelectedText")))
    .ok()
    .and_then(|v| unsafe {
      let type_id = core_foundation::base::CFGetTypeID(v.as_CFTypeRef());
      if type_id == CFString::type_id() {
        let cf_string = CFString::wrap_under_get_rule(v.as_CFTypeRef() as *const _);
        let s = cf_string.to_string();
        if !s.is_empty() {
          Some(s)
        } else {
          None
        }
      } else {
        None
      }
    })?;

  let range = get_selected_text_range(element);
  Some((selected_text, range))
}

/// Get window ID from an AX element by looking at its window attribute
fn get_window_id_from_ax_element(element: &AXUIElement) -> Option<WindowId> {
  // Try to get the window attribute
  let window_element = element
    .attribute(&AXAttribute::new(&CFString::new("AXWindow")))
    .ok()?;

  // Get position of the window to match with our tracked windows
  let window_ax = unsafe { AXUIElement::wrap_under_get_rule(window_element.as_CFTypeRef() as _) };
  let bounds = get_element_bounds(&window_ax)?;

  // Find matching window by bounds
  crate::window_registry::find_by_bounds(&bounds)
}

/// Create an AXObserver for a process and add it to the main run loop.
pub fn create_observer_for_pid(pid: u32) -> AxioResult<AXObserverRef> {
  let mut observer_ref: AXObserverRef = std::ptr::null_mut();

  let result = unsafe {
    AXObserverCreate(
      pid as i32,
      observer_callback as _,
      &mut observer_ref as *mut _,
    )
  };

  if result != 0 {
    return Err(AxioError::ObserverError(format!(
      "AXObserverCreate failed with code {}",
      result
    )));
  }

  // Must add to MAIN run loop for callbacks to fire
  unsafe {
    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
    if run_loop_source_ref.is_null() {
      return Err(AxioError::ObserverError(
        "Failed to get run loop source".to_string(),
      ));
    }
    let run_loop_source = CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
    let main_run_loop = CFRunLoop::get_main();
    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);
  }

  Ok(observer_ref)
}

unsafe extern "C" fn observer_callback(
  _observer: AXObserverRef,
  _element: accessibility_sys::AXUIElementRef,
  notification: core_foundation::string::CFStringRef,
  refcon: *mut std::ffi::c_void,
) {
  // Catch any panics to prevent crashes
  let result = std::panic::catch_unwind(|| {
    if refcon.is_null() {
      return;
    }

    // Look up the element ID from the registry.
    // If not found, the element was already cleaned up - just return.
    let Some(element_id) = lookup_observer_context(refcon as *const ObserverContextHandle) else {
      return;
    };

    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();
    let changed_element = AXUIElement::wrap_under_get_rule(_element);

    handle_notification(&element_id, &notification_name, &changed_element);
  });

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
      // Get fresh value and update cached element
      if let Ok(value_attr) = ax_element.attribute(&AXAttribute::value()) {
        let role = ax_element
          .attribute(&AXAttribute::role())
          .ok()
          .map(|r| r.to_string());

        if let Some(value) = extract_value(&value_attr, role.as_deref()) {
          // Update cached element and emit
          if let Ok(mut element) = ElementRegistry::get(element_id) {
            element.value = Some(value);
            let _ = ElementRegistry::update(element_id, element.clone());
            crate::events::emit_element_changed(&element);
          }
        }
      }
    }

    AXNotification::TitleChanged => {
      if let Ok(title) = ax_element.attribute(&AXAttribute::title()) {
        let label = title.to_string();
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
      // Get element before removing (for event data)
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
// AXUIElement to AXElement Conversion
// ============================================================================

/// Extract a string from a CFType value
fn cftype_to_string(value: &core_foundation::base::CFType) -> Option<String> {
  unsafe {
    let type_id = core_foundation::base::CFGetTypeID(value.as_CFTypeRef());
    if type_id == CFString::type_id() {
      let cf_string = CFString::wrap_under_get_rule(value.as_CFTypeRef() as *const _);
      let s = cf_string.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    } else {
      None
    }
  }
}

/// Extract a boolean from a CFType value
fn cftype_to_bool(value: &core_foundation::base::CFType) -> Option<bool> {
  unsafe {
    use core_foundation::boolean::CFBoolean;
    let type_id = core_foundation::base::CFGetTypeID(value.as_CFTypeRef());
    if type_id == CFBoolean::type_id() {
      let cf_bool = CFBoolean::wrap_under_get_rule(value.as_CFTypeRef() as *const _);
      Some(cf_bool.into())
    } else {
      None
    }
  }
}

/// Build an AXElement from a macOS AXUIElement and register it.
/// Returns the registered element (may be existing if duplicate).
///
/// Uses batch attribute fetching for ~10x fewer IPC calls.
pub fn build_element(
  ax_element: &AXUIElement,
  window_id: &WindowId,
  pid: u32,
  parent_id: Option<&ElementId>,
) -> AXElement {
  use crate::element_registry::ElementRegistry;

  // Ensure Tier 1 app-level observer is set up for this app
  ensure_app_observer(pid);

  // Batch fetch all attributes in one IPC call
  let attrs = batch_fetch_attributes(ax_element);

  // Extract role
  let platform_role = attrs
    .get(&ATTR_ROLE)
    .and_then(cftype_to_string)
    .unwrap_or_else(|| "Unknown".to_string());
  let role = map_platform_role(&platform_role);

  // Extract subrole (only for unknown roles, or if present)
  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.clone())
  } else {
    attrs.get(&ATTR_SUBROLE).and_then(cftype_to_string)
  };

  // Extract label (title)
  let label = attrs.get(&ATTR_TITLE).and_then(cftype_to_string);

  // Extract value
  let value = attrs
    .get(&ATTR_VALUE)
    .and_then(|v| extract_value(v, Some(&platform_role)));

  // Extract description
  let description = attrs.get(&ATTR_DESCRIPTION).and_then(cftype_to_string);

  // Extract placeholder
  let placeholder = attrs.get(&ATTR_PLACEHOLDER).and_then(cftype_to_string);

  // Extract bounds from position + size
  let bounds = match (attrs.get(&ATTR_POSITION), attrs.get(&ATTR_SIZE)) {
    (Some(pos), Some(size)) => {
      let position = extract_position(pos);
      let size_val = extract_size(size);
      match (position, size_val) {
        (Some((x, y)), Some((w, h))) => Some(Bounds { x, y, w, h }),
        _ => None,
      }
    }
    _ => None,
  };

  // Extract focused
  let focused = attrs.get(&ATTR_FOCUSED).and_then(cftype_to_bool);

  // Extract enabled
  let enabled = attrs.get(&ATTR_ENABLED).and_then(cftype_to_bool);

  // Fetch actions (separate call, but typically returns quickly)
  let actions = get_element_actions(ax_element);

  let element = AXElement {
    id: ElementId::new(Uuid::new_v4().to_string()),
    window_id: window_id.clone(),
    parent_id: parent_id.cloned(),
    children: None, // Discovered separately
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

  // Register (returns existing if duplicate)
  ElementRegistry::register(element, ax_element.clone(), pid, &platform_role)
}

/// Discover and register children of an element. Updates parent's children.
/// Returns the child elements.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  use crate::element_registry::ElementRegistry;

  let (ax_element, window_id, pid) = ElementRegistry::with_stored(parent_id, |stored| {
    (
      stored.ax_element.clone(),
      stored.element.window_id.clone(),
      stored.pid,
    )
  })?;

  let children_array = match ax_element.attribute(&AXAttribute::children()) {
    Ok(children) => children,
    Err(_) => {
      ElementRegistry::set_children(parent_id, vec![])?;
      return Ok(vec![]);
    }
  };

  let child_count = children_array.len();
  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for i in 0..child_count.min(max_children as isize) {
    if let Some(child_ref) = children_array.get(i) {
      let child = build_element(&child_ref, &window_id, pid, Some(parent_id));
      child_ids.push(child.id.clone());
      children.push(child);
    }
  }

  ElementRegistry::set_children(parent_id, child_ids.clone())?;

  // Emit element:added for each new child
  for child in &children {
    crate::events::emit_element_added(child);
  }

  // Emit element:changed for the parent so client knows it now has children
  if let Ok(updated_parent) = ElementRegistry::get(parent_id) {
    crate::events::emit_element_changed(&updated_parent);
  }

  Ok(children)
}

/// Refresh an element's attributes from macOS.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
  use crate::element_registry::ElementRegistry;

  let (ax_element, window_id, _pid, parent_id, children, platform_role) =
    ElementRegistry::with_stored(element_id, |stored| {
      (
        stored.ax_element.clone(),
        stored.element.window_id.clone(),
        stored.pid,
        stored.element.parent_id.clone(),
        stored.element.children.clone(),
        stored.platform_role.clone(),
      )
    })?;

  let role = map_platform_role(&platform_role);

  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.clone())
  } else {
    ax_element
      .attribute(&AXAttribute::subrole())
      .ok()
      .map(|sr| sr.to_string())
      .filter(|s| !s.is_empty())
  };

  let label = ax_element
    .attribute(&AXAttribute::title())
    .ok()
    .and_then(|t| {
      let s = t.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    });

  let value = ax_element
    .attribute(&AXAttribute::value())
    .ok()
    .and_then(|v| extract_value(&v, Some(&platform_role)));

  let description = ax_element
    .attribute(&AXAttribute::description())
    .ok()
    .and_then(|d| {
      let s = d.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    });

  let placeholder = ax_element
    .attribute(&AXAttribute::placeholder_value())
    .ok()
    .and_then(|p| {
      let s = p.to_string();
      if s.is_empty() {
        None
      } else {
        Some(s)
      }
    });

  let bounds = get_element_bounds(&ax_element);
  let focused = ax_element
    .attribute(&AXAttribute::focused())
    .ok()
    .and_then(|f| f.try_into().ok());
  let enabled = ax_element
    .attribute(&AXAttribute::enabled())
    .ok()
    .and_then(|e| e.try_into().ok());

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
  // Remove "AX" prefix if present
  let role = platform_role
    .strip_prefix("AX")
    .unwrap_or(platform_role)
    .to_lowercase();

  match role.as_str() {
    // Document structure
    "application" => AXRole::Application,
    "window" | "standardwindow" => AXRole::Window,
    "group" | "scrollarea" => AXRole::Group,

    // Interactive elements
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

    // Static content
    "statictext" | "text" => AXRole::Text,
    "heading" => AXRole::Heading,
    "image" => AXRole::Image,
    "list" => AXRole::List,
    "listitem" | "row" => AXRole::Listitem,
    "table" => AXRole::Table,
    "cell" => AXRole::Cell,

    // Other
    "progressindicator" => AXRole::Progressbar,
    "scrollbar" => AXRole::Scrollbar,

    _ => AXRole::Unknown,
  }
}

/// Map macOS AX* action strings to platform-agnostic AXAction enum
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
    _ => None, // Unknown actions ignored
  }
}

/// Convert AXAction to macOS action string for performing actions
pub fn action_to_macos(action: AXAction) -> &'static str {
  match action {
    AXAction::Press => "AXPress",
    AXAction::ShowMenu => "AXShowMenu",
    AXAction::Increment => "AXIncrement",
    AXAction::Decrement => "AXDecrement",
    AXAction::Confirm => "AXConfirm",
    AXAction::Cancel => "AXCancel",
    AXAction::Raise => "AXRaise",
    AXAction::Pick => "AXPick",
  }
}

// ============================================================================
// Batch Attribute Fetching
// ============================================================================

use accessibility_sys::{
  kAXDescriptionAttribute, kAXEnabledAttribute, kAXFocusedAttribute, kAXPlaceholderValueAttribute,
  kAXRoleAttribute, kAXSubroleAttribute, kAXTitleAttribute, kAXValueAttribute,
  AXUIElementCopyActionNames, AXUIElementCopyMultipleAttributeValues,
};
use core_foundation::array::CFArray;
use std::collections::HashMap;

/// Attribute indices for batch fetch results
const ATTR_ROLE: usize = 0;
const ATTR_SUBROLE: usize = 1;
const ATTR_TITLE: usize = 2;
const ATTR_VALUE: usize = 3;
const ATTR_DESCRIPTION: usize = 4;
const ATTR_PLACEHOLDER: usize = 5;
const ATTR_POSITION: usize = 6;
const ATTR_SIZE: usize = 7;
const ATTR_FOCUSED: usize = 8;
const ATTR_ENABLED: usize = 9;

/// Batch fetch multiple attributes in a single IPC call.
/// Returns a map of attribute index to CFType value.
fn batch_fetch_attributes(element: &AXUIElement) -> HashMap<usize, core_foundation::base::CFType> {
  use core_foundation::base::CFType;

  let attr_names: &[&str] = &[
    kAXRoleAttribute,
    kAXSubroleAttribute,
    kAXTitleAttribute,
    kAXValueAttribute,
    kAXDescriptionAttribute,
    kAXPlaceholderValueAttribute,
    kAXPositionAttribute,
    kAXSizeAttribute,
    kAXFocusedAttribute,
    kAXEnabledAttribute,
  ];

  // Build CFArray of attribute names
  let cf_names: Vec<CFString> = attr_names.iter().map(|s| CFString::new(s)).collect();
  let cf_array = CFArray::from_CFTypes(&cf_names);

  let mut values_ref: core_foundation::array::CFArrayRef = std::ptr::null();

  let result = unsafe {
    AXUIElementCopyMultipleAttributeValues(
      element.as_concrete_TypeRef(),
      cf_array.as_concrete_TypeRef(),
      0, // 0 = continue on error, we want partial results
      &mut values_ref,
    )
  };

  let mut attrs = HashMap::new();

  if result != 0 || values_ref.is_null() {
    return attrs;
  }

  let values = unsafe { CFArray::<CFType>::wrap_under_create_rule(values_ref) };

  for (i, _) in attr_names.iter().enumerate() {
    if let Some(value) = values.get(i as isize) {
      // Check if it's kCFNull (missing attribute)
      let type_id = unsafe { core_foundation::base::CFGetTypeID(value.as_CFTypeRef()) };
      let null_type_id = unsafe { core_foundation::base::CFNullGetTypeID() };
      if type_id != null_type_id {
        // Clone the value by retaining it
        attrs.insert(i, value.clone());
      }
    }
  }

  attrs
}

/// Get available actions for an element
fn get_element_actions(element: &AXUIElement) -> Vec<AXAction> {
  let mut actions_ref: core_foundation::array::CFArrayRef = std::ptr::null();

  let result =
    unsafe { AXUIElementCopyActionNames(element.as_concrete_TypeRef(), &mut actions_ref) };

  if result != 0 || actions_ref.is_null() {
    return vec![];
  }

  let actions_array = unsafe { CFArray::<CFString>::wrap_under_create_rule(actions_ref) };
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

// ============================================================================
// Geometry Extraction
// ============================================================================

/// Extract geometry (position and size) from element
fn get_element_bounds(element: &AXUIElement) -> Option<Bounds> {
  // Get position
  let position_attr = CFString::new(kAXPositionAttribute);
  let ax_position_attr = AXAttribute::new(&position_attr);

  let position = element
    .attribute(&ax_position_attr)
    .ok()
    .and_then(|p| extract_position(&p))?;

  // Get size
  let size_attr = CFString::new(kAXSizeAttribute);
  let ax_size_attr = AXAttribute::new(&size_attr);

  let size = element
    .attribute(&ax_size_attr)
    .ok()
    .and_then(|s| extract_size(&s))?;

  Some(Bounds {
    x: position.0,
    y: position.1,
    w: size.0,
    h: size.1,
  })
}

/// Get accessibility tree for an application by PID
///
/// This is the main entry point for getting an accessibility tree
/// in AXIO format from a macOS application.
///
/// If `load_children` is false, returns only the root node with children_count populated.
/// Get all window AXUIElements for a given PID
///
/// Returns a vector of AXUIElements for each window (no CGWindowID).
/// We match windows by bounds instead of using private APIs.
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<AXUIElement>> {
  use core_foundation::string::CFString;

  let app_element = AXUIElement::application(pid as i32);

  // Get children of the application element
  let children_array = match app_element.attribute(&AXAttribute::children()) {
    Ok(children) => children,
    Err(_) => return Ok(Vec::new()),
  };

  let child_count = children_array.len();

  let mut result = Vec::new();

  // Filter children by role = "AXWindow"
  for i in 0..child_count {
    if let Some(child_element) = children_array.get(i) {
      // Check if role is "AXWindow"
      if let Ok(role) = child_element.attribute(&AXAttribute::role()) {
        let role_str = unsafe {
          let cf_string = CFString::wrap_under_get_rule(role.as_CFTypeRef() as *const _);
          cf_string.to_string()
        };

        if role_str == "AXWindow" {
          result.push((*child_element).clone());
        }
      }
    }
  }

  Ok(result)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  let (window, handle) = crate::window_registry::get_with_handle(window_id)
    .ok_or_else(|| AxioError::WindowNotFound(window_id.clone()))?;

  let window_element =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {} has no AX element", window_id)))?;

  Ok(build_element(
    &window_element,
    window_id,
    window.process_id.as_u32(),
    None,
  ))
}

/// Enable accessibility for an Electron app by setting AXManualAccessibility
/// This is necessary because Electron apps only expose their accessibility tree
/// when they detect assistive technology (like VoiceOver) is running.
///
/// Call this when a window is first discovered to give the accessibility tree
/// time to populate before querying elements.
pub fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  use core_foundation::boolean::CFBoolean;
  use core_foundation::string::CFString;

  let raw_pid = pid.as_u32();
  let app_element = AXUIElement::application(raw_pid as i32);
  let attr_name = CFString::new("AXManualAccessibility");

  // Try to set AXManualAccessibility to true
  let value = CFBoolean::true_value();

  unsafe {
    use accessibility_sys::{
      kAXErrorAttributeUnsupported, kAXErrorSuccess, AXUIElementSetAttributeValue,
    };
    use core_foundation::base::TCFType;

    let result = AXUIElementSetAttributeValue(
      app_element.as_concrete_TypeRef(),
      attr_name.as_concrete_TypeRef(),
      value.as_CFTypeRef(),
    );

    if result == kAXErrorSuccess {
      eprintln!("[axio] ✓ Enabled accessibility for PID {}", raw_pid);
    } else if result != kAXErrorAttributeUnsupported {
      // Only warn if it's not "attribute unsupported" (which is expected for native apps)
      eprintln!(
        "[axio] ⚠️  Failed to enable accessibility for PID {} (error: {})",
        raw_pid, result
      );
    }
    // Silently ignore kAXErrorAttributeUnsupported - it's expected for non-Electron apps
  }
}

/// Get the accessibility element at a specific screen position.
/// Queries only tracked windows (which exclude our own PID) to avoid hitting our overlay.
/// Uses AXUIElementCopyElementAtPosition as a starting point, then searches deeper
/// to find the most specific/interactive element at that position.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXElement> {
  use accessibility_sys::AXUIElementRef;
  use core_foundation::base::TCFType;
  use std::ptr;

  // Find which tracked window contains this point
  // Tracked windows already exclude our own PID, so we naturally skip our overlay
  let window = crate::window_registry::find_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!(
      "No tracked window found at position ({}, {})",
      x, y
    ))
  })?;

  let window_id = window.id.clone();
  let pid = window.process_id.as_u32();

  // Create an app element from the window's PID
  // Querying on the app element searches its entire hierarchy (all windows, all children)
  // Since we pre-filtered by tracked window bounds, we know we're hitting the right app
  let app_element = AXUIElement::application(pid as i32);

  unsafe {
    let mut element_ref: AXUIElementRef = ptr::null_mut();
    let result = AXUIElementCopyElementAtPosition(
      app_element.as_concrete_TypeRef(),
      x as f32,
      y as f32,
      &mut element_ref,
    );

    if result != 0 || element_ref.is_null() {
      return Err(AxioError::AccessibilityError(format!(
        "No element found at ({}, {}) in app {}",
        x, y, pid
      )));
    }

    let ax_element = AXUIElement::wrap_under_create_rule(element_ref);
    Ok(build_element(&ax_element, &window_id, pid, None))
  }
}

// ============================================================================
// AXValue Extraction (merged from ax_value.rs)
// ============================================================================
//
// FFI bindings for AXValue to properly extract CGPoint and CGSize from
// accessibility attributes. This provides safe wrappers around the macOS
// Accessibility API's AXValue functions which are not exposed by the
// `accessibility` crate.

use core_foundation::base::{CFType, CFTypeRef};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use std::os::raw::c_void;

// CGPoint and CGSize structs matching macOS CoreGraphics definitions
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

// AXValueType enum values
#[allow(non_upper_case_globals)]
const kAXValueTypeCGPoint: i32 = 1;
#[allow(non_upper_case_globals)]
const kAXValueTypeCGSize: i32 = 2;

// AXValue type (it's actually just a CFTypeRef under the hood)
type AXValueRef = CFTypeRef;

// External declarations for AXValue functions
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
  fn AXValueGetType(value: AXValueRef) -> i32;
  fn AXValueGetValue(value: AXValueRef, value_type: i32, value_ptr: *mut c_void) -> bool;
  fn AXUIElementCopyElementAtPosition(
    application: accessibility_sys::AXUIElementRef,
    x: f32,
    y: f32,
    element: *mut accessibility_sys::AXUIElementRef,
  ) -> i32;
}

/// Safely extract a CGPoint from a CFType that should be an AXValue
pub fn extract_position(cf_value: &impl TCFType) -> Option<(f64, f64)> {
  unsafe {
    let ax_value: AXValueRef = cf_value.as_CFTypeRef();

    // Check if this is a CGPoint type
    let value_type = AXValueGetType(ax_value);
    if value_type != kAXValueTypeCGPoint {
      return None;
    }

    // Extract the CGPoint
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let success = AXValueGetValue(
      ax_value,
      kAXValueTypeCGPoint,
      &mut point as *mut CGPoint as *mut c_void,
    );

    if success {
      Some((point.x, point.y))
    } else {
      None
    }
  }
}

/// Safely extract a CGSize from a CFType that should be an AXValue
pub fn extract_size(cf_value: &impl TCFType) -> Option<(f64, f64)> {
  unsafe {
    let ax_value: AXValueRef = cf_value.as_CFTypeRef();

    // Check if this is a CGSize type
    let value_type = AXValueGetType(ax_value);
    if value_type != kAXValueTypeCGSize {
      return None;
    }

    // Extract the CGSize
    let mut size = CGSize {
      width: 0.0,
      height: 0.0,
    };
    let success = AXValueGetValue(
      ax_value,
      kAXValueTypeCGSize,
      &mut size as *mut CGSize as *mut c_void,
    );

    if success {
      Some((size.width, size.height))
    } else {
      None
    }
  }
}

/// Properly extract a typed value from a CFType
/// Handles CFString, CFNumber, CFBoolean, and returns the appropriate typed value
///
/// For certain roles (toggles, checkboxes, radio buttons), 0/1 integers are converted to booleans
pub fn extract_value(cf_value: &impl TCFType, role: Option<&str>) -> Option<AXValue> {
  unsafe {
    let type_ref = cf_value.as_CFTypeRef();
    let cf_type = CFType::wrap_under_get_rule(type_ref);
    let type_id = cf_type.type_of();

    // Try CFString first (most common for values)
    if type_id == CFString::type_id() {
      let cf_string = CFString::wrap_under_get_rule(type_ref as *const _);
      let s = cf_string.to_string();
      // Filter out empty strings
      return if s.is_empty() {
        None
      } else {
        Some(AXValue::String(s))
      };
    }

    // Try CFNumber
    if type_id == CFNumber::type_id() {
      let cf_number = CFNumber::wrap_under_get_rule(type_ref as *const _);

      // For toggle-like elements, convert 0/1 integers to booleans
      if let Some(r) = role {
        if r == "AXToggle"
          || r == "AXCheckBox"
          || r == "AXRadioButton"
          || r.contains("Toggle")
          || r.contains("CheckBox")
          || r.contains("RadioButton")
        {
          if let Some(int_val) = cf_number.to_i64() {
            return Some(AXValue::Boolean(int_val != 0));
          }
        }
      }

      // Try to get as i64 first, then f64 if that fails
      if let Some(int_val) = cf_number.to_i64() {
        return Some(AXValue::Integer(int_val));
      } else if let Some(float_val) = cf_number.to_f64() {
        return Some(AXValue::Float(float_val));
      }
    }

    // Try CFBoolean
    if type_id == CFBoolean::type_id() {
      let cf_bool = CFBoolean::wrap_under_get_rule(type_ref as *const _);
      // CFBoolean can be converted to bool via Into trait
      let bool_val: bool = cf_bool.into();
      return Some(AXValue::Boolean(bool_val));
    }

    // For other types, we can't reliably extract them
    None
  }
}

// ============================================================================
// Element Operations (write, click, watch, unwatch)
// ============================================================================

/// Roles that support writing text values.
const WRITABLE_ROLES: &[&str] = &[
  "AXTextField",
  "AXTextArea",
  "AXComboBox",
  "AXSecureTextField",
  "AXSearchField",
];

/// Compare two AXUIElement handles for equality using CFEqual.
pub fn elements_equal(elem1: &AXUIElement, elem2: &AXUIElement) -> bool {
  use accessibility_sys::AXUIElementRef;
  use core_foundation::base::CFEqual;

  let ref1 = elem1.as_concrete_TypeRef() as AXUIElementRef;
  let ref2 = elem2.as_concrete_TypeRef() as AXUIElementRef;

  unsafe { CFEqual(ref1 as _, ref2 as _) != 0 }
}

/// Write a text value to an element.
/// Only works for text-input roles (AXTextField, AXTextArea, etc.)
pub fn write_element_value(
  ax_element: &AXUIElement,
  text: &str,
  platform_role: &str,
) -> AxioResult<()> {
  if !WRITABLE_ROLES.contains(&platform_role) {
    return Err(AxioError::NotSupported(format!(
      "Element with role '{}' is not writable",
      platform_role
    )));
  }

  let cf_string = CFString::new(text);
  ax_element
    .set_attribute(&AXAttribute::value(), cf_string.as_CFType())
    .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {:?}", e)))?;

  Ok(())
}

/// Perform a click (press) action on an element.
pub fn click_element(ax_element: &AXUIElement) -> AxioResult<()> {
  use accessibility_sys::{kAXPressAction, AXUIElementPerformAction};

  let action = CFString::new(kAXPressAction);
  let result = unsafe {
    AXUIElementPerformAction(
      ax_element.as_concrete_TypeRef(),
      action.as_concrete_TypeRef(),
    )
  };

  if result == 0 {
    Ok(())
  } else {
    Err(AxioError::AccessibilityError(format!(
      "AXUIElementPerformAction failed with code {}",
      result
    )))
  }
}

/// Register notifications for an element and return the subscribed notifications.
/// Returns empty vec if no notifications could be registered.
pub fn subscribe_element_notifications(
  element_id: &ElementId,
  ax_element: &AXUIElement,
  platform_role: &str,
  observer: AXObserverRef,
) -> AxioResult<(*mut ObserverContextHandle, Vec<AXNotification>)> {
  use accessibility_sys::{AXObserverAddNotification, AXUIElementRef};
  use std::ffi::c_void;

  let notifications = AXNotification::for_role(platform_role);
  if notifications.is_empty() {
    return Ok((std::ptr::null_mut(), Vec::new()));
  }

  let context_handle = register_observer_context(element_id.clone());
  let element_ref = ax_element.as_concrete_TypeRef() as AXUIElementRef;

  let mut registered = Vec::new();
  for notification in &notifications {
    let notif_cfstring = CFString::new(notification.as_str());
    let result = unsafe {
      AXObserverAddNotification(
        observer,
        element_ref,
        notif_cfstring.as_concrete_TypeRef() as _,
        context_handle as *mut c_void,
      )
    };
    if result == 0 {
      registered.push(*notification);
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
  ax_element: &AXUIElement,
  observer: AXObserverRef,
  context_handle: *mut ObserverContextHandle,
  notifications: &[AXNotification],
) {
  use accessibility_sys::{AXObserverRemoveNotification, AXUIElementRef};

  let element_ref = ax_element.as_concrete_TypeRef() as AXUIElementRef;

  for notification in notifications {
    let notif_cfstring = CFString::new(notification.as_str());
    unsafe {
      let _ = AXObserverRemoveNotification(
        observer,
        element_ref,
        notif_cfstring.as_concrete_TypeRef() as _,
      );
    }
  }

  // Unregister from context registry.
  // If a callback is in-flight, it will safely fail the lookup and return early.
  unregister_observer_context(context_handle);
}

/// Fetch the AXUIElement handle for a window by matching bounds.
pub fn fetch_window_handle(window: &crate::AXWindow) -> Option<AXUIElement> {
  let window_elements = get_window_elements(window.process_id.as_u32()).ok()?;

  if window_elements.is_empty() {
    return None;
  }

  const MARGIN: f64 = 2.0;

  for element in window_elements.iter() {
    let position_attr = CFString::new(kAXPositionAttribute);
    let ax_position_attr = AXAttribute::new(&position_attr);
    let element_pos = element
      .attribute(&ax_position_attr)
      .ok()
      .and_then(|p| extract_position(&p));

    let size_attr = CFString::new(kAXSizeAttribute);
    let ax_size_attr = AXAttribute::new(&size_attr);
    let element_size = element
      .attribute(&ax_size_attr)
      .ok()
      .and_then(|s| extract_size(&s));

    if let (Some((ax_x, ax_y)), Some((ax_w, ax_h))) = (element_pos, element_size) {
      let element_bounds = Bounds {
        x: ax_x,
        y: ax_y,
        w: ax_w,
        h: ax_h,
      };
      if window.bounds.matches(&element_bounds, MARGIN) {
        return Some(element.clone());
      }
    }
  }

  // Fallback: use only element if there's just one
  if window_elements.len() == 1 {
    return Some(window_elements[0].clone());
  }

  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cgpoint_size() {
    // Verify that CGPoint has the expected layout
    assert_eq!(std::mem::size_of::<CGPoint>(), 16); // 2 * f64
  }

  #[test]
  fn test_cgsize_size() {
    // Verify that CGSize has the expected layout
    assert_eq!(std::mem::size_of::<CGSize>(), 16); // 2 * f64
  }
}
