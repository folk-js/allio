/*!
Event handlers for OS accessibility notifications.

These methods process notifications from the platform layer and update the
registry accordingly.
*/

use super::registry::CachedProcess;
use super::Axio;
use crate::accessibility::Notification;
use crate::platform::{CurrentPlatform, Platform, PlatformObserver};
use crate::types::{AxioResult, ElementId, ProcessId, WindowId};

impl Axio {
  /// Handle element destroyed notification.
  pub(crate) fn handle_element_destroyed(&self, element_id: ElementId) {
    self.write(|s| s.remove_element(element_id));
  }

  /// Handle focus changed notification.
  /// Only processes if element self-identifies as focused.
  pub(crate) fn handle_focus_changed(&self, pid: u32, element_id: ElementId) {
    use super::build_element;

    // Build element from cache
    let Some(element) = self.read(|r| build_element(r, element_id)) else {
      log::debug!("handle_focus_changed: element {element_id} not in cache");
      return;
    };

    // Only process focus for elements that self-identify as focused
    if element.focused != Some(true) {
      return;
    }

    let super::registry::FocusChange::Changed(previous_id) =
      self.write(|s| s.set_focused_element(ProcessId(pid), element.clone()))
    else {
      return;
    };

    // Auto-unwatch previous element
    if let Some(prev_id) = previous_id {
      let should_unwatch = self.read(|s| {
        s.element(prev_id)
          .is_some_and(|e| e.role.auto_watch_on_focus() || e.role.is_writable())
      });
      if should_unwatch {
        drop(self.unwatch(prev_id));
      }
    }

    // Auto-watch new element
    if element.role.auto_watch_on_focus() || element.role.is_writable() {
      drop(self.watch(element.id));
    }
  }

  /// Handle selection changed notification.
  pub(crate) fn handle_selection_changed(
    &self,
    pid: u32,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    self.write(|s| s.set_selection(ProcessId(pid), window_id, element_id, text, range));
  }

  /// Handle element changed notification.
  pub(crate) fn handle_element_changed(&self, element_id: ElementId, _notification: Notification) {
    if let Err(e) = self.refresh_element(element_id) {
      log::debug!("Failed to refresh element {element_id} on change: {e:?}");
    }
  }
}

impl Axio {
  /// Ensure process state exists for a PID. Idempotent.
  pub(crate) fn ensure_process(&self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    if self.read(|s| s.has_process(process_id)) {
      return Ok(process_id);
    }

    // Enable accessibility (needed for Chromium/Electron apps)
    CurrentPlatform::enable_accessibility_for_pid(pid);

    let callbacks = std::sync::Arc::new(self.clone());
    let observer = CurrentPlatform::create_observer(pid, callbacks.clone())?;
    let app_handle = CurrentPlatform::app_element(pid);

    let app_notifications = match observer.subscribe_app_notifications(pid, callbacks.clone()) {
      Ok(handle) => Some(handle),
      Err(e) => {
        log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
        None
      }
    };

    self.write(|s| {
      s.upsert_process(
        process_id,
        CachedProcess {
          observer,
          app_handle,
          focused_element: None,
          last_selection: None,
          _app_notifications: app_notifications,
        },
      )
    });

    Ok(process_id)
  }

  /// Ensure an element has a destruction watch set up. Idempotent.
  pub(crate) fn ensure_watched(&self, element_id: ElementId) {
    // Check if already watched
    let (needs_watch, pid, handle) = self.read(|r| {
      let Some(entry) = r.element(element_id) else {
        return (false, ProcessId(0), None);
      };
      let needs = entry.watch.is_none();
      (needs, entry.pid, Some(entry.handle.clone()))
    });

    if !needs_watch {
      return;
    }

    let Some(handle) = handle else {
      return;
    };

    // Get observer for this process
    let observer = self.read(|r| r.process(pid).map(|p| p.observer.clone()));

    let Some(observer) = observer else {
      return;
    };

    // Create watch (platform call, NO LOCK)
    let callbacks = std::sync::Arc::new(self.clone());
    match observer.create_watch(&handle, element_id, &[Notification::Destroyed], callbacks) {
      Ok(watch) => {
        self.write(|r| r.set_element_watch(element_id, watch));
      }
      Err(e) => {
        log::debug!("Failed to create destruction watch for element {element_id}: {e:?}");
      }
    }
  }
}
