/*!
Notification subscription management for macOS accessibility.

Handles:
- Subscribing/unsubscribing to element notifications via WatchHandle RAII
- Subscribing to app-level notifications (focus, selection)
*/

#![allow(unsafe_code)]

use objc2_application_services::AXError;
use objc2_core_foundation::CFString;
use std::ffi::c_void;

use super::handles::{ElementHandle, ObserverHandle};
use super::mapping::notification_to_macos;
use super::observer::{
  register_observer_context, register_process_context, unregister_observer_context,
  ObserverContextHandle,
};
use super::util::app_element;
use crate::accessibility::Notification;
use crate::core::Axio;
use crate::types::{AxioError, AxioResult, ElementId};

/// Inner implementation of WatchHandle for macOS.
/// Handles unsubscription automatically when dropped.
pub(crate) struct WatchHandleInner {
  observer: ObserverHandle,
  handle: ElementHandle,
  context: *mut ObserverContextHandle,
  notifications: Vec<Notification>,
}

impl Drop for WatchHandleInner {
  fn drop(&mut self) {
    // Unsubscribe from all notifications
    for notification in &self.notifications {
      let notif_str = notification_to_macos(*notification);
      let notif_cfstring = CFString::from_str(notif_str);
      unsafe {
        let _ = self
          .observer
          .inner()
          .remove_notification(self.handle.inner(), &notif_cfstring);
      }
    }
    // Clean up the context
    unregister_observer_context(self.context);
  }
}

/// Watch for element destruction.
/// Returns a WatchHandleInner that unsubscribes when dropped.
pub(super) fn watch_destruction(
  observer: &ObserverHandle,
  handle: &ElementHandle,
  element_id: ElementId,
  axio: Axio,
) -> AxioResult<WatchHandleInner> {
  let context = register_observer_context(element_id, axio);
  let notification = Notification::Destroyed;

  let notif_str = notification_to_macos(notification);
  let notif_cfstring = CFString::from_str(notif_str);
  let result = unsafe {
    observer
      .inner()
      .add_notification(handle.inner(), &notif_cfstring, context.cast::<c_void>())
  };

  if result != AXError::Success {
    unregister_observer_context(context);
    return Err(AxioError::ObserverError(format!(
      "Failed to register destruction notification for element {element_id}: {result:?}"
    )));
  }

  Ok(WatchHandleInner {
    observer: observer.clone(),
    handle: handle.clone(),
    context,
    notifications: vec![notification],
  })
}

/// Watch an element for notifications.
/// Returns a WatchHandleInner that unsubscribes when dropped.
pub(super) fn watch_element(
  observer: &ObserverHandle,
  handle: &ElementHandle,
  element_id: ElementId,
  notifications: &[Notification],
  axio: Axio,
) -> AxioResult<WatchHandleInner> {
  if notifications.is_empty() {
    return Err(AxioError::NotSupported(
      "No notifications to subscribe".into(),
    ));
  }

  let context = register_observer_context(element_id, axio);

  let mut registered = Vec::new();
  for notification in notifications {
    let notif_str = notification_to_macos(*notification);
    let notif_cfstring = CFString::from_str(notif_str);
    unsafe {
      let result = observer.inner().add_notification(
        handle.inner(),
        &notif_cfstring,
        context.cast::<c_void>(),
      );
      if result == AXError::Success {
        registered.push(*notification);
      }
    }
  }

  if registered.is_empty() {
    unregister_observer_context(context);
    return Err(AxioError::ObserverError(
      "Failed to register any notifications".into(),
    ));
  }

  Ok(WatchHandleInner {
    observer: observer.clone(),
    handle: handle.clone(),
    context,
    notifications: registered,
  })
}

/// Subscribe to app-level notifications (focus, selection) on the application element.
pub(super) fn subscribe_app_notifications(
  pid: u32,
  observer: &ObserverHandle,
  axio: Axio,
) -> AxioResult<()> {
  let app_el = app_element(pid);
  let context = register_process_context(pid, axio);

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
          .add_notification(&app_el, &notif_cfstring, context.cast::<c_void>());
      if result == AXError::Success {
        registered += 1;
      }
    }
  }

  if registered == 0 {
    unregister_observer_context(context);
    return Err(AxioError::ObserverError(format!(
      "Failed to subscribe to app notifications for PID {pid}"
    )));
  }

  // Note: context is intentionally leaked for app-level notifications
  // since they live for the lifetime of the process observer
  Ok(())
}
