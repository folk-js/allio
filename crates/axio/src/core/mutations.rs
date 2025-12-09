/*!
Mutation methods for Axio.

- `set_*` = value setting
- `perform_*` = actions
- `pub(crate)` methods = internal state updates from polling/notifications
*/

use super::state::{ElementState, ProcessState, WindowState};
use super::Axio;
use crate::platform::{self, CurrentPlatform, Platform, PlatformHandle, PlatformObserver};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, Event, ProcessId, TextSelection, WindowId,
};
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
    // Validation happens HERE in core, not in platform code
    let role = self.with_element(element_id, |e| e.element.role)?;
    if !role.is_writable() {
      return Err(AxioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    self.with_element(element_id, |e| e.handle.set_value(value))?
  }

  /// Perform click action on an element.
  pub fn perform_click(&self, element_id: ElementId) -> AxioResult<()> {
    self.with_element(element_id, |e| e.handle.perform_action("AXPress"))?
  }
}

// ============================================================================
// Internal Mutations (pub(crate))
// ============================================================================

impl Axio {
  /// Get or create process state for a PID.
  /// Creates the `AXObserver` and app handle if this is a new process.
  pub(crate) fn get_or_create_process(&self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    // Fast path: check if already exists
    if self.state.read().processes.contains_key(&process_id) {
      return Ok(process_id);
    }

    // Slow path: create observer and app handle
    let observer = platform::create_observer(pid, self.clone())?;
    let app_handle = CurrentPlatform::app_element(pid);

    // Subscribe to app-level notifications (focus, selection)
    if let Err(e) = observer.subscribe_app_notifications(pid, self.clone()) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
    }

    self.state.write().processes.insert(
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

  /// Update windows from polling. Returns PIDs of newly added windows.
  pub(crate) fn update_windows(&self, new_windows: Vec<AXWindow>) -> Vec<ProcessId> {
    let mut events = Vec::new();
    let mut added_window_ids = Vec::new();
    let mut new_process_ids = Vec::new();
    let mut changed_window_ids = Vec::new();

    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    {
      let mut state = self.state.write();

      // Find removed windows
      let removed: Vec<WindowId> = state
        .windows
        .keys()
        .filter(|id| !new_ids.contains(id))
        .copied()
        .collect();

      for window_id in removed {
        events.extend(Self::remove_window_internal(&mut state, window_id));
      }

      // Process new/existing windows
      for window_info in new_windows {
        let window_id = window_info.id;
        let process_id = window_info.process_id;

        if let Some(existing) = state.windows.get_mut(&window_id) {
          if existing.info != window_info {
            changed_window_ids.push(window_id);
          }
          existing.info = window_info;

          if existing.handle.is_none() {
            existing.handle = platform::get_window_handle(&existing.info);
          }
        } else {
          // New window
          let handle = platform::get_window_handle(&window_info);

          state.windows.insert(
            window_id,
            WindowState {
              process_id,
              info: window_info.clone(),
              handle,
            },
          );
          added_window_ids.push(window_id);
          new_process_ids.push(process_id);
        }
      }

      // Update depth order
      let mut windows: Vec<_> = state.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      state.depth_order = windows.into_iter().map(|w| w.id).collect();

      // Generate events for added windows
      for window_id in &added_window_ids {
        if let Some(window) = state.windows.get(window_id) {
          events.push(Event::WindowAdded {
            window: window.info.clone(),
          });
        }
      }

      // Generate events for changed windows
      for window_id in changed_window_ids {
        if let Some(window) = state.windows.get(&window_id) {
          events.push(Event::WindowChanged {
            window: window.info.clone(),
          });
        }
      }
    }

    // Ensure processes exist for new windows (outside the state lock)
    for process_id in &new_process_ids {
      if let Err(e) = self.get_or_create_process(process_id.0) {
        log::warn!("Failed to create process for window: {e:?}");
      }
    }

    // Emit events
    self.emit_all(events);

    new_process_ids
  }

  /// Set currently focused window. Emits `FocusWindow` if value changed.
  pub(crate) fn set_focused_window(&self, window_id: Option<WindowId>) {
    let changed = {
      let mut state = self.state.write();
      if state.focused_window == window_id {
        false
      } else {
        state.focused_window = window_id;
        true
      }
    };
    if changed {
      self.emit(Event::FocusWindow { window_id });
    }
  }

  /// Update mouse position and emit event if changed.
  pub(crate) fn update_mouse_position(&self, pos: crate::types::Point) {
    let changed = {
      let mut state = self.state.write();
      let changed = state
        .mouse_position
        .is_none_or(|last| pos.moved_from(last, 1.0));
      if changed {
        state.mouse_position = Some(pos);
      }
      changed
    };
    if changed {
      self.emit(Event::MousePosition(pos));
    }
  }

  /// Register an element. Returns existing if hash matches.
  pub(crate) fn register_element(&self, elem_state: ElementState) -> Option<AXElement> {
    let mut events = Vec::new();
    let result = {
      let mut state = self.state.write();
      Self::register_internal(&mut state, elem_state, self, &mut events)
    };
    self.emit_all(events);
    result
  }

  /// Update element data. Emits `ElementChanged` if actually changed.
  pub(crate) fn update_element(&self, element_id: ElementId, updated: AXElement) -> AxioResult<()> {
    let maybe_event = {
      let mut state = self.state.write();
      let elem_state = state
        .elements
        .get_mut(&element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;

      if elem_state.element == updated {
        None
      } else {
        elem_state.element = updated.clone();
        Some(Event::ElementChanged { element: updated })
      }
    };

    if let Some(event) = maybe_event {
      self.emit(event);
    }
    Ok(())
  }

  /// Set children for an element. Emits `ElementChanged` if children changed.
  pub(crate) fn set_element_children(
    &self,
    element_id: ElementId,
    children: Vec<ElementId>,
  ) -> AxioResult<()> {
    let maybe_event = {
      let mut state = self.state.write();
      let elem_state = state
        .elements
        .get_mut(&element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;

      let new_children = Some(children);
      if elem_state.element.children == new_children {
        None
      } else {
        elem_state.element.children = new_children;
        Some(Event::ElementChanged {
          element: elem_state.element.clone(),
        })
      }
    };

    if let Some(event) = maybe_event {
      self.emit(event);
    }
    Ok(())
  }

  /// Remove an element (cascades to children).
  pub(crate) fn remove_element(&self, element_id: ElementId) {
    let events = {
      let mut state = self.state.write();
      Self::remove_element_internal(&mut state, element_id)
    };
    self.emit_all(events);
  }

  /// Update focused element for a process. Emits `FocusElement` event.
  pub(crate) fn update_focus(&self, pid: u32, element: AXElement) -> Option<ElementId> {
    let (previous_id, should_emit) = {
      let mut state = self.state.write();
      let process_id = ProcessId(pid);
      let process = state.processes.get_mut(&process_id)?;

      let previous = process.focused_element;
      let same_element = previous == Some(element.id);

      if same_element {
        return previous;
      }

      process.focused_element = Some(element.id);
      (previous, true)
    };

    if !should_emit {
      return previous_id;
    }

    // Auto-unwatch previous element
    if let Some(prev_id) = previous_id {
      if let Some(prev_elem) = self.get_element(prev_id) {
        if prev_elem.role.auto_watch_on_focus() || prev_elem.role.is_writable() {
          drop(self.unwatch(prev_id));
        }
      }
    }

    // Auto-watch new element
    if element.role.auto_watch_on_focus() || element.role.is_writable() {
      drop(self.watch(element.id));
    }

    // Emit focus event
    self.emit(Event::FocusElement {
      element,
      previous_element_id: previous_id,
    });

    previous_id
  }

  /// Update selection for a process. Emits `SelectionChanged` if changed.
  pub(crate) fn update_selection(
    &self,
    pid: u32,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    let new_selection = TextSelection {
      element_id,
      text,
      range,
    };

    let should_emit = {
      let mut state = self.state.write();
      let process_id = ProcessId(pid);
      let Some(process) = state.processes.get_mut(&process_id) else {
        return;
      };

      let changed = process.last_selection.as_ref() != Some(&new_selection);
      process.last_selection = Some(new_selection.clone());
      changed
    };

    if should_emit {
      self.emit(Event::SelectionChanged {
        window_id,
        element_id: new_selection.element_id,
        text: new_selection.text,
        range: new_selection.range,
      });
    }
  }
}
