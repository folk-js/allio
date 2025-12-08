/*!
Notification subscription management for macOS accessibility.

Handles:
- Subscribing/unsubscribing to element notifications
- Subscribing to app-level notifications (focus, selection)
- Destruction notification subscription
*/

use objc2_application_services::AXError;
use objc2_core_foundation::CFString;
use std::ffi::c_void;

use crate::accessibility::Notification;
use crate::platform::handles::{ElementHandle, ObserverHandle};
use crate::types::{AxioError, AxioResult, ElementId};

use super::mapping::notification_to_macos;
use super::observer::{
  register_observer_context, register_process_context, unregister_observer_context,
  ObserverContextHandle,
};
use super::util::app_element;

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

  Ok(context_handle)
}
