/*!
Window operations for the Registry.

CRUD: upsert_window, update_window, remove_window
Query: window, windows, window_ids
Window-specific: set_window_handle, window_root, set_window_root
*/

use super::{Registry, WindowEntry};
use crate::platform::Handle;
use crate::types::{ElementId, Event, ProcessId, Window, WindowId};

// ============================================================================
// Window CRUD
// ============================================================================

impl Registry {
  /// Insert a window if it doesn't exist.
  ///
  /// Returns the window ID (whether newly inserted or already present).
  /// If new: inserts, updates z-order, emits WindowAdded.
  pub(crate) fn upsert_window(
    &mut self,
    id: WindowId,
    process_id: ProcessId,
    info: Window,
    handle: Option<Handle>,
  ) -> WindowId {
    if self.windows.contains_key(&id) {
      return id;
    }

    // Maintain window handle index
    if let Some(ref h) = handle {
      self.window_handle_to_id.insert(h.clone(), id);
    }

    self.windows.insert(
      id,
      WindowEntry {
        process_id,
        info: info.clone(),
        handle,
        root_element: None,
      },
    );
    self.update_z_order();
    self.emit(Event::WindowAdded { window: info });
    id
  }

  /// Update window info. Emits WindowChanged if different.
  pub(crate) fn update_window(&mut self, id: WindowId, info: Window) {
    let Some(window) = self.windows.get_mut(&id) else {
      return;
    };

    if window.info == info {
      return;
    }

    let z_changed = window.info.z_index != info.z_index;
    window.info = info.clone();

    if z_changed {
      self.update_z_order();
    }

    self.emit(Event::WindowChanged { window: info });
  }

  /// Remove a window and cascade to all its elements.
  pub(crate) fn remove_window(&mut self, id: WindowId) {
    // First remove all elements belonging to this window
    let element_ids: Vec<ElementId> = self
      .elements
      .iter()
      .filter(|(_, e)| e.data.window_id == id)
      .map(|(eid, _)| *eid)
      .collect();

    for element_id in element_ids {
      self.remove_element(element_id);
    }

    // Then remove window
    if let Some(window) = self.windows.remove(&id) {
      // Clean up window handle index
      if let Some(ref handle) = window.handle {
        self.window_handle_to_id.remove(handle);
      }

      self.update_z_order();
      self.emit(Event::WindowRemoved { window_id: id });

      // Check if process should be removed
      let pid = window.process_id;
      let has_windows = self.windows.values().any(|w| w.process_id == pid);
      if !has_windows {
        self.remove_process(pid);
      }
    }
  }

  /// Update z-order after window changes.
  pub(super) fn update_z_order(&mut self) {
    let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
    windows.sort_by_key(|w| w.z_index);
    self.z_order = windows.into_iter().map(|w| w.id).collect();
  }
}

// ============================================================================
// Window Queries
// ============================================================================

impl Registry {
  /// Get window entry by ID.
  pub(crate) fn window(&self, id: WindowId) -> Option<&WindowEntry> {
    self.windows.get(&id)
  }

  /// Iterate over all window entries.
  pub(crate) fn windows(&self) -> impl Iterator<Item = &WindowEntry> {
    self.windows.values()
  }

  /// Iterate over all window IDs.
  pub(crate) fn window_ids(&self) -> impl Iterator<Item = WindowId> + '_ {
    self.windows.keys().copied()
  }

  /// Find window ID by its accessibility handle. O(1) lookup.
  pub(crate) fn find_window_by_handle(&self, handle: &Handle) -> Option<WindowId> {
    self.window_handle_to_id.get(handle).copied()
  }
}

// ============================================================================
// Window-Specific Operations
// ============================================================================

impl Registry {
  /// Set window handle (may be obtained lazily after initial insertion).
  pub(crate) fn set_window_handle(&mut self, id: WindowId, handle: Handle) {
    if let Some(window) = self.windows.get_mut(&id) {
      // Maintain window handle index
      self.window_handle_to_id.insert(handle.clone(), id);
      window.handle = Some(handle);
    }
  }

  /// Get cached root element for a window.
  pub(crate) fn window_root(&self, id: WindowId) -> Option<ElementId> {
    self.windows.get(&id).and_then(|w| w.root_element)
  }

  /// Set cached root element for a window.
  pub(crate) fn set_window_root(&mut self, id: WindowId, element_id: ElementId) {
    if let Some(window) = self.windows.get_mut(&id) {
      window.root_element = Some(element_id);
    }
  }
}
