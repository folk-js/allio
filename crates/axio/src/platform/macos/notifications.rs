/*!
Notification subscription management for macOS accessibility.

Handles:
- Subscribing/unsubscribing to element notifications via WatchHandle RAII
- Subscribing to app-level notifications (focus, selection)
*/

#![allow(unsafe_code)]

use objc2_application_services::AXError;
use objc2_core_foundation::CFString;
use std::collections::HashSet;
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
/// Manages a set of notifications for an element. Unsubscribes on drop.
pub(crate) struct WatchHandleInner {
  observer: ObserverHandle,
  handle: ElementHandle,
  context: *mut ObserverContextHandle,
  notifications: HashSet<Notification>,
}

impl WatchHandleInner {
  /// Add notifications to the watch set.
  /// Already-subscribed notifications are skipped.
  pub(crate) fn add(&mut self, notifs: &[Notification]) -> usize {
    let mut added = 0;
    for notif in notifs {
      if self.notifications.contains(notif) {
        continue;
      }
      let notif_str = notification_to_macos(*notif);
  let notif_cfstring = CFString::from_str(notif_str);
  let result = unsafe {
        self.observer.inner().add_notification(
          self.handle.inner(),
      &notif_cfstring,
          self.context.cast::<c_void>(),
    )
  };
      if result == AXError::Success {
        self.notifications.insert(*notif);
        added += 1;
      }
    }
    added
  }

  /// Remove notifications from the watch set.
  pub(crate) fn remove(&mut self, notifs: &[Notification]) {
    for notif in notifs {
      if !self.notifications.contains(notif) {
        continue;
      }
      let notif_str = notification_to_macos(*notif);
      let notif_cfstring = CFString::from_str(notif_str);
      unsafe {
        let _ = self
          .observer
          .inner()
          .remove_notification(self.handle.inner(), &notif_cfstring);
      }
      self.notifications.remove(notif);
    }
  }
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

/// Create a watch handle for an element with initial notifications.
pub(super) fn create_watch(
  observer: &ObserverHandle,
  handle: &ElementHandle,
  element_id: ElementId,
  initial_notifications: &[Notification],
  axio: Axio,
) -> AxioResult<WatchHandleInner> {
  let context = register_observer_context(element_id, axio);

  let mut notifications = HashSet::new();
  for notif in initial_notifications {
    let notif_str = notification_to_macos(*notif);
    let notif_cfstring = CFString::from_str(notif_str);
    let result = unsafe {
      observer
        .inner()
        .add_notification(handle.inner(), &notif_cfstring, context.cast::<c_void>())
    };
      if result == AXError::Success {
      notifications.insert(*notif);
    }
  }

  if notifications.is_empty() && !initial_notifications.is_empty() {
    unregister_observer_context(context);
    return Err(AxioError::ObserverError(format!(
      "Failed to register notifications {:?} for element {}",
      initial_notifications, element_id
    )));
  }

  Ok(WatchHandleInner {
    observer: observer.clone(),
    handle: handle.clone(),
    context,
    notifications,
  })
}

/// Inner implementation of AppNotificationHandle for macOS.
/// Cleans up the observer context when dropped.
pub(crate) struct AppNotificationHandleInner {
  context: *mut ObserverContextHandle,
}

impl Drop for AppNotificationHandleInner {
  fn drop(&mut self) {
    unregister_observer_context(self.context);
  }
}

/// Subscribe to app-level notifications (focus, selection) on the application element.
/// Returns a handle that cleans up subscriptions when dropped.
pub(super) fn subscribe_app_notifications(
  pid: u32,
  observer: &ObserverHandle,
  axio: Axio,
) -> AxioResult<AppNotificationHandleInner> {
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

  Ok(AppNotificationHandleInner { context })
}
