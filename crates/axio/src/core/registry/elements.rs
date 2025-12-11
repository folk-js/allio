/*!
Element operations for the Registry.

CRUD: upsert_element, update_element, remove_element
Query: element, elements
Element-specific: set_children, set_element_watch, take_element_watch
Indexes: find_by_hash, find_by_hash_in_window
*/

use super::{ElementData, ElementEntry, Registry};
use crate::platform::WatchHandle;
use crate::types::{ElementId, Event, ProcessId, WindowId};

// ============================================================================
// Element CRUD
// ============================================================================

impl Registry {
  /// Insert or update an element by hash.
  ///
  /// - If hash exists in same window: updates data, returns existing ElementId
  /// - If new: inserts, maintains indexes, resolves orphans, emits ElementAdded
  ///
  /// This is a pure data operation. Call `Axio::ensure_watched` after to set up OS sync.
  pub(crate) fn upsert_element(&mut self, elem: ElementEntry) -> ElementId {
    let hash = elem.hash;
    let parent_hash = elem.parent_hash;
    let is_root = elem.data.is_root;
    let window_id = elem.data.window_id;

    // Element already exists in this window - update data from fresh fetch
    if let Some(existing_id) = self.find_by_hash_in_window(hash, window_id) {
      // Update the element data with fresh values (preserves ID)
      let mut fresh_data = elem.data;
      fresh_data.id = existing_id; // Keep the existing ID
      self.update_element(existing_id, fresh_data);
      return existing_id;
    }

    let element_id = elem.data.id;

    // Insert element data into primary collection and hash index
    self.elements.insert(element_id, elem);
    // Note: This overwrites the hash_to_element entry. For cross-window hash sharing,
    // the most recently registered element "wins" the hash lookup. find_by_hash
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

// ============================================================================
// Element Hash Indexes
// ============================================================================

impl Registry {
  /// Find element by hash, optionally filtering by process ID.
  ///
  /// When `pid` is provided, ensures the returned element belongs to that process.
  /// This handles hash collisions across windows/processes where the hash_to_element
  /// fast-path might point to the wrong element.
  pub(crate) fn find_by_hash(&self, hash: u64, pid: Option<ProcessId>) -> Option<ElementId> {
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

  /// Find element by hash within a specific window.
  ///
  /// Used for deduplication: same hash in different windows = different elements.
  pub(crate) fn find_by_hash_in_window(&self, hash: u64, window_id: WindowId) -> Option<ElementId> {
    // Fast path: direct lookup
    if let Some(&element_id) = self.hash_to_element.get(&hash) {
      if let Some(elem) = self.elements.get(&element_id) {
        if elem.data.window_id == window_id {
          return Some(element_id);
        }
      }
    }

    // Slow path: hash collision, search all elements in this window
    self
      .elements
      .iter()
      .find(|(_, e)| e.hash == hash && e.data.window_id == window_id)
      .map(|(id, _)| *id)
  }
}
