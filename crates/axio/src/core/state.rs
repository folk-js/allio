/*!
State - the single source of truth for cached accessibility data.

All fields are private. Mutations go through methods that maintain invariants
and emit events. This guarantees:
- Indexes are always updated
- Events are always emitted
- Cascades always happen
*/

use async_broadcast::Sender;
use std::collections::HashMap;

use crate::platform::{Handle, Observer, WatchHandle};
use crate::types::{
  AXElement, AXWindow, ElementId, Event, Point, ProcessId, TextSelection, WindowId,
};

// ============================================================================
// Entity State Types
// ============================================================================

/// Per-process state.
pub(crate) struct ProcessState {
  pub(crate) observer: Observer,
  pub(crate) app_handle: Handle,
  pub(crate) focused_element: Option<ElementId>,
  pub(crate) last_selection: Option<TextSelection>,
}

/// Per-window state.
pub(crate) struct WindowState {
  process_id: ProcessId,
  info: AXWindow,
  handle: Option<Handle>,
}

/// Per-element state.
pub(crate) struct ElementState {
  pub(crate) element: AXElement,
  pub(crate) handle: Handle,
  pub(crate) hash: u64,
  pub(crate) parent_hash: Option<u64>,
  pub(crate) watch: Option<WatchHandle>,
}

impl ElementState {
  pub(crate) fn new(
    element: AXElement,
    handle: Handle,
    hash: u64,
    parent_hash: Option<u64>,
  ) -> Self {
    Self {
      element,
      handle,
      hash,
      parent_hash,
      watch: None,
    }
  }

  pub(crate) fn pid(&self) -> u32 {
    self.element.pid.0
  }
}

// ============================================================================
// State
// ============================================================================

/// Internal state storage with automatic event emission.
pub(crate) struct State {
  // Event emission
  events_tx: Sender<Event>,

  // Primary collections (PRIVATE)
  processes: HashMap<ProcessId, ProcessState>,
  windows: HashMap<WindowId, WindowState>,
  elements: HashMap<ElementId, ElementState>,

  // Indexes (PRIVATE)
  element_to_window: HashMap<ElementId, WindowId>,
  hash_to_element: HashMap<u64, ElementId>,
  waiting_for_parent: HashMap<u64, Vec<ElementId>>,

  // Focus/UI state (PRIVATE)
  focused_window: Option<WindowId>,
  depth_order: Vec<WindowId>,
  mouse_position: Option<Point>,
}

impl State {
  pub(crate) fn new(events_tx: Sender<Event>) -> Self {
    Self {
      events_tx,
      processes: HashMap::new(),
      windows: HashMap::new(),
      elements: HashMap::new(),
      element_to_window: HashMap::new(),
      hash_to_element: HashMap::new(),
      waiting_for_parent: HashMap::new(),
      focused_window: None,
      depth_order: Vec::new(),
      mouse_position: None,
    }
  }

  fn emit(&self, event: Event) {
    drop(self.events_tx.try_broadcast(event));
  }
}

// ============================================================================
// Element Operations
// ============================================================================

impl State {
  /// Get or insert an element by hash.
  ///
  /// - If hash exists: returns existing ElementId (no event)
  /// - If new: inserts, maintains indexes, resolves orphans, emits ElementAdded
  ///
  /// NOTE: Does not set up watch subscription - caller must do that via `set_element_watch`.
  pub(crate) fn get_or_insert_element(&mut self, mut elem: ElementState) -> ElementId {
    let hash = elem.hash;
    let parent_hash = elem.parent_hash;

    // Fast path: already exists
    if let Some(&existing_id) = self.hash_to_element.get(&hash) {
      if self.elements.contains_key(&existing_id) {
        return existing_id;
      }
    }

    // Try to link to parent if parent exists
    if !elem.element.is_root && elem.element.parent_id.is_none() {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = self.hash_to_element.get(ph) {
          elem.element.parent_id = Some(parent_id);
        }
      }
    }

    let element_id = elem.element.id;
    let window_id = elem.element.window_id;
    let element_parent_id = elem.element.parent_id;
    let is_root = elem.element.is_root;
    let element_clone = elem.element.clone();

    // Insert into primary collection and indexes
    self.elements.insert(element_id, elem);
    self.element_to_window.insert(element_id, window_id);
    self.hash_to_element.insert(hash, element_id);

    // Link to parent's children list
    if let Some(parent_id) = element_parent_id {
      self.add_child_to_parent(parent_id, element_id);
    } else if !is_root {
      // Orphan: has parent in OS but not loaded yet
      if let Some(ref ph) = parent_hash {
        self
          .waiting_for_parent
          .entry(*ph)
          .or_default()
          .push(element_id);
      }
    }

    // Resolve any orphans waiting for this element
    if let Some(orphans) = self.waiting_for_parent.remove(&hash) {
      for orphan_id in orphans {
        self.link_orphan_to_parent(orphan_id, element_id);
      }
    }

    self.emit(Event::ElementAdded {
      element: element_clone,
    });
    element_id
  }

  /// Update element data. Emits ElementChanged if data differs.
  pub(crate) fn update_element(&mut self, id: ElementId, data: AXElement) -> bool {
    let Some(elem) = self.elements.get_mut(&id) else {
      return false;
    };

    if elem.element == data {
      return false;
    }

    elem.element = data.clone();
    self.emit(Event::ElementChanged { element: data });
    true
  }

  /// Set children for an element. Emits ElementChanged if different.
  pub(crate) fn set_element_children(&mut self, id: ElementId, children: Vec<ElementId>) -> bool {
    let new_children = Some(children);

    let element_clone = {
      let Some(elem) = self.elements.get_mut(&id) else {
        return false;
      };

      if elem.element.children == new_children {
        return false;
      }

      elem.element.children = new_children;
      elem.element.clone()
    };

    self.emit(Event::ElementChanged {
      element: element_clone,
    });
    true
  }

  /// Remove an element and cascade to children.
  pub(crate) fn remove_element(&mut self, id: ElementId) {
    self.remove_element_recursive(id);
  }

  fn remove_element_recursive(&mut self, id: ElementId) {
    let Some(_) = self.element_to_window.remove(&id) else {
      return;
    };

    let Some(mut elem) = self.elements.remove(&id) else {
      return;
    };

    // Remove from parent's children list
    if let Some(parent_id) = elem.element.parent_id {
      self.remove_child_from_parent(parent_id, id);
    }

    // Clean waiting_for_parent
    if let Some(ref ph) = elem.parent_hash {
      if let Some(waiting) = self.waiting_for_parent.get_mut(ph) {
        waiting.retain(|&wid| wid != id);
        if waiting.is_empty() {
          self.waiting_for_parent.remove(ph);
        }
      }
    }
    self.waiting_for_parent.remove(&elem.hash);

    // Cascade to children
    if let Some(children) = &elem.element.children {
      for child_id in children.clone() {
        self.remove_element_recursive(child_id);
      }
    }

    // Clean hash index
    self.hash_to_element.remove(&elem.hash);

    // Drop watch (RAII cleanup)
    elem.watch.take();

    self.emit(Event::ElementRemoved { element_id: id });
  }

  /// Set watch handle for element.
  pub(crate) fn set_element_watch(&mut self, id: ElementId, watch: WatchHandle) {
    if let Some(elem) = self.elements.get_mut(&id) {
      elem.watch = Some(watch);
    }
  }

  /// Get mutable watch handle for element (for adding/removing notifications).
  pub(crate) fn get_element_watch_mut(&mut self, id: ElementId) -> Option<&mut WatchHandle> {
    self.elements.get_mut(&id).and_then(|e| e.watch.as_mut())
  }

  // --- Internal helpers ---

  fn add_child_to_parent(&mut self, parent_id: ElementId, child_id: ElementId) {
    let element_clone = {
      let Some(parent) = self.elements.get_mut(&parent_id) else {
        return;
      };
      let children = parent.element.children.get_or_insert_with(Vec::new);
      if children.contains(&child_id) {
        return;
      }
      children.push(child_id);
      parent.element.clone()
    };

    self.emit(Event::ElementChanged {
      element: element_clone,
    });
  }

  fn remove_child_from_parent(&mut self, parent_id: ElementId, child_id: ElementId) {
    let element_clone = {
      let Some(parent) = self.elements.get_mut(&parent_id) else {
        return;
      };
      let Some(children) = &mut parent.element.children else {
        return;
      };
      let old_len = children.len();
      children.retain(|&cid| cid != child_id);
      if children.len() == old_len {
        return;
      }
      parent.element.clone()
    };

    self.emit(Event::ElementChanged {
      element: element_clone,
    });
  }

  fn link_orphan_to_parent(&mut self, orphan_id: ElementId, parent_id: ElementId) {
    let element_clone = {
      let Some(orphan) = self.elements.get_mut(&orphan_id) else {
        return;
      };
      orphan.element.parent_id = Some(parent_id);
      orphan.element.clone()
    };

    self.emit(Event::ElementChanged {
      element: element_clone,
    });
    self.add_child_to_parent(parent_id, orphan_id);
  }
}

// ============================================================================
// Window Operations
// ============================================================================

impl State {
  /// Get or insert a window.
  ///
  /// - If window ID exists: returns false (already existed)
  /// - If new: inserts, updates depth order, emits WindowAdded, returns true
  pub(crate) fn get_or_insert_window(
    &mut self,
    id: WindowId,
    process_id: ProcessId,
    info: AXWindow,
    handle: Option<Handle>,
  ) -> bool {
    if self.windows.contains_key(&id) {
      return false;
    }

    self.windows.insert(
      id,
      WindowState {
        process_id,
        info: info.clone(),
        handle,
      },
    );
    self.update_depth_order();
    self.emit(Event::WindowAdded { window: info });
    true
  }

  /// Update window info. Emits WindowChanged if different.
  pub(crate) fn update_window(&mut self, id: WindowId, info: AXWindow) -> bool {
    let Some(window) = self.windows.get_mut(&id) else {
      return false;
    };

    if window.info == info {
      return false;
    }

    window.info = info.clone();
    self.emit(Event::WindowChanged { window: info });
    true
  }

  /// Remove a window and cascade to all its elements.
  pub(crate) fn remove_window(&mut self, id: WindowId) {
    // First remove all elements belonging to this window
    let element_ids: Vec<ElementId> = self
      .elements
      .iter()
      .filter(|(_, e)| e.element.window_id == id)
      .map(|(eid, _)| *eid)
      .collect();

    for element_id in element_ids {
      self.remove_element(element_id);
    }

    // Then remove window
    if let Some(window) = self.windows.remove(&id) {
      self.update_depth_order();
      self.emit(Event::WindowRemoved { window_id: id });

      // Check if process should be removed
      let pid = window.process_id;
      let has_windows = self.windows.values().any(|w| w.process_id == pid);
      if !has_windows {
        self.processes.remove(&pid);
      }
    }
  }

  fn update_depth_order(&mut self) {
    let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
    windows.sort_by_key(|w| w.z_index);
    self.depth_order = windows.into_iter().map(|w| w.id).collect();
  }
}

// ============================================================================
// Process Operations
// ============================================================================

impl State {
  /// Insert a process.
  pub(crate) fn insert_process(&mut self, id: ProcessId, process: ProcessState) {
    self.processes.insert(id, process);
  }

  /// Check if process exists.
  pub(crate) fn has_process(&self, id: ProcessId) -> bool {
    self.processes.contains_key(&id)
  }

  /// Get process state.
  pub(crate) fn get_process(&self, id: ProcessId) -> Option<&ProcessState> {
    self.processes.get(&id)
  }

  /// Get mutable process state.
  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_process_mut(&mut self, id: ProcessId) -> Option<&mut ProcessState> {
    self.processes.get_mut(&id)
  }
}

// ============================================================================
// Focus & Selection
// ============================================================================

impl State {
  /// Set focused window. Emits FocusWindow if changed.
  pub(crate) fn set_focused_window(&mut self, id: Option<WindowId>) -> bool {
    if self.focused_window == id {
      return false;
    }
    self.focused_window = id;
    self.emit(Event::FocusWindow { window_id: id });
    true
  }

  /// Set focused element for a process. Emits FocusElement if changed.
  /// Returns (changed, previous_element_id).
  pub(crate) fn set_focused_element(
    &mut self,
    pid: ProcessId,
    element: AXElement,
  ) -> (bool, Option<ElementId>) {
    let Some(process) = self.processes.get_mut(&pid) else {
      return (false, None);
    };

    let previous = process.focused_element;
    if previous == Some(element.id) {
      return (false, previous);
    }

    process.focused_element = Some(element.id);
    self.emit(Event::FocusElement {
      element,
      previous_element_id: previous,
    });
    (true, previous)
  }

  /// Set selection. Emits SelectionChanged if changed.
  pub(crate) fn set_selection(
    &mut self,
    pid: ProcessId,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) -> bool {
    let new_selection = TextSelection {
      element_id,
      text: text.clone(),
      range,
    };

    let Some(process) = self.processes.get_mut(&pid) else {
      return false;
    };

    if process.last_selection.as_ref() == Some(&new_selection) {
      return false;
    }

    process.last_selection = Some(new_selection);
    self.emit(Event::SelectionChanged {
      window_id,
      element_id,
      text,
      range,
    });
    true
  }

  /// Update mouse position. Emits MousePosition if changed significantly.
  pub(crate) fn set_mouse_position(&mut self, pos: Point) -> bool {
    let changed = self
      .mouse_position
      .is_none_or(|last| pos.moved_from(last, 1.0));
    if !changed {
      return false;
    }
    self.mouse_position = Some(pos);
    self.emit(Event::MousePosition(pos));
    true
  }
}

// ============================================================================
// Queries (read-only)
// ============================================================================

impl State {
  // --- Elements ---

  pub(crate) fn get_element(&self, id: ElementId) -> Option<&AXElement> {
    self.elements.get(&id).map(|e| &e.element)
  }

  pub(crate) fn get_element_state(&self, id: ElementId) -> Option<&ElementState> {
    self.elements.get(&id)
  }

  pub(crate) fn find_element_by_hash(&self, hash: u64) -> Option<ElementId> {
    self.hash_to_element.get(&hash).copied()
  }

  pub(crate) fn get_all_elements(&self) -> impl Iterator<Item = &AXElement> {
    self.elements.values().map(|e| &e.element)
  }

  // --- Windows ---

  pub(crate) fn get_window(&self, id: WindowId) -> Option<&AXWindow> {
    self.windows.get(&id).map(|w| &w.info)
  }

  pub(crate) fn get_window_handle(&self, id: WindowId) -> Option<&Handle> {
    self.windows.get(&id).and_then(|w| w.handle.as_ref())
  }

  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_window_process_id(&self, id: WindowId) -> Option<ProcessId> {
    self.windows.get(&id).map(|w| w.process_id)
  }

  pub(crate) fn get_all_windows(&self) -> impl Iterator<Item = &AXWindow> {
    self.windows.values().map(|w| &w.info)
  }

  pub(crate) fn get_all_window_ids(&self) -> impl Iterator<Item = WindowId> + '_ {
    self.windows.keys().copied()
  }

  // --- Focus/UI ---

  pub(crate) fn get_focused_window(&self) -> Option<WindowId> {
    self.focused_window
  }

  pub(crate) fn get_depth_order(&self) -> &[WindowId] {
    &self.depth_order
  }

  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_mouse_position(&self) -> Option<Point> {
    self.mouse_position
  }

  /// Find window at point (uses z-order).
  pub(crate) fn get_window_at_point(&self, x: f64, y: f64) -> Option<&AXWindow> {
    let point = Point::new(x, y);
    let mut candidates: Vec<_> = self
      .windows
      .values()
      .filter(|w| w.info.bounds.contains(point))
      .collect();
    candidates.sort_by_key(|w| w.info.z_index);
    candidates.first().map(|w| &w.info)
  }

  /// Get focused window for a specific PID.
  pub(crate) fn get_focused_window_for_pid(&self, pid: u32) -> Option<WindowId> {
    let window_id = self.focused_window?;
    let window = self.windows.get(&window_id)?;
    if window.process_id.0 == pid {
      Some(window_id)
    } else {
      None
    }
  }

  /// Build a snapshot of current state.
  pub(crate) fn snapshot(&self) -> crate::types::Snapshot {
    let (focused_element, selection) = self
      .focused_window
      .and_then(|wid| {
        let window = self.windows.get(&wid)?;
        let process = self.processes.get(&window.process_id)?;
        let focused = process
          .focused_element
          .and_then(|eid| self.elements.get(&eid).map(|e| e.element.clone()));
        Some((focused, process.last_selection.clone()))
      })
      .unwrap_or((None, None));

    crate::types::Snapshot {
      windows: self.windows.values().map(|w| w.info.clone()).collect(),
      elements: self.elements.values().map(|e| e.element.clone()).collect(),
      focused_window: self.focused_window,
      focused_element,
      selection,
      depth_order: self.depth_order.clone(),
      mouse_position: self.mouse_position,
    }
  }
}
