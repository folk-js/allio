/*!
State mutation methods.
*/

use super::state::{ElementData, ElementState, ProcessState, State, WindowState};
use super::Axio;
use crate::accessibility::Notification;
use crate::platform::{self, ObserverHandle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, Event, ProcessId, TextSelection, WindowId,
};
use std::collections::HashSet;
use std::ffi::c_void;

impl Axio {
  /// Get or create process state for a PID.
  /// Creates the `AXObserver` if this is a new process.
  pub(crate) fn get_or_create_process(&self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    // Fast path: check if already exists
    if self.state.read().processes.contains_key(&process_id) {
      return Ok(process_id);
    }

    // Slow path: create observer and insert
    let observer = platform::create_observer_for_pid(pid, self.clone())?;

    // Subscribe to app-level notifications (focus, selection)
    if let Err(e) = platform::subscribe_app_notifications(pid, &observer, self.clone()) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
    }

    self.state.write().processes.insert(
      process_id,
      ProcessState {
        observer,
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
            existing.handle = platform::fetch_window_handle(&existing.info);
          }
        } else {
          // New window
          let handle = platform::fetch_window_handle(&window_info);

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
  pub(crate) fn register_element(&self, data: ElementData) -> Option<AXElement> {
    let mut events = Vec::new();
    let result = {
      let mut state = self.state.write();
      Self::register_internal(&mut state, data, self, &mut events)
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
          drop(self.unwatch_element(prev_id));
        }
      }
    }

    // Auto-watch new element
    if element.role.auto_watch_on_focus() || element.role.is_writable() {
      drop(self.watch_element(element.id));
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

  /// Write typed value to element.
  pub(crate) fn write_element_value(
    &self,
    element_id: ElementId,
    value: &crate::accessibility::Value,
  ) -> AxioResult<()> {
    self.with_element_handle(element_id, |handle, platform_role| {
      platform::write_element_value(handle, value, platform_role)
    })?
  }

  /// Click element.
  pub(crate) fn click_element(&self, element_id: ElementId) -> AxioResult<()> {
    self.with_element_handle(element_id, |handle, _| platform::click_element(handle))?
  }
}

// === Internal State Operations (take &mut State) ===

impl Axio {
  /// Remove a window and cascade to all its elements.
  pub(super) fn remove_window_internal(state: &mut State, window_id: WindowId) -> Vec<Event> {
    let mut events = Vec::new();

    let element_ids: Vec<ElementId> = state
      .elements
      .iter()
      .filter(|(_, e)| e.element.window_id == window_id)
      .map(|(id, _)| *id)
      .collect();

    for element_id in element_ids {
      events.extend(Self::remove_element_internal(state, element_id));
    }

    if let Some(window_state) = state.windows.remove(&window_id) {
      let mut windows: Vec<_> = state.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      state.depth_order = windows.into_iter().map(|w| w.id).collect();

      events.push(Event::WindowRemoved { window_id });

      let process_id = window_state.process_id;
      let has_windows = state.windows.values().any(|w| w.process_id == process_id);
      if !has_windows {
        state.processes.remove(&process_id);
      }
    }

    events
  }

  /// Register a new element. Returns existing if hash matches.
  pub(super) fn register_internal(
    state: &mut State,
    data: ElementData,
    axio: &Axio,
    events: &mut Vec<Event>,
  ) -> Option<AXElement> {
    let ElementData {
      mut element,
      handle,
      hash,
      parent_hash,
      raw_role,
    } = data;

    let window_id = element.window_id;

    // Check for existing element with same hash
    if let Some(existing_id) = state.hash_to_element.get(&hash) {
      if let Some(existing) = state.elements.get(existing_id) {
        return Some(existing.element.clone());
      }
    }

    // Try to link orphan to parent if parent exists in registry
    if !element.is_root && element.parent_id.is_none() {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = state.hash_to_element.get(ph) {
          element.parent_id = Some(parent_id);
        }
      }
    }

    let element_id = element.id;
    let pid = element.pid.0;
    let process_id = element.pid;

    let mut elem_state = ElementState {
      element: element.clone(),
      handle,
      hash,
      parent_hash,
      pid,
      platform_role: raw_role,
      subscriptions: HashSet::new(),
      destruction_context: None,
      watch_context: None,
    };

    // Subscribe to destruction notification
    if let Some(process) = state.processes.get(&process_id) {
      Self::subscribe_destruction(&mut elem_state, &process.observer, axio);
    }

    state.elements.insert(element_id, elem_state);
    state.element_to_window.insert(element_id, window_id);
    state.hash_to_element.insert(hash, element_id);

    // Link to parent
    if let Some(parent_id) = element.parent_id {
      Self::add_child_to_parent(state, parent_id, element_id, events);
    } else if !element.is_root {
      // Orphan: has parent in OS but not loaded yet
      if let Some(ref ph) = parent_hash {
        state
          .waiting_for_parent
          .entry(*ph)
          .or_default()
          .push(element_id);
      }
    }

    // Link waiting orphans to this element
    if let Some(orphans) = state.waiting_for_parent.remove(&hash) {
      for orphan_id in orphans {
        Self::link_orphan_to_parent(state, orphan_id, element_id, events);
      }
    }

    events.push(Event::ElementAdded {
      element: element.clone(),
    });

    Some(element)
  }

  /// Link an orphan element to its newly-discovered parent.
  fn link_orphan_to_parent(
    state: &mut State,
    orphan_id: ElementId,
    parent_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(orphan_state) = state.elements.get_mut(&orphan_id) {
      orphan_state.element.parent_id = Some(parent_id);
      events.push(Event::ElementChanged {
        element: orphan_state.element.clone(),
      });
    }
    Self::add_child_to_parent(state, parent_id, orphan_id, events);
  }

  /// Add a child to a parent's children list.
  fn add_child_to_parent(
    state: &mut State,
    parent_id: ElementId,
    child_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(parent_state) = state.elements.get_mut(&parent_id) {
      let children = parent_state.element.children.get_or_insert_with(Vec::new);
      if !children.contains(&child_id) {
        children.push(child_id);
        events.push(Event::ElementChanged {
          element: parent_state.element.clone(),
        });
      }
    }
  }

  /// Subscribe to destruction notification for an element.
  fn subscribe_destruction(elem_state: &mut ElementState, observer: &ObserverHandle, axio: &Axio) {
    if elem_state.destruction_context.is_some() {
      return;
    }

    match platform::subscribe_destruction_notification(
      &elem_state.element.id,
      &elem_state.handle,
      observer,
      axio.clone(),
    ) {
      Ok(context) => {
        elem_state.destruction_context = Some(context.cast::<c_void>());
        elem_state.subscriptions.insert(Notification::Destroyed);
      }
      Err(e) => {
        log::debug!(
          "Failed to register destruction for element {} (role: {}): {:?}",
          elem_state.element.id,
          elem_state.platform_role,
          e
        );
      }
    }
  }

  /// Remove an element.
  pub(super) fn remove_element_internal(state: &mut State, element_id: ElementId) -> Vec<Event> {
    let mut events = Vec::new();

    let Some(_window_id) = state.element_to_window.remove(&element_id) else {
      return events;
    };

    let Some(mut elem_state) = state.elements.remove(&element_id) else {
      return events;
    };

    // Remove from parent's children
    if let Some(parent_id) = elem_state.element.parent_id {
      Self::remove_child_from_parent(state, parent_id, element_id, &mut events);
    }

    // Remove from waiting_for_parent
    if let Some(ref ph) = elem_state.parent_hash {
      if let Some(waiting) = state.waiting_for_parent.get_mut(ph) {
        waiting.retain(|&id| id != element_id);
        if waiting.is_empty() {
          state.waiting_for_parent.remove(ph);
        }
      }
    }

    state.waiting_for_parent.remove(&elem_state.hash);

    // Recursively remove children
    if let Some(children) = &elem_state.element.children {
      for child_id in children.clone() {
        events.extend(Self::remove_element_internal(state, child_id));
      }
    }

    state.hash_to_element.remove(&elem_state.hash);

    // Unsubscribe from notifications
    let process_id = ProcessId(elem_state.pid);
    if let Some(process) = state.processes.get(&process_id) {
      Self::unsubscribe_all(&mut elem_state, &process.observer);
    }

    events.push(Event::ElementRemoved { element_id });

    events
  }

  /// Remove a child from a parent's children list.
  fn remove_child_from_parent(
    state: &mut State,
    parent_id: ElementId,
    child_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(parent_state) = state.elements.get_mut(&parent_id) {
      if let Some(children) = &mut parent_state.element.children {
        let old_len = children.len();
        children.retain(|&id| id != child_id);
        if children.len() != old_len {
          events.push(Event::ElementChanged {
            element: parent_state.element.clone(),
          });
        }
      }
    }
  }

  /// Unsubscribe from all notifications for an element.
  fn unsubscribe_all(elem_state: &mut ElementState, observer: &ObserverHandle) {
    // Unsubscribe destruction tracking
    if let Some(context) = elem_state.destruction_context.take() {
      platform::unsubscribe_destruction_notification(
        &elem_state.handle,
        observer,
        context.cast::<platform::ObserverContextHandle>(),
      );
    }

    // Unsubscribe watch notifications
    if let Some(context) = elem_state.watch_context.take() {
      let notifs: Vec<_> = elem_state
        .subscriptions
        .iter()
        .filter(|n| **n != Notification::Destroyed)
        .copied()
        .collect();

      platform::unsubscribe_notifications(
        &elem_state.handle,
        observer,
        context.cast::<platform::ObserverContextHandle>(),
        &notifs,
      );
    }

    elem_state.subscriptions.clear();
  }
}
