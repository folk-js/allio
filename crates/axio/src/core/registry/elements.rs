/*!
Element operations for the Registry.

CRUD: upsert_element, update_element, remove_element
Query: element, elements, find_element
Element-specific: set_children, set_element_watch, take_element_watch

## Handle-Based Identity

Elements are keyed by Handle (implements Hash via cached CFHash, Eq via CFEqual).
This gives O(1) lookups with correct collision resolution - no more O(n) fallbacks.
*/

use super::{ElementData, ElementEntry, Registry};
use crate::platform::{Handle, WatchHandle};
use crate::types::{ElementId, Event};

// ============================================================================
// Element CRUD
// ============================================================================

impl Registry {
  /// Insert or update an element by handle.
  ///
  /// - If handle matches existing element: updates data, returns existing ElementId
  /// - If new: inserts, maintains indexes, resolves orphans, emits ElementAdded
  ///
  /// Handle identity (via CFEqual) ensures the same AXUIElement always maps to the
  /// same ElementId. An element's parent is always in the same window by construction
  /// (accessibility trees are window-scoped).
  ///
  /// This is a pure data operation. Call `Axio::ensure_watched` after to set up OS sync.
  pub(crate) fn upsert_element(&mut self, elem: ElementEntry) -> ElementId {
    let handle = elem.handle.clone();
    let parent_handle = elem.parent_handle.clone();
    let is_root = elem.data.is_root;

    // Element already exists - update data from fresh fetch
    if let Some(&existing_id) = self.handle_to_id.get(&handle) {
      // Update the element data with fresh values (preserves ID)
      let mut fresh_data = elem.data;
      fresh_data.id = existing_id; // Keep the existing ID
      self.update_element(existing_id, fresh_data);
      return existing_id;
    }

    let element_id = elem.data.id;

    // Insert into handle index
    self.handle_to_id.insert(handle.clone(), element_id);

    // Insert element data into primary collection
    self.elements.insert(element_id, elem);

    // Link to parent via tree if parent handle exists in cache
    if !is_root {
      if let Some(ref ph) = parent_handle {
        if let Some(&parent_id) = self.handle_to_id.get(ph) {
          // Parent exists - link immediately
          self.tree.add_child(parent_id, element_id);
          self.emit_element_changed(parent_id);
        } else {
          // Orphan: parent not loaded yet, queue for later
          self
            .waiting_for_parent
            .entry(ph.clone())
            .or_default()
            .push(element_id);
        }
      }
    }

    // Resolve any orphans waiting for this element
    if let Some(orphans) = self.waiting_for_parent.remove(&handle) {
      for orphan_id in orphans {
        self.tree.add_child(element_id, orphan_id);
        self.emit_element_changed(orphan_id);
      }
    }

    self.emit_element_added(element_id);
    element_id
  }

  /// Update element data from fresh platform fetch.
  /// Only updates the data fields (not relationships). Emits ElementChanged if data differs.
  /// No-op if element doesn't exist.
  pub(crate) fn update_element(&mut self, id: ElementId, new_data: ElementData) {
    let Some(elem) = self.elements.get_mut(&id) else {
      return;
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

    // Always update refresh timestamp since we fetched from OS
    elem.mark_refreshed();

    if changed {
      elem.data = new_data;
      self.emit_element_changed(id);
    }
  }

  /// Remove an element and cascade to all descendants.
  pub(crate) fn remove_element(&mut self, id: ElementId) {
    // Remove subtree from tree structure, get all removed IDs
    let removed_ids = self.tree.remove_subtree(id);

    for removed_id in removed_ids {
      self.remove_element_internal(removed_id);
    }
  }

  /// Remove element data and indexes (internal helper).
  fn remove_element_internal(&mut self, id: ElementId) {
    let Some(mut elem) = self.elements.remove(&id) else {
      return;
    };

    // Clean handle index
    self.handle_to_id.remove(&elem.handle);

    // Clean waiting_for_parent
    if let Some(ref ph) = elem.parent_handle {
      if let Some(waiting) = self.waiting_for_parent.get_mut(ph) {
        waiting.retain(|&wid| wid != id);
        if waiting.is_empty() {
          self.waiting_for_parent.remove(ph);
        }
      }
    }
    self.waiting_for_parent.remove(&elem.handle);

    // Drop watch (RAII cleanup)
    elem.watch.take();

    self.emit(Event::ElementRemoved { element_id: id });
  }
}

// ============================================================================
// Element Queries
// ============================================================================

impl Registry {
  /// Get element entry by ID.
  pub(crate) fn element(&self, id: ElementId) -> Option<&ElementEntry> {
    self.elements.get(&id)
  }

  /// Iterate over all element entries.
  pub(crate) fn elements(&self) -> impl Iterator<Item = (ElementId, &ElementEntry)> {
    self.elements.iter().map(|(id, e)| (*id, e))
  }

  /// Find element by handle. O(1) with correct collision resolution via CFEqual.
  pub(crate) fn find_element(&self, handle: &Handle) -> Option<ElementId> {
    self.handle_to_id.get(handle).copied()
  }
}

// ============================================================================
// Element-Specific Operations
// ============================================================================

impl Registry {
  /// Set children for an element in OS order.
  /// Updates tree relationships. Emits ElementChanged if different.
  /// Filters to only existing elements to prevent dangling refs.
  /// No-op if element doesn't exist.
  pub(crate) fn set_children(&mut self, id: ElementId, children: Vec<ElementId>) {
    if !self.elements.contains_key(&id) {
      return;
    }

    // Filter to only existing elements (prevents dangling refs)
    let valid_children: Vec<ElementId> = children
      .into_iter()
      .filter(|&cid| self.elements.contains_key(&cid))
      .collect();

    let old_children = self.tree.children(id);
    if old_children == valid_children {
      return;
    }

    self.tree.set_children(id, valid_children);
    self.emit_element_changed(id);
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
