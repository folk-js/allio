/*!
Internal state operations that maintain registry invariants.

These functions take `&mut State` directly and should only be modified
with care - they maintain the consistency of the element/window registry.
*/

use super::state::{ElementState, State};
use super::Axio;
use crate::accessibility::Notification;
use crate::platform::{Observer, PlatformObserver};
use crate::types::{ElementId, Event, WindowId};

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
    mut elem_state: ElementState,
    axio: &Axio,
    events: &mut Vec<Event>,
  ) -> Option<crate::types::AXElement> {
    let hash = elem_state.hash;
    let parent_hash = elem_state.parent_hash;

    // Check for existing element with same hash
    if let Some(existing_id) = state.hash_to_element.get(&hash) {
      if let Some(existing) = state.elements.get(existing_id) {
        return Some(existing.element.clone());
      }
    }

    // Try to link orphan to parent if parent exists in registry
    if !elem_state.element.is_root && elem_state.element.parent_id.is_none() {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = state.hash_to_element.get(ph) {
          elem_state.element.parent_id = Some(parent_id);
        }
      }
    }

    let element_id = elem_state.element.id;
    let window_id = elem_state.element.window_id;
    let process_id = elem_state.element.pid;
    let element_parent_id = elem_state.element.parent_id;
    let is_root = elem_state.element.is_root;

    // Subscribe to destruction notification
    if let Some(process) = state.processes.get(&process_id) {
      Self::subscribe_destruction(&mut elem_state, &process.observer, axio);
    }

    // Clone element for return value and event before moving into state
    let element = elem_state.element.clone();

    state.elements.insert(element_id, elem_state);
    state.element_to_window.insert(element_id, window_id);
    state.hash_to_element.insert(hash, element_id);

    // Link to parent
    if let Some(parent_id) = element_parent_id {
      Self::add_child_to_parent(state, parent_id, element_id, events);
    } else if !is_root {
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
  pub(super) fn link_orphan_to_parent(
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
  pub(super) fn add_child_to_parent(
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

  /// Create watch for an element with destruction notification.
  pub(super) fn subscribe_destruction(
    elem_state: &mut ElementState,
    observer: &Observer,
    axio: &Axio,
  ) {
    if elem_state.watch.is_some() {
      return;
    }

    // Create watch with Destroyed notification - additional notifications added via watch_element
    match observer.create_watch(
      &elem_state.handle,
      elem_state.element.id,
      &[Notification::Destroyed],
      axio.clone(),
    ) {
      Ok(watch_handle) => {
        elem_state.watch = Some(watch_handle);
      }
      Err(e) => {
        log::debug!(
          "Failed to create watch for element {} (role: {}): {:?}",
          elem_state.element.id,
          elem_state.raw_role,
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

    // Unsubscribe from notifications (RAII - drop the watch handle)
    elem_state.watch.take();

    events.push(Event::ElementRemoved { element_id });

    events
  }

  /// Remove a child from a parent's children list.
  pub(super) fn remove_child_from_parent(
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
}

