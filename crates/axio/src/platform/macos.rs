/**
 * macOS Platform Implementation
 */
use objc2_application_services::{
  AXError, AXIsProcessTrusted, AXObserver, AXObserverCallback, AXUIElement, AXValue as AXValueRef,
  AXValueType,
};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFBoolean, CFHash, CFRange, CFRetained, CFRunLoop, CFString,
};
use std::ffi::c_void;
use std::ptr::NonNull;

use super::handles::{ElementHandle, ObserverHandle};
use crate::events::emit;
use crate::types::{AXElement, AxioError, AxioResult, ElementId, Event, WindowId};

/// Create an AXUIElement for an application by PID.
/// Encapsulates the unsafe FFI call.
fn app_element(pid: u32) -> CFRetained<AXUIElement> {
  unsafe { AXUIElement::new_application(pid as i32) }
}

/// Check if accessibility permissions are granted.
/// Returns true if trusted, false otherwise.
pub fn check_accessibility_permissions() -> bool {
  unsafe { AXIsProcessTrusted() }
}

// AXNotification enum has been replaced by:
// - crate::accessibility::Notification (cross-platform enum)
// - crate::platform::macos_platform::mapping (macOS string mappings)

// ============================================================================
// Unified Context Registry - Safe callback handling for macOS observers
// ============================================================================

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::LazyLock;

use crate::accessibility::Notification;
use crate::platform::macos_platform::mapping::{
  ax_action, ax_role, notification_from_macos, notification_to_macos,
};

/// Next available context ID
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Unified observer context - either element-level or process-level.
///
/// Element contexts are used for per-element notifications (destruction, value change).
/// Process contexts are used for app-level notifications (focus, selection).
#[derive(Clone)]
pub enum ObserverContext {
  /// Element-level notification (context identifies which element)
  Element(ElementId),
  /// Process-level notification (context identifies which app)
  Process(u32),
}

/// Opaque handle passed to macOS callbacks - contains only an ID.
#[repr(C)]
pub struct ContextHandle {
  context_id: u64,
}

pub type ObserverContextHandle = ContextHandle;

/// Unified registry for observer contexts.
static OBSERVER_CONTEXTS: LazyLock<Mutex<HashMap<u64, ObserverContext>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register an element context and get a raw pointer handle.
pub fn register_observer_context(element_id: ElementId) -> *mut ObserverContextHandle {
  register_context(ObserverContext::Element(element_id))
}

/// Register a process context and get a raw pointer handle.
pub fn register_process_context(pid: u32) -> *mut ObserverContextHandle {
  register_context(ObserverContext::Process(pid))
}

fn register_context(ctx: ObserverContext) -> *mut ObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  OBSERVER_CONTEXTS.lock().insert(context_id, ctx);
  Box::into_raw(Box::new(ContextHandle { context_id }))
}

/// Unregister and free a context handle.
pub fn unregister_observer_context(handle_ptr: *mut ObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    OBSERVER_CONTEXTS.lock().remove(&handle.context_id);
  }
}

/// Look up context from handle (for use in callbacks).
fn lookup_context(handle_ptr: *const ObserverContextHandle) -> Option<ObserverContext> {
  if handle_ptr.is_null() {
    return None;
  }
  unsafe {
    let handle = &*handle_ptr;
    OBSERVER_CONTEXTS.lock().get(&handle.context_id).cloned()
  }
}

// ============================================================================
// Observer Management
// ============================================================================
//
// One observer per process, managed by Registry.ProcessState.
// App-level notifications (focus, selection) are subscribed in Registry.get_or_create_process().

/// Create an AXObserver and add it to the main run loop.
/// This is the core observer creation logic shared by all observer types.
fn create_observer_raw(
  pid: u32,
  callback: AXObserverCallback,
) -> AxioResult<CFRetained<AXObserver>> {
  let observer = unsafe {
    let mut observer_ptr: *mut AXObserver = std::ptr::null_mut();
    let result = AXObserver::create(
      pid as i32,
      callback,
      NonNull::new_unchecked(&mut observer_ptr),
    );

    if result != AXError::Success {
      return Err(AxioError::ObserverError(format!(
        "AXObserverCreate failed for PID {pid} with code {result:?}"
      )));
    }

    CFRetained::from_raw(
      NonNull::new(observer_ptr)
        .ok_or_else(|| AxioError::ObserverError("AXObserverCreate returned null".to_string()))?,
    )
  };

  // Add to main run loop - required for callbacks to fire
  unsafe {
    let run_loop_source = observer.run_loop_source();
    if let Some(main_run_loop) = CFRunLoop::main() {
      main_run_loop.add_source(Some(&run_loop_source), kCFRunLoopDefaultMode);
    }
  }

  Ok(observer)
}

fn should_auto_watch(role: &crate::accessibility::Role) -> bool {
  role.auto_watch_on_focus() || role.is_writable()
}

/// Handle focus change notification from the unified callback.
pub fn handle_app_focus_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle, pid) {
    Some(id) => id,
    None => {
      // Expected when desktop is focused or window not yet tracked
      log::debug!("FocusChanged: no window_id found for PID {}, skipping", pid);
      return;
    }
  };

  let Some(ax_element) = build_element_from_handle(handle, &window_id, pid, None) else {
    log::warn!("FocusChanged: element build failed for PID {}", pid);
    return;
  };

  let new_is_watchable = should_auto_watch(&ax_element.role);

  // Update focus in registry, get previous
  let previous_element_id = crate::registry::set_process_focus(pid, ax_element.id);
  let same_element = previous_element_id.as_ref() == Some(&ax_element.id);

  // Auto-watch/unwatch based on role
  if !same_element {
    if let Some(ref prev_id) = previous_element_id {
      // Check if previous was watchable before unwatching
      if let Ok(prev_elem) = crate::registry::get_element(prev_id) {
        if should_auto_watch(&prev_elem.role) {
          crate::registry::unwatch_element(prev_id);
        }
      }
    }

    if new_is_watchable {
      let _ = crate::registry::watch_element(&ax_element.id);
    }
  }

  emit(Event::FocusElement {
    element: ax_element,
    previous_element_id,
  });
}

/// Handle selection change notification from the unified callback.
pub fn handle_app_selection_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle, pid) {
    Some(id) => id,
    None => {
      // Expected when desktop is focused or window not yet tracked
      log::debug!(
        "SelectionChanged: no window_id found for PID {}, skipping",
        pid
      );
      return;
    }
  };

  let Some(ax_element) = build_element_from_handle(handle.clone(), &window_id, pid, None) else {
    log::warn!("SelectionChanged: element build failed for PID {}", pid);
    return;
  };

  let selected_text = handle.get_string("AXSelectedText").unwrap_or_default();
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&handle)
  };

  emit(Event::SelectionChanged {
    window_id,
    element_id: ax_element.id,
    text: selected_text,
    range,
  });
}

fn get_selected_text_range(handle: &ElementHandle) -> Option<crate::types::TextRange> {
  let attr_name = CFString::from_static_str("AXSelectedTextRange");
  let value = handle.get_raw_attr_internal(&attr_name)?;

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
  // Create ElementHandle for app element
  let app_handle = ElementHandle::new(app_element(pid));

  // Use safe ElementHandle method to get focused element
  let Some(focused_handle) = app_handle.get_element("AXFocusedUIElement") else {
    return (None, None);
  };

  let window_id = match get_window_id_for_handle(&focused_handle, pid) {
    Some(id) => id,
    None => return (None, None),
  };

  let Some(element) = build_element_from_handle(focused_handle.clone(), &window_id, pid, None)
  else {
    return (None, None); // Element was previously destroyed
  };

  // Get selection using handle method
  let selection =
    get_selection_from_handle(&focused_handle).map(|(text, range)| crate::types::Selection {
      element_id: element.id,
      text,
      range,
    });

  (Some(element), selection)
}

/// Get window ID for an ElementHandle using hash-based lookup.
/// Gets the AXWindow element, hashes it, and looks up in the registry.
fn get_window_id_for_handle(handle: &ElementHandle, pid: u32) -> Option<WindowId> {
  // First: check if element is already registered (by hash)
  let element_hash = element_hash(handle);
  if let Some(element) = crate::registry::get_element_by_hash(element_hash) {
    return Some(element.window_id);
  }

  // Fallback: use the currently focused window for this PID
  // This works because focus/selection events only come from the focused app
  crate::registry::get_focused_window_for_pid(pid)
}

/// Get selected text and range from an element handle.
fn get_selection_from_handle(
  handle: &ElementHandle,
) -> Option<(String, Option<crate::types::TextRange>)> {
  let selected_text = handle.get_string("AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  // TODO: Parse AXSelectedTextRange if needed
  Some((selected_text, None))
}

/// Create an observer for a process and add it to the main run loop.
/// Uses the unified callback that handles both element-level and app-level notifications.
pub fn create_observer_for_pid(pid: u32) -> AxioResult<ObserverHandle> {
  let observer = create_observer_raw(pid, Some(unified_observer_callback))?;
  Ok(ObserverHandle::new(observer))
}

/// Unified observer callback - handles both element-level and app-level notifications.
///
/// Dispatches based on context type:
/// - Element context → element-level notifications (destruction, value change, title change)
/// - Process context → app-level notifications (focus change, selection change)
unsafe extern "C-unwind" fn unified_observer_callback(
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

    let notification_str = notification.as_ref().to_string();
    let element_ref = CFRetained::retain(element);

    // Convert macOS string to our Notification type
    let Some(notif) = notification_from_macos(&notification_str) else {
      log::warn!("Unknown macOS notification: {}", notification_str);
      return;
    };

    // Look up unified context and dispatch based on type
    let Some(ctx) = lookup_context(refcon as *const ObserverContextHandle) else {
      return;
    };

    match ctx {
      ObserverContext::Element(element_id) => {
        handle_element_notification(&element_id, notif, element_ref);
      }
      ObserverContext::Process(pid) => {
        handle_process_notification(pid, notif, element_ref);
      }
    }
  }));

  if result.is_err() {
    log::warn!("Accessibility notification handler panicked (possibly invalid element)");
  }
}

/// Handle element-level notifications (destruction, value change, title change).
fn handle_element_notification(
  element_id: &ElementId,
  notif: Notification,
  ax_element: CFRetained<AXUIElement>,
) {
  match notif {
    Notification::ValueChanged => {
      let handle = ElementHandle::new(ax_element);
      let attrs = handle.get_attributes(None);
      if let Some(value) = attrs.value {
        if let Ok(mut element) = crate::registry::get_element(element_id) {
          element.value = Some(value);
          let _ = crate::registry::update_element(element_id, element.clone());
          emit(Event::ElementChanged {
            element: element.clone(),
          });
        }
      }
    }

    Notification::TitleChanged => {
      let handle = ElementHandle::new(ax_element);
      if let Some(label) = handle.get_string("AXTitle") {
        if !label.is_empty() {
          if let Ok(mut element) = crate::registry::get_element(element_id) {
            element.label = Some(label);
            let _ = crate::registry::update_element(element_id, element.clone());
            emit(Event::ElementChanged {
              element: element.clone(),
            });
          }
        }
      }
    }

    Notification::Destroyed => {
      log::debug!("Destroyed notification for element {}", element_id);
      crate::registry::remove_element(element_id);
      // Event is emitted by registry
    }

    _ => {}
  }
}

/// Handle app/process-level notifications (focus change, selection change).
fn handle_process_notification(pid: u32, notif: Notification, ax_element: CFRetained<AXUIElement>) {
  match notif {
    Notification::FocusChanged => {
      handle_app_focus_changed(pid, ax_element);
    }
    Notification::SelectionChanged => {
      handle_app_selection_changed(pid, ax_element);
    }
    _ => {}
  }
}

// ============================================================================
// AXUIElement to AXElement Conversion
// ============================================================================

/// Build an AXElement from an ElementHandle and register it.
/// Uses batch attribute fetching for ~10x faster element creation.
/// All unsafe code is encapsulated in ElementHandle methods.
/// Returns None if the element's hash is in the dead set (was previously destroyed).
pub fn build_element_from_handle(
  handle: ElementHandle,
  window_id: &WindowId,
  pid: u32,
  parent_id: Option<&ElementId>,
) -> Option<AXElement> {
  // Fetch all attributes in ONE IPC call - safe method!
  let attrs = handle.get_attributes(None);

  let platform_role = attrs.role.clone().unwrap_or_else(|| "Unknown".to_string());
  let role = crate::platform::macos_platform::mapping::role_from_macos(&platform_role);

  let subrole = if matches!(role, crate::accessibility::Role::Unknown) {
    Some(platform_role.clone())
  } else {
    attrs.subrole
  };

  let element = AXElement {
    id: ElementId::new(),
    window_id: *window_id,
    parent_id: parent_id.copied(),
    children: None,
    role,
    subrole,
    label: attrs.title,
    value: attrs.value,
    description: attrs.description,
    placeholder: attrs.placeholder,
    bounds: attrs.bounds,
    focused: attrs.focused,
    enabled: attrs.enabled,
    actions: attrs.actions,
  };

  crate::registry::register_element(element, handle, pid, &platform_role)
}

/// Discover and register children of an element.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  let info = crate::registry::get_stored_element_info(parent_id)?;

  // Use safe ElementHandle method
  let child_handles = info.handle.get_children();
  if child_handles.is_empty() {
    crate::registry::set_element_children(parent_id, vec![])?;
    return Ok(vec![]);
  }

  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for child_handle in child_handles.into_iter().take(max_children) {
    // Skip children that were previously destroyed
    if let Some(child) =
      build_element_from_handle(child_handle, &info.window_id, info.pid, Some(parent_id))
    {
      child_ids.push(child.id);
      children.push(child);
    }
  }

  crate::registry::set_element_children(parent_id, child_ids)?;

  for child in &children {
    emit(Event::ElementAdded {
      element: child.clone(),
    });
  }

  if let Ok(updated_parent) = crate::registry::get_element(parent_id) {
    emit(Event::ElementChanged {
      element: updated_parent.clone(),
    });
  }

  Ok(children)
}

/// Refresh an element's attributes from the platform.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
  let info = crate::registry::get_stored_element_info(element_id)?;

  // Use safe ElementHandle method for batch attribute fetch
  let attrs = info.handle.get_attributes(Some(&info.platform_role));

  let role = crate::platform::macos_platform::mapping::role_from_macos(&info.platform_role);
  let subrole = if matches!(role, crate::accessibility::Role::Unknown) {
    Some(info.platform_role.to_string())
  } else {
    attrs.subrole
  };

  let updated = AXElement {
    id: *element_id,
    window_id: info.window_id,
    parent_id: info.parent_id,
    children: info.children,
    role,
    subrole,
    label: attrs.title,
    value: attrs.value,
    description: attrs.description,
    placeholder: attrs.placeholder,
    bounds: attrs.bounds,
    focused: attrs.focused,
    enabled: attrs.enabled,
    actions: attrs.actions,
  };

  crate::registry::update_element(element_id, updated.clone())?;
  Ok(updated)
}

// ============================================================================
// Window Elements
// ============================================================================

/// Get all window ElementHandles for a given PID.
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<ElementHandle>> {
  let app_handle = ElementHandle::new(app_element(pid));
  let children = app_handle.get_children();

  let windows = children
    .into_iter()
    .filter(|child| child.get_string("AXRole").as_deref() == Some(ax_role::WINDOW))
    .collect();

  Ok(windows)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  let (window, handle) = crate::registry::get_window_with_handle(window_id)
    .ok_or_else(|| AxioError::WindowNotFound(*window_id))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {window_id} has no AX element")))?;

  // Clone handle for safe method use
  build_element_from_handle(window_handle.clone(), window_id, window.process_id.0, None)
    .ok_or_else(|| AxioError::Internal("Window root element was previously destroyed".to_string()))
}

/// Enable accessibility for an Electron app
pub fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  let raw_pid = pid.0;
  let app_element = app_element(raw_pid);
  let attr_name = CFString::from_static_str("AXManualAccessibility");
  let value = CFBoolean::new(true);

  unsafe {
    let result = app_element.set_attribute_value(&attr_name, value);

    if result == AXError::Success {
      log::debug!("Enabled accessibility for PID {raw_pid}");
    } else if result != AXError::AttributeUnsupported {
      log::warn!("Failed to enable accessibility for PID {raw_pid} (error: {result:?})");
    }
  }
}

/// Get the accessibility element at a specific screen position.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXElement> {
  let window = crate::registry::find_window_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No tracked window found at position ({x}, {y})"))
  })?;

  let window_id = window.id;
  let pid = window.process_id.0;

  // Use safe ElementHandle method
  let app_handle = ElementHandle::new(app_element(pid));
  let element_handle = app_handle.element_at_position(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No element found at ({x}, {y}) in app {pid}"))
  })?;

  build_element_from_handle(element_handle, &window_id, pid, None).ok_or_else(|| {
    AxioError::AccessibilityError(format!("Element at ({x}, {y}) was previously destroyed"))
  })
}

// ============================================================================
// Element Operations
// ============================================================================

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
  // Use Role::is_writable() for writability check
  let role = crate::platform::macos_platform::mapping::role_from_macos(platform_role);
  if !role.is_writable() {
    return Err(AxioError::NotSupported(format!(
      "Element with role '{platform_role}' is not writable"
    )));
  }

  handle
    .set_value(text)
    .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {e:?}")))
}

/// Perform a click (press) action on an element.
pub fn click_element(handle: &ElementHandle) -> AxioResult<()> {
  handle
    .perform_action(ax_action::PRESS)
    .map_err(|e| AxioError::AccessibilityError(format!("AXPress failed: {e:?}")))
}

/// Subscribe to destruction notification only (lightweight tracking for all elements).
pub fn subscribe_destruction_notification(
  element_id: &ElementId,
  handle: &ElementHandle,
  observer: ObserverHandle,
) -> AxioResult<*mut ObserverContextHandle> {
  let context_handle = register_observer_context(*element_id);

  let notif_str = notification_to_macos(Notification::Destroyed);
  let notif_cfstring = CFString::from_str(notif_str);
  let result = unsafe {
    observer.inner().add_notification(
      handle.inner(),
      &notif_cfstring,
      context_handle as *mut c_void,
    )
  };

  if result != AXError::Success {
    unregister_observer_context(context_handle);
    return Err(AxioError::ObserverError(format!(
      "Failed to register destruction notification for element {element_id}: {result:?}"
    )));
  }

  Ok(context_handle)
}

/// Unsubscribe from destruction notification.
pub fn unsubscribe_destruction_notification(
  handle: &ElementHandle,
  observer: ObserverHandle,
  context_handle: *mut ObserverContextHandle,
) {
  let notif_str = notification_to_macos(Notification::Destroyed);
  let notif_cfstring = CFString::from_str(notif_str);
  unsafe {
    let _ = observer
      .inner()
      .remove_notification(handle.inner(), &notif_cfstring);
  }
  unregister_observer_context(context_handle);
}

// =============================================================================
// Notification API
// =============================================================================

/// Subscribe to notifications for an element.
pub fn subscribe_notifications(
  element_id: &ElementId,
  handle: &ElementHandle,
  observer: ObserverHandle,
  _platform_role: &str,
  notifications: &[Notification],
) -> AxioResult<*mut ObserverContextHandle> {
  if notifications.is_empty() {
    return Err(AxioError::NotSupported(
      "No notifications to subscribe".into(),
    ));
  }

  let context_handle = register_observer_context(*element_id);

  let mut registered = 0;
  for notification in notifications {
    let notif_str = notification_to_macos(*notification);
    let notif_cfstring = CFString::from_str(notif_str);
    unsafe {
      let result = observer.inner().add_notification(
        handle.inner(),
        &notif_cfstring,
        context_handle as *mut c_void,
      );
      if result == AXError::Success {
        registered += 1;
      }
    }
  }

  if registered == 0 {
    unregister_observer_context(context_handle);
    return Err(AxioError::ObserverError(
      "Failed to register any notifications".into(),
    ));
  }

  Ok(context_handle)
}

/// Unsubscribe from notifications (using new Notification type).
pub fn unsubscribe_notifications(
  handle: &ElementHandle,
  observer: ObserverHandle,
  context_handle: *mut ObserverContextHandle,
  notifications: &[Notification],
) {
  for notification in notifications {
    let notif_str = notification_to_macos(*notification);
    let notif_cfstring = CFString::from_str(notif_str);
    unsafe {
      let _ = observer
        .inner()
        .remove_notification(handle.inner(), &notif_cfstring);
    }
  }

  unregister_observer_context(context_handle);
}

/// Subscribe to app-level notifications (focus, selection) on the application element.
/// Returns a context handle for the subscription.
pub fn subscribe_app_notifications(
  pid: u32,
  observer: &ObserverHandle,
) -> AxioResult<*mut ObserverContextHandle> {
  let app_el = app_element(pid);
  let context_handle = register_process_context(pid);

  // Subscribe to focus and selection changes on the app element
  let notifications = [Notification::FocusChanged, Notification::SelectionChanged];
  let mut registered = 0;

  for notif in &notifications {
    let notif_str = notification_to_macos(*notif);
    let notif_cfstring = CFString::from_str(notif_str);
    unsafe {
      let result =
        observer
          .inner()
          .add_notification(&app_el, &notif_cfstring, context_handle as *mut c_void);
      if result == AXError::Success {
        registered += 1;
      }
    }
  }

  if registered == 0 {
    unregister_observer_context(context_handle);
    return Err(AxioError::ObserverError(format!(
      "Failed to subscribe to app notifications for PID {pid}"
    )));
  }

  log::debug!(
    "Subscribed to {}/{} app-level notifications for PID {}",
    registered,
    notifications.len(),
    pid
  );
  Ok(context_handle)
}

/// Fetch an element handle for a window by matching bounds.
pub fn fetch_window_handle(window: &crate::AXWindow) -> Option<ElementHandle> {
  let window_elements = get_window_elements(window.process_id.0).ok()?;

  if window_elements.is_empty() {
    return None;
  }

  const MARGIN: f64 = 2.0;

  for element in window_elements.iter() {
    if let Some(element_bounds) = element.get_bounds() {
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
