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
use crate::platform::{self, CurrentPlatform, Platform, PlatformHandle, PlatformObserver};
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
    // Extract what we need, then release lock BEFORE platform call
    let (handle, role) = {
      let state = self.state.read();
      let elem = state
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      (elem.handle.clone(), elem.element.role)
    };

    // Validation in core, not platform
    if !role.is_writable() {
      return Err(AxioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    // Platform call with NO lock held
    handle.set_value(value)
  }

  /// Perform click action on an element.
  pub fn perform_click(&self, element_id: ElementId) -> AxioResult<()> {
    // Extract handle, then release lock BEFORE platform call
    let handle = {
      let state = self.state.read();
      let elem = state
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      elem.handle.clone()
    };

    // Platform call with NO lock held
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

    // Prepare window handles OUTSIDE the lock (platform calls)
    let windows_with_handles: Vec<_> = new_windows
      .into_iter()
      .map(|w| {
        let handle = platform::get_window_handle(&w);
        (w, handle)
      })
      .collect();

    // Single write lock for all state mutations
    let new_process_pids = {
      let mut state = self.state.write();

      // Remove windows no longer present
      let to_remove: Vec<WindowId> = state
        .get_all_window_ids()
        .filter(|id| !new_ids.contains(id))
        .collect();

      for window_id in to_remove {
        state.remove_window(window_id);
      }

      // Process new/existing windows
      let mut new_pids = Vec::new();

      for (window_info, handle) in windows_with_handles {
        let window_id = window_info.id;
        let process_id = window_info.process_id;

        let inserted =
          state.get_or_insert_window(window_id, process_id, window_info.clone(), handle.clone());

        if inserted {
          new_pids.push(process_id);
        } else {
          // Update existing window
          state.update_window(window_id, window_info);

          // Set handle if we have one
          if let Some(h) = handle {
            state.set_window_handle(window_id, h);
          }
        }
      }

      new_pids
    }; // Lock released here

    // Process creation happens OUTSIDE the main lock
    for process_id in new_process_pids {
      if let Err(e) = self.get_or_create_process(process_id.0) {
        log::warn!("Failed to create process for window: {e:?}");
      }
    }
  }

  /// Sync focused window from polling.
  pub(crate) fn sync_focused_window(&self, window_id: Option<WindowId>) {
    self.state.write().set_focused_window(window_id);
  }

  /// Sync mouse position from polling.
  pub(crate) fn sync_mouse(&self, pos: crate::types::Point) {
    self.state.write().set_mouse_position(pos);
  }
}

// ============================================================================
// Notification Handlers (on_*)
// ============================================================================

impl Axio {
  /// Handle element destroyed notification.
  pub(crate) fn on_element_destroyed(&self, element_id: ElementId) {
    self.state.write().remove_element(element_id);
  }

  /// Handle focus changed notification.
  pub(crate) fn on_focus_changed(&self, pid: u32, element: AXElement) {
    let (changed, previous_id) = self
      .state
      .write()
      .set_focused_element(ProcessId(pid), element.clone());

    if !changed {
      return;
    }

    // Auto-unwatch previous element
    if let Some(prev_id) = previous_id {
      if let Some(prev_elem) = self.state.read().get_element(prev_id).cloned() {
        if prev_elem.role.auto_watch_on_focus() || prev_elem.role.is_writable() {
          drop(self.unwatch(prev_id));
        }
      }
    }

    // Auto-watch new element
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
    self
      .state
      .write()
      .set_selection(ProcessId(pid), window_id, element_id, text, range);
  }

  /// Handle element changed notification (value, children, etc).
  #[allow(dead_code)] // Called from observer, may be used by other notification handlers
  pub(crate) fn on_element_changed(&self, element_id: ElementId, _notification: Notification) {
    // Refresh the element from platform
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

    // Fast path: already exists
    if self.state.read().has_process(process_id) {
      return Ok(process_id);
    }

    // Slow path: create observer and app handle
    let observer = platform::create_observer(pid, self.clone())?;
    let app_handle = CurrentPlatform::app_element(pid);

    // Subscribe to app-level notifications
    if let Err(e) = observer.subscribe_app_notifications(pid, self.clone()) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
    }

    self.state.write().insert_process(
      process_id,
      ProcessState {
        observer,
        app_handle,
        focused_element: None,
        last_selection: None,
      },
    );

    Ok(process_id)
  }
}

// ============================================================================
// Element Registration (used by element_ops)
// ============================================================================

impl Axio {
  /// Register an element (get or insert by hash).
  ///
  /// Returns the element (existing if hash matched, new if inserted).
  /// Sets up destruction watch after insertion.
  pub(crate) fn register_element(&self, elem_state: super::ElementState) -> Option<AXElement> {
    let pid = elem_state.element.pid;
    let handle = elem_state.handle.clone();

    // Get or insert (handles deduplication by hash internally)
    let element_id = self.state.write().get_or_insert_element(elem_state);

    // If this was a new element (not existing), set up destruction watch
    // Check if watch is already set
    let needs_watch = self
      .state
      .read()
      .get_element_state(element_id)
      .map(|e| e.watch.is_none())
      .unwrap_or(false);

    if needs_watch {
      self.setup_destruction_watch(element_id, pid, &handle);
    }

    self.state.read().get_element(element_id).cloned()
  }

  /// Set up destruction watch for an element.
  fn setup_destruction_watch(
    &self,
    element_id: ElementId,
    pid: ProcessId,
    handle: &crate::platform::Handle,
  ) {
    let observer = {
      let state = self.state.read();
      state.get_process(pid).map(|p| p.observer.clone())
    };

    let Some(observer) = observer else {
      return;
    };

    match observer.create_watch(handle, element_id, &[Notification::Destroyed], self.clone()) {
      Ok(watch) => {
        self.state.write().set_element_watch(element_id, watch);
      }
      Err(e) => {
        log::debug!("Failed to create destruction watch for element {element_id}: {e:?}");
      }
    }
  }

  /// Update element data (used by fetch_element).
  pub(crate) fn update_element(&self, element_id: ElementId, data: AXElement) -> AxioResult<()> {
    let updated = self.state.write().update_element(element_id, data);
    if !updated {
      // Element might not exist
      if self.state.read().get_element(element_id).is_none() {
        return Err(AxioError::ElementNotFound(element_id));
      }
    }
    Ok(())
  }

  /// Set children for an element (used by fetch_children).
  pub(crate) fn set_element_children(
    &self,
    element_id: ElementId,
    children: Vec<ElementId>,
  ) -> AxioResult<()> {
    self
      .state
      .write()
      .set_element_children(element_id, children);
    Ok(())
  }
}
