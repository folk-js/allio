/*!
Element operations for the Registry.

CRUD: `upsert_element`, `update_element`, `remove_element`
Query: element, elements, `find_element`
Element-specific: `set_children`, `set_element_watch`, `take_element_watch`

## Handle-Based Identity

Elements are keyed by Handle (implements Hash via cached `CFHash`, Eq via `CFEqual`).
This gives O(1) lookups with correct collision resolution - no more O(n) fallbacks.
*/

use super::{CachedElement, Registry};
use crate::platform::{Handle, WatchHandle};
use crate::types::{ElementId, Event};

impl Registry {
  /// Insert or update an element by handle.
  ///
  /// If the element exists with the same parent, updates it in place.
  ///
  /// If the element exists with a DIFFERENT parent (platform reparented it),
  /// destroys the old element and its subtree, then creates a new element.
  /// Our API doesn't support reparenting - element IDs have stable parents.
  pub(crate) fn upsert_element(&mut self, elem: CachedElement) -> ElementId {
    let handle = elem.handle.clone();
    let parent_handle = elem.parent_handle.clone();
    let is_root = elem.is_root;

    if let Some(&existing_id) = self.handle_to_id.get(&handle) {
      // Check if parent changed (platform reparented this element)
      let parent_changed = self
        .elements
        .get(&existing_id)
        .is_some_and(|cached| !is_root && cached.parent_handle != parent_handle);

      if parent_changed {
        // Parent changed = element was reparented by platform.
        // Our API doesn't support reparenting, so destroy old element and subtree,
        // then create as new element with new ID.
        self.remove_element(existing_id);
        // Fall through to create new element below
      } else {
        // Same parent - just update in place
        let mut fresh_elem = elem;
        fresh_elem.id = existing_id;
        self.update_element(existing_id, fresh_elem);
        return existing_id;
      }
    }

    // Create new element (either first time, or after reparent-destroy)
    let element_id = elem.id;

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

  /// Update element data. Preserves handle, watch, and updates `last_refreshed`.
  /// Emits `ElementChanged` if semantic data differs.
  pub(crate) fn update_element(&mut self, id: ElementId, mut new_elem: CachedElement) {
    let Some(old_elem) = self.elements.get_mut(&id) else {
      return;
    };

    let changed = old_elem != &new_elem;

    // Preserve metadata from old entry
    new_elem.handle = old_elem.handle.clone();
    new_elem.watch = old_elem.watch.take();
    new_elem.last_refreshed = std::time::Instant::now();

    *old_elem = new_elem;

    if changed {
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
  pub(crate) fn element(&self, id: ElementId) -> Option<&CachedElement> {
    self.elements.get(&id)
  }

  /// Iterate over all element entries.
  pub(crate) fn elements(&self) -> impl Iterator<Item = (ElementId, &CachedElement)> {
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

    // Clear children from waiting_for_parent to prevent orphan resolution
    // from trying to re-link them to a different parent later.
    for &child_id in &valid_children {
      if let Some(elem) = self.elements.get(&child_id) {
        if let Some(ref parent_handle) = elem.parent_handle {
          if let Some(waiting) = self.waiting_for_parent.get_mut(parent_handle) {
            waiting.retain(|&wid| wid != child_id);
            if waiting.is_empty() {
              self.waiting_for_parent.remove(parent_handle);
            }
          }
        }
      }
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
