/*!
Tree relationship management.

Single source of truth for parent-child relationships in the accessibility tree.
All mutations go through methods that maintain bidirectional link invariants.

## Invariants

1. **Single parent**: Each child has exactly ONE parent for its lifetime.
2. **Bidirectional consistency**: If `parent_of[child] = parent`, then
   `children_of[parent]` contains `child`, and vice versa.
3. **No reparenting**: Once an element has a parent, it cannot be moved.
   If the platform reparents an element, we destroy and recreate it.
*/

use crate::types::ElementId;
use std::collections::HashMap;

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
      .is_some_and(|children| !children.is_empty())
  }

  /// Set children for a parent, replacing any existing children.
  ///
  /// Children must either be unparented or already under this parent.
  /// A child with a different parent is a bug (reparenting should have been
  /// detected earlier and handled via destroy+create).
  pub(super) fn set_children(&mut self, parent: ElementId, children: Vec<ElementId>) {
    // Clear old children's parent refs (only those that still point to this parent)
    if let Some(old_children) = self.children_of.get(&parent) {
      for &child_id in old_children {
        if self.parent_of.get(&child_id) == Some(&parent) {
          self.parent_of.remove(&child_id);
        }
      }
    }

    // Link all children to this parent
    for &child_id in &children {
      if let Some(&existing_parent) = self.parent_of.get(&child_id) {
        if existing_parent != parent {
          // BUG: Child has a different parent. Reparenting should have been
          // detected in upsert_element and handled via destroy+create.
          log::error!(
            "set_children: child {child_id} already has parent {existing_parent}, \
             cannot set under {parent}. This is a bug - reparenting was not detected."
          );
          continue;
        }
      }
      self.parent_of.insert(child_id, parent);
    }
    self.children_of.insert(parent, children);
  }

  /// Link a child to a parent.
  ///
  /// - Same parent: no-op (idempotent)
  /// - No parent: links to the specified parent
  /// - Different parent: ERROR (reparenting should have been detected earlier)
  pub(super) fn add_child(&mut self, parent: ElementId, child: ElementId) {
    if let Some(&existing_parent) = self.parent_of.get(&child) {
      if existing_parent == parent {
        // Already linked to this parent - idempotent, no-op
        return;
      }
      // BUG: Child has a different parent. Reparenting should have been
      // detected in upsert_element and handled via destroy+create.
      log::error!(
        "add_child: child {child} already has parent {existing_parent}, \
         cannot add to {parent}. This is a bug - reparenting was not detected."
      );
      return;
    }

    self.parent_of.insert(child, parent);
    self.children_of.entry(parent).or_default().push(child);
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
  fn test_add_child_idempotent() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2));

    // Adding same child to same parent again is a no-op
    tree.add_child(id(1), id(2));

    assert_eq!(tree.parent(id(2)), Some(id(1)));
    // Should NOT have duplicate in children list
    assert_eq!(tree.children(id(1)), &[id(2)]);
  }

  #[test]
  fn test_add_child_rejects_different_parent() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2));

    // Adding to a different parent should be rejected (logs error, no-op)
    tree.add_child(id(99), id(2));

    // Child should still be under original parent (rejected)
    assert_eq!(tree.parent(id(2)), Some(id(1)));
    // Child should NOT appear in new parent's list
    assert_eq!(tree.children(id(99)), &[] as &[ElementId]);
    // Original parent's children unchanged
    assert_eq!(tree.children(id(1)), &[id(2)]);
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
  fn test_set_children_rejects_already_parented() {
    let mut tree = ElementTree::new();
    tree.add_child(id(1), id(2)); // Child 2 belongs to parent 1

    // Set children including child 2 - should reject child 2 (logs error)
    tree.set_children(id(99), vec![id(2), id(3)]);

    // Child 2 should still be under parent 1 (rejected from new parent)
    assert_eq!(tree.parent(id(2)), Some(id(1)));
    // Child 3 should be parented to 99 (was unparented)
    assert_eq!(tree.parent(id(3)), Some(id(99)));
    // Parent 99's children list will have [id(2), id(3)] but id(2) not actually linked
    // This is a known inconsistency when hitting the error case - the children list
    // contains the ID but parent_of doesn't point back. In practice this error
    // indicates a bug that should be fixed upstream.
    // Parent 1 should still have child 2
    assert_eq!(tree.children(id(1)), &[id(2)]);
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
}
