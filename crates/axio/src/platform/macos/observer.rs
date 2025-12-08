//! Observer management and unified callback for macOS accessibility.
//!
//! This module handles:
//! - Context registry for observer callbacks (element-level and process-level)
//! - Observer creation and run loop integration
//! - Unified callback dispatching

use objc2_application_services::{AXError, AXObserver, AXObserverCallback, AXUIElement};
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRetained, CFRunLoop, CFString};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::LazyLock;

use super::mapping::notification_from_macos;
use crate::accessibility::Notification;
use crate::platform::handles::{ElementHandle, ObserverHandle};
use crate::types::{AxioError, AxioResult, ElementId};

// =============================================================================
// Context Registry
// =============================================================================

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

// =============================================================================
// Observer Creation
// =============================================================================

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

/// Create an observer for a process and add it to the main run loop.
/// Uses the unified callback that handles both element-level and app-level notifications.
pub fn create_observer_for_pid(pid: u32) -> AxioResult<ObserverHandle> {
  let observer = create_observer_raw(pid, Some(unified_observer_callback))?;
  Ok(ObserverHandle::new(observer))
}

// =============================================================================
// Unified Callback
// =============================================================================

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
      log::warn!("Unknown macOS notification: {notification_str}");
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

// =============================================================================
// Notification Handlers
// =============================================================================

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
      // update_element emits ElementChanged if value actually changed
      if let Ok(mut element) = crate::registry::get_element(element_id) {
        element.value = attrs.value;
        let _ = crate::registry::update_element(element_id, element);
      }
    }

    Notification::TitleChanged => {
      let handle = ElementHandle::new(ax_element);
      // update_element emits ElementChanged if label actually changed
      if let Ok(mut element) = crate::registry::get_element(element_id) {
        element.label = handle.get_string("AXTitle");
        let _ = crate::registry::update_element(element_id, element);
      }
    }

    Notification::Destroyed => {
      crate::registry::remove_element(element_id);
    }

    Notification::ChildrenChanged => {
      // Re-fetch children - this registers new ones and updates the children list
      // The linking logic in register_element handles parent-child relationships
      let _ = super::element::children(element_id, 1000);
    }

    _ => {}
  }
}

/// Handle app/process-level notifications (focus change, selection change).
fn handle_process_notification(pid: u32, notif: Notification, ax_element: CFRetained<AXUIElement>) {
  match notif {
    Notification::FocusChanged => {
      super::focus::handle_app_focus_changed(pid, ax_element);
    }
    Notification::SelectionChanged => {
      super::focus::handle_app_selection_changed(pid, ax_element);
    }
    _ => {}
  }
}
