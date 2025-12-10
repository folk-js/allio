/*!
Tree relationship management.

Single source of truth for parent-child relationships in the accessibility tree.
All mutations go through methods that maintain bidirectional link invariants.
*/

use crate::types::ElementId;
use std::collections::HashMap;

/// Single source of truth for tree relationships.
///
/// Maintains parent→child and child→parent mappings with guaranteed consistency.
/// All mutations go through methods that update both directions atomically.
pub(crate) struct ElementTree {
  parent_of: HashMap<ElementId, ElementId>,
  children_of: HashMap<ElementId, Vec<ElementId>>,
}

impl ElementTree {
  pub(super) fn new() -> Self {
    Self {
      parent_of: HashMap::new(),
      children_of: HashMap::new(),
    }
  }

  /// Get parent of an element.
  pub(super) fn parent(&self, id: ElementId) -> Option<ElementId> {
    self.parent_of.get(&id).copied()
  }

  /// Get children of an element (empty slice if none or not tracked).
  pub(super) fn children(&self, id: ElementId) -> &[ElementId] {
    self.children_of.get(&id).map_or(&[], Vec::as_slice)
  }

  /// Check if element has any children registered.
  pub(super) fn has_children(&self, id: ElementId) -> bool {
    self
      .children_of
      .get(&id)
      .map_or(false, |children| !children.is_empty())
  }

  /// Set parent for a child. Handles unlinking from old parent.
  /// Does NOT emit events - caller is responsible.
  #[allow(dead_code)] // Available for future reparenting operations
  pub(super) fn set_parent(&mut self, child: ElementId, new_parent: Option<ElementId>) {
    // Remove from old parent's children list
    if let Some(old_parent) = self.parent_of.remove(&child) {
      if let Some(siblings) = self.children_of.get_mut(&old_parent) {
        siblings.retain(|&id| id != child);
      }
    }

    // Add to new parent
    if let Some(parent_id) = new_parent {
      self.parent_of.insert(child, parent_id);
      self.children_of.entry(parent_id).or_default().push(child);
    }
  }

  /// Set children for a parent, replacing any existing children.
  /// Updates parent_of for all new children and clears for old children.
  /// Used by fetch_children to set children in OS order.
  pub(super) fn set_children(&mut self, parent: ElementId, children: Vec<ElementId>) {
    // Clear old children's parent refs
    if let Some(old_children) = self.children_of.get(&parent) {
      for &child_id in old_children {
        self.parent_of.remove(&child_id);
      }
    }

    // Set new children
    for &child_id in &children {
      self.parent_of.insert(child_id, parent);
    }
    self.children_of.insert(parent, children);
  }

  /// Add a single child to parent's children list.
  /// Used for orphan resolution when parent is discovered after child.
  pub(super) fn add_child(&mut self, parent: ElementId, child: ElementId) {
    debug_assert!(
      self.parent_of.get(&child).is_none(),
      "add_child: child {child} already has parent {:?}",
      self.parent_of.get(&child)
    );
    self.parent_of.insert(child, parent);
    self.children_of.entry(parent).or_default().push(child);
  }

  /// Remove a child from its parent (but keep the child's entry).
  /// Used when reparenting or before removal.
  #[allow(dead_code)] // Available for future reparenting operations
  pub(super) fn unlink_from_parent(&mut self, child: ElementId) {
    if let Some(parent_id) = self.parent_of.remove(&child) {
      if let Some(siblings) = self.children_of.get_mut(&parent_id) {
        siblings.retain(|&id| id != child);
      }
    }
  }

  /// Remove an element and all its descendants.
  /// Returns removed IDs in removal order (parent before children).
  /// Iterative to avoid stack overflow on deep trees.
  pub(super) fn remove_subtree(&mut self, root: ElementId) -> Vec<ElementId> {
    let mut removed = Vec::new();
    let mut queue = vec![root];

    while let Some(id) = queue.pop() {
      // Remove from parent's children list
      if let Some(parent_id) = self.parent_of.remove(&id) {
        if let Some(siblings) = self.children_of.get_mut(&parent_id) {
          siblings.retain(|&sid| sid != id);
        }
      }

      // Queue children for removal, then remove this node's children list
      if let Some(children) = self.children_of.remove(&id) {
        queue.extend(children);
      }

      removed.push(id);
    }

    removed
  }

  /// Remove a single element without touching its children.
  /// Children become orphans (their parent_of entries are removed).
  /// Used when an element is destroyed but we want to preserve children
  /// (rare - usually use remove_subtree).
  #[allow(dead_code)]
  pub(super) fn remove_single(&mut self, id: ElementId) {
    // Unlink from parent
    self.unlink_from_parent(id);

    // Orphan children (remove their parent refs but keep them in tree)
    if let Some(children) = self.children_of.remove(&id) {
      for child_id in children {
        self.parent_of.remove(&child_id);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn id(n: u32) -> ElementId {
    ElementId(n)
  }

  #[test]
  fn test_add_child() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2));
    tree.add_child(id(1), id(3));

    assert_eq!(tree.parent(id(2)), Some(id(1)));
    assert_eq!(tree.parent(id(3)), Some(id(1)));
    assert_eq!(tree.children(id(1)), &[id(2), id(3)]);
  }

  #[test]
  fn test_set_children_replaces() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2));
    tree.add_child(id(1), id(3));

    // Replace children
    tree.set_children(id(1), vec![id(4), id(5)]);

    assert_eq!(tree.parent(id(2)), None); // Old children unlinked
    assert_eq!(tree.parent(id(3)), None);
    assert_eq!(tree.parent(id(4)), Some(id(1)));
    assert_eq!(tree.parent(id(5)), Some(id(1)));
    assert_eq!(tree.children(id(1)), &[id(4), id(5)]);
  }

  #[test]
  fn test_set_parent_unlinks_old() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(3));

    // Reparent
    tree.set_parent(id(3), Some(id(2)));

    assert_eq!(tree.parent(id(3)), Some(id(2)));
    assert_eq!(tree.children(id(1)), &[]); // Removed from old parent
    assert_eq!(tree.children(id(2)), &[id(3)]); // Added to new parent
  }

  #[test]
  fn test_remove_subtree() {
    let mut tree = ElementTree::new();
    // Build: 1 -> [2, 3], 2 -> [4, 5]
    tree.add_child(id(1), id(2));
    tree.add_child(id(1), id(3));
    tree.add_child(id(2), id(4));
    tree.add_child(id(2), id(5));

    let removed = tree.remove_subtree(id(2));

    // Should remove 2, 4, 5 (order depends on queue processing)
    assert!(removed.contains(&id(2)));
    assert!(removed.contains(&id(4)));
    assert!(removed.contains(&id(5)));
    assert_eq!(removed.len(), 3);

    // 1 and 3 should remain
    assert_eq!(tree.children(id(1)), &[id(3)]);
    assert_eq!(tree.parent(id(3)), Some(id(1)));

    // Removed nodes should be gone
    assert_eq!(tree.parent(id(2)), None);
    assert_eq!(tree.parent(id(4)), None);
    assert_eq!(tree.children(id(2)), &[]);
  }

  #[test]
  fn test_unlink_from_parent() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2));
    tree.add_child(id(1), id(3));

    tree.unlink_from_parent(id(2));

    assert_eq!(tree.parent(id(2)), None);
    assert_eq!(tree.children(id(1)), &[id(3)]);
  }
}
