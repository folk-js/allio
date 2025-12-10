# Tree Relationships Refactor

This document outlines the plan to fix flaky tree relationship management in Axio by establishing a single source of truth for parent-child relationships.

## Problems Being Addressed

From `TREE_DESIGN.md`:

1. **Children ordering is non-deterministic** - Children are appended in registration order, not OS order
2. **Orphan resolution order is non-deterministic** - Linked in Vec order (registration order)
3. **`children` field can reference non-existent elements** - Registration failures leave dangling refs
4. **Bidirectional links must stay consistent** - Error-prone manual maintenance of `parent_id` and `children`
5. **Cascade removal during iteration** - Works but requires clone workaround

## Core Insight

The root cause is that relationship data is duplicated:

- `parent_id` lives on `Element`
- `children` lives on `Element`
- Both must be kept in sync manually

**Solution:** Extract relationships into a dedicated `ElementTree` struct that encapsulates all invariants. `Element` becomes pure data; relationships are derived when needed (e.g., at event emission).

---

## Implementation Plan

### Phase 1: Extract TreeRelationships

Create a new struct that owns all parent-child relationships with encapsulated invariants.

**New file: `crates/axio/src/core/tree.rs`**

```rust
use crate::types::ElementId;
use std::collections::HashMap;

/// Single source of truth for tree relationships.
/// All mutations go through methods that maintain invariants.
pub(crate) struct TreeRelationships {
    parent_of: HashMap<ElementId, ElementId>,
    children_of: HashMap<ElementId, Vec<ElementId>>,
}

impl TreeRelationships {
    pub fn new() -> Self {
        Self {
            parent_of: HashMap::new(),
            children_of: HashMap::new(),
        }
    }

    /// Get parent of an element.
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.parent_of.get(&id).copied()
    }

    /// Get children of an element (empty slice if none).
    pub fn children(&self, id: ElementId) -> &[ElementId] {
        self.children_of.get(&id).map_or(&[], |v| v.as_slice())
    }

    /// Set parent for a child. Handles unlinking from old parent.
    /// Does NOT emit events - caller is responsible.
    pub fn set_parent(&mut self, child: ElementId, new_parent: Option<ElementId>) {
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
    /// Updates parent_of for all children. Used by fetch_children.
    pub fn set_children(&mut self, parent: ElementId, children: Vec<ElementId>) {
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

    /// Add a single child to parent (maintains order).
    /// Used for orphan resolution.
    pub fn add_child(&mut self, parent: ElementId, child: ElementId) {
        debug_assert!(
            self.parent_of.get(&child).is_none(),
            "Child {child} already has parent {:?}",
            self.parent_of.get(&child)
        );
        self.parent_of.insert(child, parent);
        self.children_of.entry(parent).or_default().push(child);
    }

    /// Remove an element and all descendants. Returns removed IDs in removal order.
    /// Iterative to avoid stack overflow on deep trees.
    pub fn remove_subtree(&mut self, root: ElementId) -> Vec<ElementId> {
        let mut removed = Vec::new();
        let mut queue = vec![root];

        while let Some(id) = queue.pop() {
            // Remove from parent's children
            if let Some(parent_id) = self.parent_of.remove(&id) {
                if let Some(siblings) = self.children_of.get_mut(&parent_id) {
                    siblings.retain(|&sid| sid != id);
                }
            }

            // Queue children for removal, then remove children list
            if let Some(children) = self.children_of.remove(&id) {
                queue.extend(children);
            }

            removed.push(id);
        }

        removed
    }

    /// Remove a single element (not its children).
    /// Children become orphans (parent_of entries removed).
    pub fn remove_single(&mut self, id: ElementId) {
        // Unlink from parent
        if let Some(parent_id) = self.parent_of.remove(&id) {
            if let Some(siblings) = self.children_of.get_mut(&parent_id) {
                siblings.retain(|&sid| sid != id);
            }
        }

        // Orphan children (remove their parent refs)
        if let Some(children) = self.children_of.remove(&id) {
            for child_id in children {
                self.parent_of.remove(&child_id);
            }
        }
    }
}
```

### Phase 2: Create ElementData (Relationship-Free)

Separate the pure element data from relationships.

**Modify `ElementState` in `state.rs`:**

```rust
/// Per-element state (internal storage).
pub(crate) struct ElementState {
    pub(crate) data: ElementData,
    pub(crate) handle: Handle,
    pub(crate) hash: u64,
    pub(crate) parent_hash: Option<u64>,  // For orphan resolution
    pub(crate) watch: Option<WatchHandle>,
}

/// Pure element data without relationships.
pub(crate) struct ElementData {
    pub id: ElementId,
    pub window_id: WindowId,
    pub pid: ProcessId,
    pub is_root: bool,
    // ... all other fields from Element EXCEPT parent_id and children
    pub role: Role,
    pub platform_role: String,
    pub label: Option<String>,
    pub description: Option<String>,
    // ... etc
}
```

### Phase 3: Update State to Use TreeRelationships

**In `state.rs`:**

```rust
pub(crate) struct State {
    // Event emission
    events_tx: Sender<Event>,

    // Primary collections
    processes: HashMap<ProcessId, ProcessState>,
    windows: HashMap<WindowId, WindowState>,
    elements: HashMap<ElementId, ElementState>,

    // Tree structure (single source of truth)
    tree: TreeRelationships,

    // Indexes
    element_to_window: HashMap<ElementId, WindowId>,
    hash_to_element: HashMap<u64, ElementId>,
    waiting_for_parent: HashMap<u64, Vec<ElementId>>,

    // ... other state
}
```

### Phase 4: Build Element at Emission Time

`Element` remains the public API type, but it's now **derived** rather than stored:

```rust
impl Registry {
    /// Build an Element snapshot for API/events.
    pub(crate) fn build_element(&self, id: ElementId) -> Option<Element> {
        let elem = self.elements.get(&id)?;
        Some(Element {
            id,
            window_id: elem.data.window_id,
            pid: elem.data.pid,
            is_root: elem.data.is_root,
            // Derived from TreeRelationships:
            parent_id: if elem.data.is_root { None } else { self.tree.parent(id) },
            children: Some(self.tree.children(id).to_vec()),
            // ... rest from elem.data
            role: elem.data.role,
            platform_role: elem.data.platform_role.clone(),
            label: elem.data.label.clone(),
            // ... etc
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
```

### Phase 5: Update Element Operations

**Registration (`get_or_insert_element`):**

```rust
pub(crate) fn get_or_insert_element(&mut self, elem: ElementState) -> (ElementId, bool) {
    let hash = elem.hash;
    let parent_hash = elem.parent_hash;
    let is_root = elem.data.is_root;

    // Dedup check
    if let Some(&existing_id) = self.hash_to_element.get(&hash) {
        if self.elements.contains_key(&existing_id) {
            let has_watch = self.elements.get(&existing_id)
                .map_or(false, |e| e.watch.is_some());
            return (existing_id, has_watch);
        }
    }

    let element_id = elem.data.id;
    let window_id = elem.data.window_id;

    // Insert element data
    self.elements.insert(element_id, elem);
    self.element_to_window.insert(element_id, window_id);
    self.hash_to_element.insert(hash, element_id);

    // Link to parent if known
    if !is_root {
        if let Some(ref ph) = parent_hash {
            if let Some(&parent_id) = self.hash_to_element.get(ph) {
                self.tree.add_child(parent_id, element_id);
            } else {
                // Orphan: queue for later resolution
                self.waiting_for_parent.entry(*ph).or_default().push(element_id);
            }
        }
    }

    // Resolve waiting orphans
    if let Some(orphans) = self.waiting_for_parent.remove(&hash) {
        for orphan_id in orphans {
            self.tree.add_child(element_id, orphan_id);
            self.emit_element_changed(orphan_id);  // Orphan now has parent
        }
    }

    self.emit_element_added(element_id);
    (element_id, false)
}
```

**Setting children (`set_element_children`):**

```rust
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

    let old_children = self.tree.children(id).to_vec();
    if old_children == valid_children {
        return (true, false);
    }

    self.tree.set_children(id, valid_children);
    self.emit_element_changed(id);
    (true, true)
}
```

**Removal (`remove_element`):**

```rust
pub(crate) fn remove_element(&mut self, id: ElementId) {
    // Remove subtree from relationships, get all removed IDs
    let removed_ids = self.tree.remove_subtree(id);

    for removed_id in removed_ids {
        // Clean up element data
        if let Some(elem) = self.elements.remove(&removed_id) {
            self.hash_to_element.remove(&elem.hash);
            self.element_to_window.remove(&removed_id);

            // Clean orphan queue
            if let Some(ref ph) = elem.parent_hash {
                if let Some(waiting) = self.waiting_for_parent.get_mut(ph) {
                    waiting.retain(|&wid| wid != removed_id);
                }
            }
            self.waiting_for_parent.remove(&elem.hash);
        }

        self.emit(Event::ElementRemoved { element_id: removed_id });
    }
}
```

### Phase 6: Update Queries

**`get_element` now builds the snapshot:**

```rust
pub(crate) fn get_element(&self, id: ElementId) -> Option<Element> {
    self.build_element(id)
}
```

---

## Migration Checklist

- [x] Create `crates/axio/src/core/tree.rs` with `TreeRelationships`
- [x] Add `mod tree;` to `core/mod.rs`
- [x] Create `ElementData` struct (fields from `Element` minus `parent_id`/`children`)
- [x] Update `ElementState` to use `ElementData`
- [x] Add `tree: TreeRelationships` to `State`
- [x] Implement `build_element()` method
- [x] Update `get_or_insert_element` to use `tree`
- [x] Update `set_element_children` to use `tree`
- [x] Update `remove_element` to use `tree.remove_subtree()`
- [x] Update orphan resolution to use `tree.add_child()`
- [x] Update all query methods that return `Element`
- [x] Update `element_ops.rs` to build `ElementData` instead of `Element`
- [x] Run tests, fix any compilation errors
- [x] Verify events still emit correct `parent_id`/`children`

---

## Benefits

1. **Single source of truth** - Relationships only exist in `TreeRelationships`
2. **Encapsulated invariants** - Can't break bidirectional links from outside
3. **No dangling refs** - `set_element_children` validates existence
4. **Clean removal** - Iterative, no clones needed
5. **Orphan handling unchanged** - Still use hash-based queue, but linking goes through `tree`

---

## Future: Derived Views

Once the core refactor is stable, we can add simplified/contracted tree views.

### Concept

Views are **derived representations** of the tree that:

- Prune generic leaf elements
- Collapse single-child generic containers
- Stay synchronized with the source tree via events

### Potential Approach

```rust
/// A simplified view that prunes/contracts the full tree.
pub struct SimplifiedView {
    /// Maps full-tree ID → simplified visibility
    visibility: HashMap<ElementId, Visibility>,
    /// Maps collapsed elements to what they collapse into
    collapsed_into: HashMap<ElementId, ElementId>,
}

enum Visibility {
    Visible,           // Shown in simplified tree
    Pruned,            // Removed (generic leaf)
    Collapsed(ElementId), // Collapsed into another element
}

impl SimplifiedView {
    /// Called when State emits ElementAdded
    pub fn on_element_added(&mut self, tree: &TreeRelationships, elem: &ElementData) {
        // Determine visibility based on role, parent, children
    }

    /// Get simplified parent (skipping collapsed containers)
    pub fn simplified_parent(&self, tree: &TreeRelationships, id: ElementId) -> Option<ElementId> {
        // Walk up tree, skipping Collapsed elements
    }
}
```

### Open Questions for Views

1. Should views emit their own events, or augment State events?
2. How to handle view updates when tree structure changes?
3. Should clients request "simplified" vs "full" elements, or always get both?
4. Performance: eager vs lazy view computation?

These will be addressed when we implement views.

---

## Completed: Naming Cleanup

Renames applied for clarity and cross-platform consistency:

| Old            | New            | Rationale                                                             |
| -------------- | -------------- | --------------------------------------------------------------------- |
| `State`        | `Registry`     | Matches actual usage (doc comments already say "registry lookups")    |
| `AXElement`    | `Element`      | `AX` prefix is macOS-specific; core types should be platform-agnostic |
| `AXWindow`     | `Window`       | Same as above                                                         |
| `ElementState` | `ElementEntry` | It's a cache entry in the registry, not "state"                       |
| `ProcessState` | `ProcessEntry` | Consistency with above                                                |
| `WindowState`  | `WindowEntry`  | Consistency with above                                                |
| `depth_order`  | `z_order`      | Industry-standard term (doc comment already says "z-order")           |
| `TreeRelationships` | `ElementTree` | More descriptive name                                            |

**Kept as-is:** `get_*`/`fetch_*` convention, `watch`/`unwatch`, `Role`/`Action`, `Handle`, branded IDs.

**Note:** To avoid collisions with browser types, TypeScript imports use the `AX` namespace, e.g. `AX.Element`, `AX.Window`, `AX.Event`.
