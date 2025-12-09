/*!
Mutation methods for Axio.

- `set_*` = write value to OS
- `perform_*` = execute action on OS
- `sync_*` = bulk updates from polling
- `on_*` = notification handlers
*/

use super::state::ProcessState;
use super::Axio;
use crate::accessibility::Notification;
use crate::platform::{CurrentPlatform, Platform, PlatformHandle, PlatformObserver};
use crate::types::{AXElement, AXWindow, AxioError, AxioResult, ElementId, ProcessId, WindowId};
use std::collections::HashSet;

// ============================================================================
// Public Mutations (set_*, perform_*)
// ============================================================================

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
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.element.role))
    })?;

    // Step 2: Validate (no lock)
    if !role.is_writable() {
      return Err(AxioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    // Step 3: Platform call (NO LOCK)
    handle.set_value(value)
  }

  /// Perform click action on an element.
  pub fn perform_click(&self, element_id: ElementId) -> AxioResult<()> {
    // Step 1: Extract handle (quick read)
    let handle = self.read(|s| {
      let e = s
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok(e.handle.clone())
    })?;

    // Step 2: Platform call (NO LOCK)
    handle.perform_action("AXPress")
  }
}

// ============================================================================
// Sync Operations (from polling)
// ============================================================================

impl Axio {
  /// Sync windows from polling. Handles add/update/remove.
  pub(crate) fn sync_windows(&self, new_windows: Vec<AXWindow>) {
    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    // Step 1: Get existing windows that need handle fetch (new or missing handle)
    let windows_needing_handle: HashSet<WindowId> = self.read(|s| {
      new_ids
        .iter()
        .filter(|id| {
          // Fetch handle if: window is new OR existing window has no handle
          s.get_window_handle(**id).is_none()
        })
        .copied()
        .collect()
    });

    // Step 2: Platform calls OUTSIDE lock - only fetch handles when needed
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

    // Step 3: Single write for all state mutations
    let new_process_pids = self.write(|s| {
      // Remove windows no longer present
      let to_remove: Vec<WindowId> = s
        .get_all_window_ids()
        .filter(|id| !new_ids.contains(id))
        .collect();
      for window_id in to_remove {
        s.remove_window(window_id);
      }

      // Add/update windows
      let mut new_pids = Vec::new();
      for (window_info, handle) in windows_with_handles {
        let window_id = window_info.id;
        let process_id = window_info.process_id;

        let inserted =
          s.get_or_insert_window(window_id, process_id, window_info.clone(), handle.clone());
        if inserted {
          new_pids.push(process_id);
        } else {
          s.update_window(window_id, window_info);
          // Update handle if we fetched one (retrying for windows that had None)
          if let Some(h) = handle {
            s.set_window_handle(window_id, h);
          }
        }
      }
      new_pids
    });

    // Step 4: Process creation OUTSIDE lock (has platform calls)
    for process_id in new_process_pids {
      if let Err(e) = self.get_or_create_process(process_id.0) {
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

  /// Fast path: update just a single window's bounds.
  /// Used by event-driven mode when we know only one window moved.
  /// Returns true if window exists and was updated (or bounds unchanged).
  pub(crate) fn update_window_bounds(&self, window_id: WindowId, bounds: crate::types::Bounds) -> bool {
    self.write(|s| {
      // Check if window exists first
      if !s.has_window(window_id) {
        return false;
      }
      s.update_window_bounds(window_id, bounds);
      true
    })
  }
}

// ============================================================================
// Notification Handlers (on_*)
// ============================================================================

impl Axio {
  /// Handle element destroyed notification.
  pub(crate) fn on_element_destroyed(&self, element_id: ElementId) {
    self.write(|s| s.remove_element(element_id));
  }

  /// Handle focus changed notification.
  pub(crate) fn on_focus_changed(&self, pid: u32, element: AXElement) {
    // Step 1: Update focus (quick write)
    let (changed, previous_id) =
      self.write(|s| s.set_focused_element(ProcessId(pid), element.clone()));

    if !changed {
      return;
    }

    // Step 2: Auto-unwatch previous (separate read, then unwatch which may have platform calls)
    if let Some(prev_id) = previous_id {
      let should_unwatch = self.read(|s| {
        s.get_element(prev_id)
          .map(|e| e.role.auto_watch_on_focus() || e.role.is_writable())
          .unwrap_or(false)
      });
      if should_unwatch {
        drop(self.unwatch(prev_id));
      }
    }

    // Step 3: Auto-watch new element (may have platform calls)
    if element.role.auto_watch_on_focus() || element.role.is_writable() {
      drop(self.watch(element.id));
    }
  }

  /// Handle selection changed notification.
  pub(crate) fn on_selection_changed(
    &self,
    pid: u32,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    self.write(|s| s.set_selection(ProcessId(pid), window_id, element_id, text, range));
  }

  /// Handle element changed notification (value, children, etc).
  #[allow(dead_code)]
  pub(crate) fn on_element_changed(&self, element_id: ElementId, _notification: Notification) {
    // Platform call - already outside any lock
    if let Err(e) = crate::platform::element_ops::fetch_element(self, element_id) {
      log::debug!("Failed to refresh element {element_id} on change: {e:?}");
    }
  }
}

// ============================================================================
// Process Management
// ============================================================================

impl Axio {
  /// Get or create process state for a PID.
  pub(crate) fn get_or_create_process(&self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    // Fast path: check if exists (quick read)
    if self.read(|s| s.has_process(process_id)) {
      return Ok(process_id);
    }

    // Slow path: platform calls OUTSIDE lock

    // Enable accessibility for this process (needed for Chromium/Electron apps).
    // This is idempotent and only called once per process (on first registration).
    CurrentPlatform::enable_accessibility_for_pid(pid);

    let observer = CurrentPlatform::create_observer(pid, self.clone())?;
    let app_handle = CurrentPlatform::app_element(pid);

    if let Err(e) = observer.subscribe_app_notifications(pid, self.clone()) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
    }

    // Quick write to insert
    self.write(|s| {
      s.insert_process(
        process_id,
        ProcessState {
          observer,
          app_handle,
          focused_element: None,
          last_selection: None,
        },
      )
    });

    Ok(process_id)
  }
}

// ============================================================================
// Element Registration (used by element_ops)
// ============================================================================

impl Axio {
  /// Register an element (get or insert by hash).
  /// Sets up destruction watch after insertion.
  pub(crate) fn register_element(&self, elem_state: super::ElementState) -> Option<AXElement> {
    let pid = elem_state.element.pid;
    let handle = elem_state.handle.clone();

    // Step 1: Insert element (quick write)
    let element_id = self.write(|s| s.get_or_insert_element(elem_state));

    // Step 2: Check if watch needed (quick read)
    let needs_watch = self.read(|s| {
      s.get_element_state(element_id)
        .map(|e| e.watch.is_none())
        .unwrap_or(false)
    });

    // Step 3: Setup watch (has platform calls)
    if needs_watch {
      self.setup_destruction_watch(element_id, pid, &handle);
    }

    // Step 4: Return element (quick read)
    self.read(|s| s.get_element(element_id).cloned())
  }

  /// Set up destruction watch for an element.
  fn setup_destruction_watch(
    &self,
    element_id: ElementId,
    pid: ProcessId,
    handle: &crate::platform::Handle,
  ) {
    // Step 1: Get observer (quick read)
    let observer = self.read(|s| s.get_process(pid).map(|p| p.observer.clone()));

    let Some(observer) = observer else {
      return;
    };

    // Step 2: Create watch (platform call, NO LOCK)
    match observer.create_watch(handle, element_id, &[Notification::Destroyed], self.clone()) {
      Ok(watch) => {
        // Step 3: Store watch (quick write)
        self.write(|s| s.set_element_watch(element_id, watch));
      }
      Err(e) => {
        log::debug!("Failed to create destruction watch for element {element_id}: {e:?}");
      }
    }
  }

  /// Update element data (used by fetch_element).
  pub(crate) fn update_element(&self, element_id: ElementId, data: AXElement) -> AxioResult<()> {
    let updated = self.write(|s| s.update_element(element_id, data));
    if !updated && self.read(|s| s.get_element(element_id).is_none()) {
      return Err(AxioError::ElementNotFound(element_id));
    }
    Ok(())
  }

  /// Set children for an element (used by fetch_children).
  pub(crate) fn set_element_children(
    &self,
    element_id: ElementId,
    children: Vec<ElementId>,
  ) -> AxioResult<()> {
    self.write(|s| s.set_element_children(element_id, children));
    Ok(())
  }
}
