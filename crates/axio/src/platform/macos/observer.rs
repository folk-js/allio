/*!
Observer management and unified callback for macOS accessibility.

Handles:
- Context registry for observer callbacks (element-level and process-level)
- Observer creation and run loop integration
- Unified callback dispatching

# Context Design

Observer contexts store a `PlatformCallbacks` implementation alongside the element/process ID.
This allows callbacks to access core state without globals.

The context map (`OBSERVER_CONTEXTS`) exists because:
1. **C callback constraint**: macOS `AXObserver` callbacks receive a raw pointer (`refcon`)
   that we need to map back to our typed context. We can't pass Rust closures to C code.
2. **Lifetime management**: Context handles are passed to macOS which may hold them
   indefinitely. Using stable u64 IDs with a global map avoids lifetime issues.
*/

#![allow(unsafe_code)]
#![allow(clippy::expect_used)] // NonNull::new on stack pointers - never null

use objc2_application_services::{AXError, AXObserver, AXObserverCallback, AXUIElement};
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRetained, CFRunLoop, CFString};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, LazyLock};

use super::handles::{ElementHandle, ObserverHandle};
use super::mapping::notification_from_macos;
use crate::accessibility::Notification;
use crate::platform::{ElementEvent, PlatformCallbacks};
use crate::types::{AxioError, AxioResult, ElementId};

/// Next available context ID.
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Type-erased callbacks for storing in the global context map.
/// We use a boxed trait object to avoid generic type parameters in the global map.
trait CallbacksErased: Send + Sync {
  fn on_element_event(&self, event: ElementEvent<ElementHandle>);
}

/// Wrapper that erases the generic type parameter.
struct CallbacksWrapper<C: PlatformCallbacks<Handle = ElementHandle>>(Arc<C>);

impl<C: PlatformCallbacks<Handle = ElementHandle>> CallbacksErased for CallbacksWrapper<C> {
  fn on_element_event(&self, event: ElementEvent<ElementHandle>) {
    self.0.on_element_event(event);
  }
}

/// Observer context - stores callbacks alongside element/process ID.
struct ObserverContext {
  callbacks: Arc<dyn CallbacksErased>,
  target: ObserverTarget,
}

/// What the observer is watching.
#[derive(Clone)]
enum ObserverTarget {
  /// Element-level notification (context identifies which element)
  Element(ElementId),
  /// Process-level notification (context identifies which app)
  Process(u32),
}

/// Opaque handle passed to macOS callbacks.
/// Contains only an ID that maps to the actual context in `OBSERVER_CONTEXTS`.
#[repr(C)]
pub(crate) struct ObserverContextHandle {
  context_id: u64,
}

/// Global registry mapping context IDs to observer contexts.
static OBSERVER_CONTEXTS: LazyLock<Mutex<HashMap<u64, ObserverContext>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register an element context and get a raw pointer handle.
pub(super) fn register_observer_context<C: PlatformCallbacks<Handle = ElementHandle>>(
  element_id: ElementId,
  callbacks: Arc<C>,
) -> *mut ObserverContextHandle {
  register_context(ObserverTarget::Element(element_id), callbacks)
}

/// Register a process context and get a raw pointer handle.
pub(super) fn register_process_context<C: PlatformCallbacks<Handle = ElementHandle>>(
  pid: u32,
  callbacks: Arc<C>,
) -> *mut ObserverContextHandle {
  register_context(ObserverTarget::Process(pid), callbacks)
}

fn register_context<C: PlatformCallbacks<Handle = ElementHandle>>(
  target: ObserverTarget,
  callbacks: Arc<C>,
) -> *mut ObserverContextHandle {
  let context_id = NEXT_CONTEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
  let wrapped = Arc::new(CallbacksWrapper(callbacks)) as Arc<dyn CallbacksErased>;
  OBSERVER_CONTEXTS
    .lock()
    .insert(context_id, ObserverContext { callbacks: wrapped, target });
  Box::into_raw(Box::new(ObserverContextHandle { context_id }))
}

/// Unregister and free a context handle.
pub(super) fn unregister_observer_context(handle_ptr: *mut ObserverContextHandle) {
  if handle_ptr.is_null() {
    return;
  }
  unsafe {
    let handle = Box::from_raw(handle_ptr);
    OBSERVER_CONTEXTS.lock().remove(&handle.context_id);
  }
}

/// Lookup context info from the handle (lock is released after this returns).
/// Returns (callbacks_arc, target) if found.
fn lookup_context(
  handle_ptr: *const ObserverContextHandle,
) -> Option<(Arc<dyn CallbacksErased>, ObserverTarget)> {
  if handle_ptr.is_null() {
    return None;
  }
    let guard = OBSERVER_CONTEXTS.lock();
  let handle = unsafe { &*handle_ptr };
  guard.get(&handle.context_id).map(|ctx| {
    // Clone what we need so we can release the lock before callbacks
    (ctx.callbacks.clone(), ctx.target.clone())
  })
}

/// Create an `AXObserver` and add it to the main run loop.
fn create_observer_raw(
  pid: u32,
  callback: AXObserverCallback,
) -> AxioResult<CFRetained<AXObserver>> {
  let observer = unsafe {
    let mut observer_ptr: *mut AXObserver = std::ptr::null_mut();
    #[allow(clippy::cast_possible_wrap)] // PIDs are always positive and < i32::MAX
    let result = AXObserver::create(
      pid as i32,
      callback,
      NonNull::new(&raw mut observer_ptr).expect("stack pointer is never null"),
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
pub(crate) fn create_observer_for_pid<C: PlatformCallbacks<Handle = ElementHandle>>(
  pid: u32,
  _callbacks: Arc<C>,
) -> AxioResult<ObserverHandle> {
  let observer = create_observer_raw(pid, Some(unified_observer_callback))?;
  Ok(ObserverHandle::new(observer))
}

/// Observer callback - handles both element-level and app-level notifications.
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

    // Lookup context (releases lock before we invoke callbacks)
    let Some((callbacks, target)) = lookup_context(refcon as *const ObserverContextHandle) else {
      return;
    };

    // Now invoke callbacks without holding the OBSERVER_CONTEXTS lock
    match target {
      ObserverTarget::Element(element_id) => {
        handle_element_notification(callbacks.as_ref(), element_id, notif);
      }
      ObserverTarget::Process(_pid) => {
        handle_process_notification(callbacks.as_ref(), notif, element_ref.clone());
      }
    }
  }));

  if result.is_err() {
    log::warn!("Accessibility notification handler panicked (possibly invalid element)");
  }
}

/// Handle element-level notifications.
fn handle_element_notification(
  callbacks: &dyn CallbacksErased,
  element_id: ElementId,
  notif: Notification,
) {
  let event = match notif {
    Notification::ValueChanged | Notification::TitleChanged => {
      ElementEvent::Changed(element_id, notif)
    }

    Notification::Destroyed => ElementEvent::Destroyed(element_id),

    Notification::ChildrenChanged => ElementEvent::ChildrenChanged(element_id),

    // Element-level handler doesn't process these app-level notifications
    Notification::FocusChanged | Notification::SelectionChanged | Notification::BoundsChanged => {
      return;
    }
  };

  callbacks.on_element_event(event);
}

/// Handle app/process-level notifications (focus change, selection change).
fn handle_process_notification(
  callbacks: &dyn CallbacksErased,
  notif: Notification,
  ax_element: CFRetained<AXUIElement>,
) {
  let event = match notif {
    Notification::FocusChanged => {
      let handle = ElementHandle::new(ax_element);
      ElementEvent::FocusChanged(handle)
    }
    Notification::SelectionChanged => {
      let handle = ElementHandle::new(ax_element.clone());
      let (text, range) = super::focus::get_selection_from_handle(&handle).unwrap_or_default();
      ElementEvent::SelectionChanged { handle, text, range }
    }
    // Process-level handler doesn't process these element-level notifications
    Notification::Destroyed
    | Notification::ValueChanged
    | Notification::TitleChanged
    | Notification::BoundsChanged
    | Notification::ChildrenChanged => {
      return;
    }
  };

  callbacks.on_element_event(event);
}
