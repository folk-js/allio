/*!
Mutation methods for Axio.

- `set_*` = write value to OS
- `perform_*` = execute action on OS
- `sync_*` = bulk updates from polling
- `on_*` = notification handlers
*/

use super::registry::ProcessEntry;
use super::Axio;
use crate::accessibility::{Action, Notification};
use crate::platform::{CurrentPlatform, Platform, PlatformHandle, PlatformObserver};
use crate::types::{AxioError, AxioResult, ElementId, ProcessId, Window, WindowId};
use std::collections::HashSet;

impl Axio {
  /// Set a typed value on an element.
  pub fn set_value(
    &self,
    element_id: ElementId,
    value: &crate::accessibility::Value,
  ) -> AxioResult<()> {
    // Step 1: Extract what we need (quick read)
    let (handle, role) = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.data.role))
    })?;

    // Step 2: Validate role is writable (no lock)
    if !role.is_writable() {
      return Err(AxioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    // Step 3: Validate value type matches role's expected type
    let expected = role.value_type();
    let got = value.value_type();
    if expected != got {
      return Err(AxioError::TypeMismatch { expected, got });
    }

    // Step 4: Platform call (NO LOCK)
    handle.set_value(value)
  }

  /// Perform an action on an element.
  pub fn perform_action(&self, element_id: ElementId, action: Action) -> AxioResult<()> {
    let handle = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok(e.handle.clone())
    })?;

    handle.perform_action(action)
  }
}

impl Axio {
  /// Sync windows from polling. Handles add/update/remove.
  /// `skip_removal=true` during space transitions where window visibility is unreliable.
  /// TODO: remove `skip_removal` and just pause sync in this instance ^
  pub(crate) fn sync_windows(&self, new_windows: Vec<Window>, skip_removal: bool) {
    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    let windows_needing_handle: HashSet<WindowId> = self.read(|s| {
      new_ids
        .iter()
        .filter(|id| {
          // Fetch handle if: window is new OR existing window has no handle
          s.window(**id).and_then(|w| w.handle.as_ref()).is_none()
        })
        .copied()
        .collect()
    });

    let windows_with_handles: Vec<_> = new_windows
      .into_iter()
      .map(|w| {
        let handle = if windows_needing_handle.contains(&w.id) {
          CurrentPlatform::fetch_window_handle(&w)
        } else {
          None // Already have a cached handle
        };
        (w, handle)
      })
      .collect();

    let new_process_pids = self.write(|s| {
      // Remove windows no longer present
      if !skip_removal {
        let to_remove: Vec<WindowId> = s.window_ids().filter(|id| !new_ids.contains(id)).collect();
        for window_id in to_remove {
          s.remove_window(window_id);
        }
      }

      // Add/update windows
      let mut fresh_pids = Vec::new();
      for (window_info, handle) in windows_with_handles {
        let window_id = window_info.id;
        let process_id = window_info.process_id;
        let is_new = s.window(window_id).is_none();

        if is_new {
          s.upsert_window(window_id, process_id, window_info.clone(), handle.clone());
          fresh_pids.push(process_id);
        } else {
          // Already existed - update
          s.update_window(window_id, window_info);
          // Update handle if we fetched one (retrying for windows that had None)
          if let Some(h) = handle {
            s.set_window_handle(window_id, h);
          }
        }
      }
      fresh_pids
    });

    for process_id in new_process_pids {
      if let Err(e) = self.ensure_process(process_id.0) {
        log::warn!("Failed to create process for window: {e:?}");
      }
    }
  }

  /// Sync focused window from polling.
  pub(crate) fn sync_focused_window(&self, window_id: Option<WindowId>) {
    self.write(|s| s.set_focused_window(window_id));
  }

  /// Sync mouse position from polling.
  pub(crate) fn sync_mouse(&self, pos: crate::types::Point) {
    self.write(|s| s.set_mouse_position(pos));
  }
}

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
          .is_some_and(|e| e.data.role.auto_watch_on_focus() || e.data.role.is_writable())
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
        ProcessEntry {
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
}

impl Axio {
  /// Ensure an element has a destruction watch set up. Idempotent.
  pub(crate) fn ensure_watched(&self, element_id: ElementId) {
    // Check if already watched
    let (needs_watch, pid, handle) = self.read(|r| {
      let Some(entry) = r.element(element_id) else {
        return (false, ProcessId(0), None);
      };
      let needs = entry.watch.is_none();
      (needs, entry.data.pid, Some(entry.handle.clone()))
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
