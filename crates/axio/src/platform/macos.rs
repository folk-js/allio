/**
 * macOS Platform Implementation
 */
use objc2_application_services::{
  AXError, AXIsProcessTrusted, AXObserver, AXObserverCallback, AXUIElement, AXValue as AXValueRef,
  AXValueType,
};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFBoolean, CFHash, CFNumber, CFRange, CFRetained, CFRunLoop, CFString,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use uuid::Uuid;

use super::handles::{ElementHandle, ObserverHandle};
use crate::events::emit;
use crate::types::{AXElement, AXRole, AxioError, AxioResult, ElementId, Event, WindowId};

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
// Generic Context Registry - Safe callback handling for macOS observers
// ============================================================================

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap as StdHashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// Next available context ID (shared across all registries)
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Generic registry for mapping context IDs to values.
/// Used to safely pass data through macOS C callbacks via opaque pointers.
struct ContextRegistry<T>(Mutex<StdHashMap<u64, T>>);

impl<T: Clone> ContextRegistry<T> {
  /// Register a value and get a raw pointer handle for use in C callbacks.
  fn register(&self, value: T) -> *mut ContextHandle {
    let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
    self.0.lock().insert(context_id, value);
    Box::into_raw(Box::new(ContextHandle { context_id }))
  }

  /// Unregister and free a context handle.
  fn unregister(&self, handle_ptr: *mut ContextHandle) {
    if handle_ptr.is_null() {
      return;
    }
    unsafe {
      let handle = Box::from_raw(handle_ptr);
      self.0.lock().remove(&handle.context_id);
    }
  }

  /// Look up value from context handle (for use in callbacks).
  fn lookup(&self, handle_ptr: *const ContextHandle) -> Option<T> {
    if handle_ptr.is_null() {
      return None;
    }
    unsafe {
      let handle = &*handle_ptr;
      self.0.lock().get(&handle.context_id).cloned()
    }
  }
}

/// Opaque handle passed to macOS callbacks - contains only an ID.
#[repr(C)]
pub struct ContextHandle {
  context_id: u64,
}

// Type aliases for the two registries we need
pub type ObserverContextHandle = ContextHandle;
pub type AppObserverContextHandle = ContextHandle;

/// Registry for element observer contexts (ElementId)
static ELEMENT_CONTEXTS: Lazy<ContextRegistry<ElementId>> =
  Lazy::new(|| ContextRegistry(Mutex::new(StdHashMap::new())));

/// Registry for app observer contexts (PID)
static APP_CONTEXTS: Lazy<ContextRegistry<u32>> =
  Lazy::new(|| ContextRegistry(Mutex::new(StdHashMap::new())));

// Public API for element contexts
pub fn register_observer_context(element_id: ElementId) -> *mut ObserverContextHandle {
  ELEMENT_CONTEXTS.register(element_id)
}

pub fn unregister_observer_context(handle_ptr: *mut ObserverContextHandle) {
  ELEMENT_CONTEXTS.unregister(handle_ptr)
}

fn lookup_observer_context(handle_ptr: *const ObserverContextHandle) -> Option<ElementId> {
  ELEMENT_CONTEXTS.lookup(handle_ptr)
}

// Internal API for app contexts
fn register_app_context(pid: u32) -> *mut AppObserverContextHandle {
  APP_CONTEXTS.register(pid)
}

fn unregister_app_context(handle_ptr: *mut AppObserverContextHandle) {
  APP_CONTEXTS.unregister(handle_ptr)
}

fn lookup_app_context(handle_ptr: *const AppObserverContextHandle) -> Option<u32> {
  APP_CONTEXTS.lookup(handle_ptr)
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
        "AXObserverCreate failed for PID {} with code {:?}",
        pid, result
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

/// Create an app-level observer for Tier 1 notifications.
fn create_app_observer(
  pid: u32,
) -> AxioResult<(CFRetained<AXObserver>, *mut AppObserverContextHandle)> {
  let observer = create_observer_raw(pid, Some(app_observer_callback))?;

  // Subscribe to app-level notifications on the application element
  let app_el = app_element(pid);
  let context_handle = register_app_context(pid);

  // Subscribe to focus and selection changes
  for notif in ["AXFocusedUIElementChanged", "AXSelectedTextChanged"] {
    let notif_str = CFString::from_static_str(notif);
    unsafe {
      let _ = observer.add_notification(&app_el, &notif_str, context_handle as *mut c_void);
    }
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
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle) {
    Some(id) => id,
    None => return,
  };

  let ax_element = build_element_from_handle(handle, &window_id, pid, None);
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

  emit(Event::FocusElement {
    element: ax_element,
    previous_element_id,
  });
}

fn handle_app_selection_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle) {
    Some(id) => id,
    None => return,
  };

  let ax_element = build_element_from_handle(handle.clone(), &window_id, pid, None);

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

  let window_id = match get_window_id_for_handle(&focused_handle) {
    Some(id) => id,
    None => return (None, None),
  };

  let element = build_element_from_handle(focused_handle.clone(), &window_id, pid, None);

  // Get selection using handle method
  let selection =
    get_selection_from_handle(&focused_handle).map(|(text, range)| crate::types::Selection {
      element_id: element.id.clone(),
      text,
      range,
    });

  (Some(element), selection)
}

/// Get window ID for an ElementHandle.
fn get_window_id_for_handle(handle: &ElementHandle) -> Option<WindowId> {
  let window_handle = handle.get_element("AXWindow")?;
  let window_id_num = get_window_id_from_element(&window_handle)?;
  Some(WindowId::new(window_id_num.to_string()))
}

/// Get window ID number from an ElementHandle representing a window.
fn get_window_id_from_element(handle: &ElementHandle) -> Option<i64> {
  // Use safe string method, parse the ID
  let attr_name = CFString::from_static_str("_AXMacAppTitleBarTopLevelID");
  handle
    .get_raw_attr_internal(&attr_name)
    .and_then(|v| v.downcast_ref::<CFNumber>()?.as_i64())
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
pub fn create_observer_for_pid(pid: u32) -> AxioResult<ObserverHandle> {
  let observer = create_observer_raw(pid, Some(observer_callback))?;
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

    handle_notification(&element_id, &notification_name, element_ref);
  }));

  if result.is_err() {
    eprintln!("[axio] ⚠️  Accessibility notification handler panicked (possibly invalid element)");
  }
}

fn handle_notification(
  element_id: &ElementId,
  notification: &str,
  ax_element: CFRetained<AXUIElement>,
) {
  use crate::element_registry::ElementRegistry;

  let Some(notification_type) = AXNotification::from_str(notification) else {
    return;
  };

  match notification_type {
    AXNotification::ValueChanged => {
      let handle = ElementHandle::new(ax_element);
      let attrs = handle.get_attributes(None);
      if let Some(value) = attrs.value {
        if let Ok(mut element) = ElementRegistry::get(element_id) {
          element.value = Some(value);
          let _ = ElementRegistry::update(element_id, element.clone());
          emit(Event::ElementChanged {
            element: element.clone(),
          });
        }
      }
    }

    AXNotification::TitleChanged => {
      let handle = ElementHandle::new(ax_element);
      if let Some(label) = handle.get_string("AXTitle") {
        if !label.is_empty() {
          if let Ok(mut element) = ElementRegistry::get(element_id) {
            element.label = Some(label);
            let _ = ElementRegistry::update(element_id, element.clone());
            emit(Event::ElementChanged {
              element: element.clone(),
            });
          }
        }
      }
    }

    AXNotification::UIElementDestroyed => {
      if let Ok(element) = ElementRegistry::get(element_id) {
        ElementRegistry::remove_element(element_id);
        emit(Event::ElementRemoved {
          element: element.clone(),
        });
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

/// Build an AXElement from an ElementHandle and register it.
/// Uses batch attribute fetching for ~10x faster element creation.
/// All unsafe code is encapsulated in ElementHandle methods.
pub fn build_element_from_handle(
  handle: ElementHandle,
  window_id: &WindowId,
  pid: u32,
  parent_id: Option<&ElementId>,
) -> AXElement {
  use crate::element_registry::ElementRegistry;

  ensure_app_observer(pid);

  // Fetch all attributes in ONE IPC call - safe method!
  let attrs = handle.get_attributes(None);

  let platform_role = attrs.role.clone().unwrap_or_else(|| "Unknown".to_string());
  let role = map_platform_role(&platform_role);

  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.clone())
  } else {
    attrs.subrole
  };

  let element = AXElement {
    id: ElementId::new(Uuid::new_v4().to_string()),
    window_id: window_id.clone(),
    parent_id: parent_id.cloned(),
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

  ElementRegistry::register(element, handle, pid, &platform_role)
}

/// Discover and register children of an element.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  use crate::element_registry::ElementRegistry;

  let (parent_handle, window_id, pid) = ElementRegistry::with_stored(parent_id, |stored| {
    (
      stored.handle.clone(),
      stored.element.window_id.clone(),
      stored.pid,
    )
  })?;

  // Use safe ElementHandle method
  let child_handles = parent_handle.get_children();
  if child_handles.is_empty() {
    ElementRegistry::set_children(parent_id, vec![])?;
    return Ok(vec![]);
  }

  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for child_handle in child_handles.into_iter().take(max_children) {
    let child = build_element_from_handle(child_handle, &window_id, pid, Some(parent_id));
    child_ids.push(child.id.clone());
    children.push(child);
  }

  ElementRegistry::set_children(parent_id, child_ids.clone())?;

  for child in &children {
    emit(Event::ElementAdded {
      element: child.clone(),
    });
  }

  if let Ok(updated_parent) = ElementRegistry::get(parent_id) {
    emit(Event::ElementChanged {
      element: updated_parent.clone(),
    });
  }

  Ok(children)
}

/// Refresh an element's attributes from the platform.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
  use crate::element_registry::ElementRegistry;

  let (handle, window_id, parent_id, children, platform_role) =
    ElementRegistry::with_stored(element_id, |stored| {
      (
        stored.handle.clone(),
        stored.element.window_id.clone(),
        stored.element.parent_id.clone(),
        stored.element.children.clone(),
        stored.platform_role.clone(),
      )
    })?;

  // Use safe ElementHandle method for batch attribute fetch
  let attrs = handle.get_attributes(Some(&platform_role));

  let role = map_platform_role(&platform_role);
  let subrole = if matches!(role, AXRole::Unknown) {
    Some(platform_role.to_string())
  } else {
    attrs.subrole
  };

  let updated = AXElement {
    id: element_id.clone(),
    window_id,
    parent_id,
    children,
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

// ============================================================================
// Window Elements
// ============================================================================

/// Get all window ElementHandles for a given PID.
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<ElementHandle>> {
  let app_handle = ElementHandle::new(app_element(pid));
  let children = app_handle.get_children();

  let windows = children
    .into_iter()
    .filter(|child| child.get_string("AXRole").as_deref() == Some("AXWindow"))
    .collect();

  Ok(windows)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  let (window, handle) = crate::window_registry::get_with_handle(window_id)
    .ok_or_else(|| AxioError::WindowNotFound(window_id.clone()))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {} has no AX element", window_id)))?;

  // Clone handle for safe method use
  Ok(build_element_from_handle(
    window_handle.clone(),
    window_id,
    window.process_id.as_u32(),
    None,
  ))
}

/// Enable accessibility for an Electron app
pub fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  let raw_pid = pid.as_u32();
  let app_element = app_element(raw_pid);
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

  // Use safe ElementHandle method
  let app_handle = ElementHandle::new(app_element(pid));
  let element_handle = app_handle.element_at_position(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No element found at ({}, {}) in app {}", x, y, pid))
  })?;

  Ok(build_element_from_handle(
    element_handle,
    &window_id,
    pid,
    None,
  ))
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

  handle
    .set_value(text)
    .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {:?}", e)))
}

/// Perform a click (press) action on an element.
pub fn click_element(handle: &ElementHandle) -> AxioResult<()> {
  handle
    .perform_action("AXPress")
    .map_err(|e| AxioError::AccessibilityError(format!("AXPress failed: {:?}", e)))
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
