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

use super::tree::ElementTree;
use crate::accessibility::{Action, Role, Value};
use crate::platform::{AppNotificationHandle, Handle, Observer, WatchHandle};
use crate::types::{
  Bounds, Element, ElementId, Event, Point, ProcessId, TextSelection, Window, WindowId,
};

// ============================================================================
// Entity State Types
// ============================================================================

/// Per-process state.
pub(crate) struct ProcessEntry {
  pub(crate) observer: Observer,
  pub(crate) app_handle: Handle,
  pub(crate) focused_element: Option<ElementId>,
  pub(crate) last_selection: Option<TextSelection>,
  /// Handle to app-level notifications. Cleaned up via Drop when process is removed.
  pub(crate) _app_notifications: Option<AppNotificationHandle>,
}

/// Per-window state.
pub(crate) struct WindowEntry {
  process_id: ProcessId,
  info: Window,
  handle: Option<Handle>,
}

/// Pure element data without tree relationships.
///
/// This is the internal storage type. Tree relationships (parent/children)
/// are managed separately in `ElementTree` and derived when building
/// the public `Element` type for events/queries.
#[derive(Debug, Clone)]
pub(crate) struct ElementData {
  pub id: ElementId,
  pub window_id: WindowId,
  pub pid: ProcessId,
  pub is_root: bool,
  pub role: Role,
  pub platform_role: String,
  pub label: Option<String>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub url: Option<String>,
  pub value: Option<Value>,
  pub bounds: Option<Bounds>,
  pub focused: Option<bool>,
  pub disabled: bool,
  pub selected: Option<bool>,
  pub expanded: Option<bool>,
  pub row_index: Option<usize>,
  pub column_index: Option<usize>,
  pub row_count: Option<usize>,
  pub column_count: Option<usize>,
  pub actions: Vec<Action>,
  pub is_fallback: bool,
}

impl ElementData {
  /// Create ElementData from platform attributes.
  pub(crate) fn from_attributes(
    id: ElementId,
    window_id: WindowId,
    pid: ProcessId,
    is_root: bool,
    attrs: crate::platform::ElementAttributes,
  ) -> Self {
    Self {
      id,
      window_id,
      pid,
      is_root,
      role: attrs.role,
      platform_role: attrs.platform_role,
      label: attrs.title,
      description: attrs.description,
      placeholder: attrs.placeholder,
      url: attrs.url,
      value: attrs.value,
      bounds: attrs.bounds,
      focused: attrs.focused,
      disabled: attrs.disabled,
      selected: attrs.selected,
      expanded: attrs.expanded,
      row_index: attrs.row_index,
      column_index: attrs.column_index,
      row_count: attrs.row_count,
      column_count: attrs.column_count,
      actions: attrs.actions,
      is_fallback: false,
    }
  }
}

/// Per-element state in the registry.
pub(crate) struct ElementEntry {
  pub(crate) data: ElementData,
  pub(crate) handle: Handle,
  pub(crate) hash: u64,
  pub(crate) parent_hash: Option<u64>,
  pub(crate) watch: Option<WatchHandle>,
}

impl ElementEntry {
  pub(crate) fn new(
    data: ElementData,
    handle: Handle,
    hash: u64,
    parent_hash: Option<u64>,
  ) -> Self {
    Self {
      data,
      handle,
      hash,
      parent_hash,
      watch: None,
    }
  }

  pub(crate) fn pid(&self) -> u32 {
    self.data.pid.0
  }
}

// ============================================================================
// State
// ============================================================================

/// Internal state storage with automatic event emission.
pub(crate) struct Registry {
  // Event emission
  events_tx: Sender<Event>,

  // Primary collections (PRIVATE)
  processes: HashMap<ProcessId, ProcessEntry>,
  windows: HashMap<WindowId, WindowEntry>,
  elements: HashMap<ElementId, ElementEntry>,

  // Tree structure - single source of truth for relationships
  tree: ElementTree,

  // Indexes (PRIVATE)
  hash_to_element: HashMap<u64, ElementId>,
  waiting_for_parent: HashMap<u64, Vec<ElementId>>,

  // Focus/UI state (PRIVATE)
  focused_window: Option<WindowId>,
  z_order: Vec<WindowId>,
  mouse_position: Option<Point>,
}

impl Registry {
  pub(crate) fn new(events_tx: Sender<Event>) -> Self {
    Self {
      events_tx,
      processes: HashMap::new(),
      windows: HashMap::new(),
      elements: HashMap::new(),
      tree: ElementTree::new(),
      hash_to_element: HashMap::new(),
      waiting_for_parent: HashMap::new(),
      focused_window: None,
      z_order: Vec::new(),
      mouse_position: None,
    }
  }

  fn emit(&self, event: Event) {
    if let Err(e) = self.events_tx.try_broadcast(event) {
      if e.is_full() {
        log::error!(
          "Event channel overflow - events are being dropped. \
           Consider increasing EVENT_CHANNEL_CAPACITY or processing events faster."
        );
      }
    }
  }

  /// Build an Element snapshot from ElementData + tree relationships.
  /// This derives parent_id and children from ElementTree.
  pub(crate) fn build_element(&self, id: ElementId) -> Option<Element> {
    let elem = self.elements.get(&id)?;
    let data = &elem.data;

    // Derive relationships from tree
    let parent_id = if data.is_root {
      None
    } else {
      self.tree.parent(id)
    };

    // Get children from tree (empty vec if no children tracked)
    let children_slice = self.tree.children(id);
    let children = if children_slice.is_empty() && !self.tree.has_children(id) {
      // Children not yet fetched (vs explicitly empty)
      None
    } else {
      Some(children_slice.to_vec())
    };

    Some(Element {
      id,
      window_id: data.window_id,
      pid: data.pid,
      is_root: data.is_root,
      parent_id,
      children,
      role: data.role,
      platform_role: data.platform_role.clone(),
      label: data.label.clone(),
      description: data.description.clone(),
      placeholder: data.placeholder.clone(),
      url: data.url.clone(),
      value: data.value.clone(),
      bounds: data.bounds,
      focused: data.focused,
      disabled: data.disabled,
      selected: data.selected,
      expanded: data.expanded,
      row_index: data.row_index,
      column_index: data.column_index,
      row_count: data.row_count,
      column_count: data.column_count,
      actions: data.actions.clone(),
      is_fallback: data.is_fallback,
    })
  }

  /// Emit ElementAdded with derived relationships.
  fn emit_element_added(&self, id: ElementId) {
    if let Some(element) = self.build_element(id) {
      self.emit(Event::ElementAdded { element });
    }
  }

  /// Emit ElementChanged with derived relationships.
  fn emit_element_changed(&self, id: ElementId) {
    if let Some(element) = self.build_element(id) {
      self.emit(Event::ElementChanged { element });
    }
  }
}

// ============================================================================
// Element Operations
// ============================================================================

impl Registry {
  /// Get or insert an element by hash.
  ///
  /// - If hash exists in same window: updates data from fresh fetch, returns (existing ElementId, true) where true = already had watch
  /// - If new: inserts, maintains indexes, resolves orphans, emits ElementAdded, returns (id, false)
  ///
  /// NOTE: Does not set up watch subscription - caller must do that via `set_element_watch`.
  pub(crate) fn get_or_insert_element(&mut self, elem: ElementEntry) -> (ElementId, bool) {
    let hash = elem.hash;
    let parent_hash = elem.parent_hash;
    let is_root = elem.data.is_root;
    let window_id = elem.data.window_id;

    // Element already exists (same hash AND same window) - update data from fresh fetch
    if let Some(&existing_id) = self.hash_to_element.get(&hash) {
      if let Some(existing) = self.elements.get(&existing_id) {
        // Only dedup if same window - different windows can have elements with same CFHash
        if existing.data.window_id == window_id {
          let has_watch = existing.watch.is_some();
          // Update the element data with fresh values (preserves ID)
          let mut fresh_data = elem.data;
          fresh_data.id = existing_id; // Keep the existing ID
          self.update_element_data(existing_id, fresh_data);
          return (existing_id, has_watch);
        }
        // Different window with same hash - this is normal (common UI patterns), proceed with insert
        log::debug!(
          "Same hash {} in different windows: existing element {} (window {:?}), \
           new element for window {:?}. Registering as separate elements.",
          hash,
          existing_id,
          existing.data.window_id,
          window_id
        );
      }
    }

    let element_id = elem.data.id;

    // Insert element data into primary collection and hash index
    self.elements.insert(element_id, elem);
    // Note: This overwrites the hash_to_element entry. For cross-window hash sharing,
    // the most recently registered element "wins" the hash lookup. find_element_by_hash
    // handles this by searching when the fast-path element doesn't match the pid filter.
    self.hash_to_element.insert(hash, element_id);

    // Link to parent via tree if parent exists AND is in same window
    if !is_root {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = self.hash_to_element.get(ph) {
          // Check parent is in same window before linking
          if self
            .elements
            .get(&parent_id)
            .map_or(false, |p| p.data.window_id == window_id)
          {
            self.tree.add_child(parent_id, element_id);
            self.emit_element_changed(parent_id);
          } else {
            // Parent hash exists but in different window - treat as orphan
            self
              .waiting_for_parent
              .entry(*ph)
              .or_default()
              .push(element_id);
          }
        } else {
          // Orphan: parent not loaded yet, queue for later
          self
            .waiting_for_parent
            .entry(*ph)
            .or_default()
            .push(element_id);
        }
      }
    }

    // Resolve any orphans waiting for this element (only from same window)
    if let Some(orphans) = self.waiting_for_parent.remove(&hash) {
      for orphan_id in orphans {
        // Only link orphans from the same window
        let orphan_window = self.elements.get(&orphan_id).map(|e| e.data.window_id);
        if orphan_window == Some(window_id) {
          self.tree.add_child(element_id, orphan_id);
          self.emit_element_changed(orphan_id);
        } else {
          // Orphan is from a different window - re-queue it
          if let Some(orphan_elem) = self.elements.get(&orphan_id) {
            if let Some(ref ph) = orphan_elem.parent_hash {
              self
                .waiting_for_parent
                .entry(*ph)
                .or_default()
                .push(orphan_id);
            }
          }
        }
      }
    }

    self.emit_element_added(element_id);
    (element_id, false) // New element, no watch yet
  }

  /// Update element data from fresh platform fetch.
  /// Only updates the data fields (not relationships). Emits ElementChanged if data differs.
  /// Returns (element_exists, data_changed).
  pub(crate) fn update_element_data(
    &mut self,
    id: ElementId,
    new_data: ElementData,
  ) -> (bool, bool) {
    let Some(elem) = self.elements.get_mut(&id) else {
      return (false, false);
    };

    // Compare relevant fields (id, window_id, pid, is_root shouldn't change)
    let data = &elem.data;
    let changed = data.role != new_data.role
      || data.platform_role != new_data.platform_role
      || data.label != new_data.label
      || data.description != new_data.description
      || data.placeholder != new_data.placeholder
      || data.url != new_data.url
      || data.value != new_data.value
      || data.bounds != new_data.bounds
      || data.focused != new_data.focused
      || data.disabled != new_data.disabled
      || data.selected != new_data.selected
      || data.expanded != new_data.expanded
      || data.row_index != new_data.row_index
      || data.column_index != new_data.column_index
      || data.row_count != new_data.row_count
      || data.column_count != new_data.column_count
      || data.actions != new_data.actions
      || data.is_fallback != new_data.is_fallback;

    if !changed {
      return (true, false);
    }

    elem.data = new_data;
    self.emit_element_changed(id);
    (true, true)
  }

  /// Set children for an element in OS order.
  /// Updates tree relationships. Emits ElementChanged if different.
  /// Filters to only existing elements to prevent dangling refs.
  /// Returns (element_exists, children_changed).
  pub(crate) fn set_element_children(
    &mut self,
    id: ElementId,
    children: Vec<ElementId>,
  ) -> (bool, bool) {
    if !self.elements.contains_key(&id) {
      return (false, false);
    }

    // Filter to only existing elements (prevents dangling refs)
    let valid_children: Vec<ElementId> = children
      .into_iter()
      .filter(|&cid| self.elements.contains_key(&cid))
      .collect();

    let old_children = self.tree.children(id);
    if old_children == valid_children {
      return (true, false);
    }

    self.tree.set_children(id, valid_children);
    self.emit_element_changed(id);
    (true, true)
  }

  /// Remove an element and cascade to all descendants.
  pub(crate) fn remove_element(&mut self, id: ElementId) {
    // Remove subtree from tree structure, get all removed IDs
    let removed_ids = self.tree.remove_subtree(id);

    for removed_id in removed_ids {
      self.remove_element_data(removed_id);
    }
  }

  /// Remove element data and indexes (called after tree removal).
  fn remove_element_data(&mut self, id: ElementId) {
    let Some(mut elem) = self.elements.remove(&id) else {
      return;
    };

    // Clean hash index
    self.hash_to_element.remove(&elem.hash);

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

  /// Take watch handle from element (for operations that need to release the lock).
  pub(crate) fn take_element_watch(&mut self, id: ElementId) -> Option<WatchHandle> {
    self.elements.get_mut(&id).and_then(|e| e.watch.take())
  }
}

// ============================================================================
// Window Operations
// ============================================================================

impl Registry {
  /// Get or insert a window.
  ///
  /// - If window ID exists: returns false (already existed)
  /// - If new: inserts, updates depth order, emits WindowAdded, returns true
  pub(crate) fn get_or_insert_window(
    &mut self,
    id: WindowId,
    process_id: ProcessId,
    info: Window,
    handle: Option<Handle>,
  ) -> bool {
    if self.windows.contains_key(&id) {
      return false;
    }

    self.windows.insert(
      id,
      WindowEntry {
        process_id,
        info: info.clone(),
        handle,
      },
    );
    self.update_z_order();
    self.emit(Event::WindowAdded { window: info });
    true
  }

  /// Update window info. Emits WindowChanged if different.
  pub(crate) fn update_window(&mut self, id: WindowId, info: Window) -> bool {
    let Some(window) = self.windows.get_mut(&id) else {
      return false;
    };

    if window.info == info {
      return false;
    }

    let z_changed = window.info.z_index != info.z_index;
    window.info = info.clone();

    if z_changed {
      self.update_z_order();
    }

    self.emit(Event::WindowChanged { window: info });
    true
  }

  /// Set window handle (may be obtained lazily after initial insertion).
  pub(crate) fn set_window_handle(&mut self, id: WindowId, handle: Handle) {
    if let Some(window) = self.windows.get_mut(&id) {
      window.handle = Some(handle);
    }
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
      self.update_z_order();
      self.emit(Event::WindowRemoved { window_id: id });

      // Check if process should be removed
      let pid = window.process_id;
      let has_windows = self.windows.values().any(|w| w.process_id == pid);
      if !has_windows {
        self.processes.remove(&pid);
      }
    }
  }

  fn update_z_order(&mut self) {
    let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
    windows.sort_by_key(|w| w.z_index);
    self.z_order = windows.into_iter().map(|w| w.id).collect();
  }
}

// ============================================================================
// Process Operations
// ============================================================================

impl Registry {
  /// Try to insert a process. Returns false if process already exists (no-op).
  /// This handles the TOCTOU race where another thread may have inserted first.
  pub(crate) fn try_insert_process(&mut self, id: ProcessId, process: ProcessEntry) -> bool {
    use std::collections::hash_map::Entry;
    match self.processes.entry(id) {
      Entry::Occupied(_) => false, // Another thread won the race
      Entry::Vacant(e) => {
        e.insert(process);
        true
      }
    }
  }

  /// Check if process exists.
  pub(crate) fn has_process(&self, id: ProcessId) -> bool {
    self.processes.contains_key(&id)
  }

  /// Get process state.
  pub(crate) fn get_process(&self, id: ProcessId) -> Option<&ProcessEntry> {
    self.processes.get(&id)
  }

  /// Get mutable process state.
  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_process_mut(&mut self, id: ProcessId) -> Option<&mut ProcessEntry> {
    self.processes.get_mut(&id)
  }
}

// ============================================================================
// Focus & Selection
// ============================================================================

impl Registry {
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
    element: Element,
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

impl Registry {
  // --- Elements ---

  /// Get element with derived relationships.
  /// Returns a built Element (cloned, not a reference).
  pub(crate) fn get_element(&self, id: ElementId) -> Option<Element> {
    self.build_element(id)
  }

  /// Get internal element state (for handle access, etc).
  pub(crate) fn get_element_state(&self, id: ElementId) -> Option<&ElementEntry> {
    self.elements.get(&id)
  }

  /// Get element data (without relationships).
  #[allow(dead_code)] // Available for internal use
  pub(crate) fn get_element_data(&self, id: ElementId) -> Option<&ElementData> {
    self.elements.get(&id).map(|e| &e.data)
  }

  /// Find element by hash, optionally filtering by process ID.
  ///
  /// When `pid` is provided, ensures the returned element belongs to that process.
  /// This handles hash collisions across windows/processes where the hash_to_element
  /// fast-path might point to the wrong element.
  pub(crate) fn find_element_by_hash(
    &self,
    hash: u64,
    pid: Option<ProcessId>,
  ) -> Option<ElementId> {
    // Fast path: direct lookup
    if let Some(&element_id) = self.hash_to_element.get(&hash) {
      if let Some(elem) = self.elements.get(&element_id) {
        // If no pid filter, or pid matches, return it
        if pid.map_or(true, |p| elem.data.pid == p) {
          return Some(element_id);
        }
      }
    }

    // Slow path: pid filter didn't match (hash collision), search all elements
    if pid.is_some() {
      return self
        .elements
        .iter()
        .find(|(_, e)| e.hash == hash && Some(e.data.pid) == pid)
        .map(|(id, _)| *id);
    }

    None
  }

  /// Get all elements with derived relationships.
  pub(crate) fn get_all_elements(&self) -> Vec<Element> {
    self
      .elements
      .keys()
      .filter_map(|&id| self.build_element(id))
      .collect()
  }

  // --- Windows ---

  pub(crate) fn get_window(&self, id: WindowId) -> Option<&Window> {
    self.windows.get(&id).map(|w| &w.info)
  }

  pub(crate) fn get_window_handle(&self, id: WindowId) -> Option<&Handle> {
    self.windows.get(&id).and_then(|w| w.handle.as_ref())
  }

  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_window_process_id(&self, id: WindowId) -> Option<ProcessId> {
    self.windows.get(&id).map(|w| w.process_id)
  }

  pub(crate) fn get_all_windows(&self) -> impl Iterator<Item = &Window> {
    self.windows.values().map(|w| &w.info)
  }

  pub(crate) fn get_all_window_ids(&self) -> impl Iterator<Item = WindowId> + '_ {
    self.windows.keys().copied()
  }

  // --- Focus/UI ---

  pub(crate) fn get_focused_window(&self) -> Option<WindowId> {
    self.focused_window
  }

  pub(crate) fn get_z_order(&self) -> &[WindowId] {
    &self.z_order
  }

  #[allow(dead_code)] // Part of complete API
  pub(crate) fn get_mouse_position(&self) -> Option<Point> {
    self.mouse_position
  }

  /// Find window at point (uses cached z-order).
  pub(crate) fn get_window_at_point(&self, x: f64, y: f64) -> Option<&Window> {
    let point = Point::new(x, y);
    // z_order is sorted front-to-back, so first match is the topmost window
    for window_id in &self.z_order {
      if let Some(window) = self.windows.get(window_id) {
        if window.info.bounds.contains(point) {
          return Some(&window.info);
        }
      }
    }
    None
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
          .and_then(|eid| self.build_element(eid));
        Some((focused, process.last_selection.clone()))
      })
      .unwrap_or((None, None));

    crate::types::Snapshot {
      windows: self.windows.values().map(|w| w.info.clone()).collect(),
      elements: self.get_all_elements(),
      focused_window: self.focused_window,
      focused_element,
      selection,
      z_order: self.z_order.clone(),
      mouse_position: self.mouse_position,
    }
  }
}
