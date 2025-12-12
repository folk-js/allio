/*!
Element operations for the Registry.

CRUD: `upsert_element`, `update_element`, `remove_element`
Query: element, elements, `find_element`
Element-specific: `set_children`, `set_element_watch`, `take_element_watch`

## Handle-Based Identity

Elements are keyed by Handle (implements Hash via cached `CFHash`, Eq via `CFEqual`).
This gives O(1) lookups with correct collision resolution - no more O(n) fallbacks.
*/

use super::{ElementData, ElementEntry, Registry};
use crate::platform::{Handle, WatchHandle};
use crate::types::{ElementId, Event};

impl Registry {
  /// Insert or update an element by handle.
  pub(crate) fn upsert_element(&mut self, elem: ElementEntry) -> ElementId {
    let handle = elem.handle.clone();
    let parent_handle = elem.parent_handle.clone();
    let is_root = elem.data.is_root;

    if let Some(&existing_id) = self.handle_to_id.get(&handle) {
      let mut fresh_data = elem.data;
      fresh_data.id = existing_id;
      self.update_element(existing_id, fresh_data);
      return existing_id;
    }

    let element_id = elem.data.id;

    self.handle_to_id.insert(handle.clone(), element_id);
    self.elements.insert(element_id, elem);

    // Link to parent if already in cache, otherwise queue as orphan
    if !is_root {
      if let Some(ref ph) = parent_handle {
        if let Some(&parent_id) = self.handle_to_id.get(ph) {
          self.tree.add_child(parent_id, element_id);
          self.emit_element_changed(parent_id);
        } else {
          self
            .waiting_for_parent
            .entry(ph.clone())
            .or_default()
            .push(element_id);
        }
      }
    }

    // Resolve orphans waiting for this element
    if let Some(orphans) = self.waiting_for_parent.remove(&handle) {
      for orphan_id in orphans {
        self.tree.add_child(element_id, orphan_id);
        self.emit_element_changed(orphan_id);
      }
    }

    self.emit_element_added(element_id);
    element_id
  }

  /// Update element data. Emits `ElementChanged` if data differs.
  pub(crate) fn update_element(&mut self, id: ElementId, new_data: ElementData) {
    let Some(elem) = self.elements.get_mut(&id) else {
      return;
    };

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

    elem.mark_refreshed();

    if changed {
      elem.data = new_data;
      self.emit_element_changed(id);
    }
  }

  /// Remove an element and all descendants.
  pub(crate) fn remove_element(&mut self, id: ElementId) {
    let removed_ids = self.tree.remove_subtree(id);

    for removed_id in removed_ids {
      self.remove_element_internal(removed_id);
    }
  }

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
    elem.watch.take();

    self.emit(Event::ElementRemoved { element_id: id });
  }

  /// Get element entry by ID.
  pub(crate) fn element(&self, id: ElementId) -> Option<&ElementEntry> {
    self.elements.get(&id)
  }

  /// Iterate over all element entries.
  pub(crate) fn elements(&self) -> impl Iterator<Item = (ElementId, &ElementEntry)> {
    self.elements.iter().map(|(id, e)| (*id, e))
  }

  /// Find element by handle.
  pub(crate) fn find_element(&self, handle: &Handle) -> Option<ElementId> {
    self.handle_to_id.get(handle).copied()
  }

  /// Set children for an element. Emits `ElementChanged` if different.
  pub(crate) fn set_children(&mut self, id: ElementId, children: Vec<ElementId>) {
    if !self.elements.contains_key(&id) {
      return;
    }

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

  /// Take watch handle from element.
  pub(crate) fn take_element_watch(&mut self, id: ElementId) -> Option<WatchHandle> {
    self.elements.get_mut(&id).and_then(|e| e.watch.take())
  }
}
