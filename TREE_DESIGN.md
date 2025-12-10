# Axio Tree/Element Management Design

This document describes the challenges of managing accessibility tree state in Axio, intended for research into principled solutions.

## Problem Statement

Axio maintains a **partial, incrementally-discovered cache** of the macOS accessibility tree. This cache must:

1. Support random-order node discovery (children may arrive before parents)
2. Deduplicate nodes using OS-provided hashes
3. Handle node removal with cascading effects to children
4. Eventually support "views" (pruned/contracted versions of the tree)

The current implementation is home-grown and fragile. We're looking for established data structures or algorithms that could provide a more robust foundation.

## System Constraints

### Performance

- Hit testing happens tens to hundreds of times per frame
- Cannot make IPC calls for identity (must use cached hash)
- Lock contention must be minimal

### Identity Model

- **OS Identity:** `CFHash(AXUIElement)` - a u64 hash from the OS
  - Stable for element lifetime (observed, not guaranteed by Apple)
  - No collision handling currently
  - Used for deduplication: same hash = same element
- **Client Identity:** `ElementId` - a u32 we assign
  - Issued on first registration
  - **NOT stable across removal** - when a parent is removed, all children are cascade-removed and their IDs become invalid
  - Sent to clients over RPC (must fit in JS number)

### Structure

- One tree per window (multiple independent trees)
- Trees are partial - we only have nodes that have been explicitly fetched
- Nodes can be "orphans" - their parent exists in OS but hasn't been fetched yet

## Current Data Model

```rust
struct State {
  // Primary storage
  elements: HashMap<ElementId, ElementState>,

  // Indexes
  hash_to_element: HashMap<u64, ElementId>,
  element_to_window: HashMap<ElementId, WindowId>,
  waiting_for_parent: HashMap<u64, Vec<ElementId>>,  // orphan queue

  // ... other state
}

struct ElementState {
  element: AXElement,      // The data we expose to clients
  handle: Handle,          // OS reference for operations
  hash: u64,               // For deduplication
  parent_hash: Option<u64>, // For orphan resolution
  watch: Option<WatchHandle>,
}

struct AXElement {
  id: ElementId,
  window_id: WindowId,
  is_root: bool,
  parent_id: Option<ElementId>,      // None if orphan or root
  children: Option<Vec<ElementId>>,  // None if not yet fetched
  // ... other fields (role, bounds, value, etc.)
}
```

## Current Operations

### Node Registration

When an element is fetched from OS:

1. Compute hash from OS handle
2. If hash exists in cache, return existing ElementId (dedup)
3. Otherwise, create new ElementState with new ElementId
4. If parent_hash is known and parent is registered, link bidirectionally
5. If parent_hash is known but parent not registered, add to orphan queue
6. Check orphan queue for any children waiting for this node's hash

### Node Removal (Cascade)

When an element is destroyed (OS notification) or its window is removed:

1. Remove from all indexes
2. Recursively remove all children (using `children` field)
3. Clean up orphan queue entries
4. Emit removal events for each removed element

### Orphan Resolution

When a node is registered and orphans are waiting for its hash:

1. Link each orphan to the new parent
2. Update orphan's `parent_id`
3. Add orphan to parent's `children` list
4. Emit change events

## Known Problems

### 1. Children Ordering is Non-Deterministic

Children are appended in registration order, not OS tree order:

```rust
children.push(child_id);  // Always appends
```

If you fetch children A, B, C in that order, the list is [A, B, C].
If you fetch C, A, B, the list is [C, A, B].

The OS has a canonical order we're not preserving.

### 2. Orphan Resolution Order is Non-Deterministic

Orphans waiting for a parent are stored in a Vec. When parent arrives, they're linked in Vec order, which is registration order. Combined with issue #1, this means tree structure depends on traversal order.

### 3. `children` Field Can Reference Non-Existent Elements

When `fetch_children` is called, we set the parent's `children` field to the IDs of fetched children. But if a child registration fails (e.g., element destroyed mid-fetch), the ID list may contain dangling references.

### 4. Hash Collisions Not Handled

If two different OS elements produce the same hash (unlikely but possible), the second is silently treated as the first - wrong element data, wrong parent relationships.

### 5. Bidirectional Links Must Stay Consistent

Every mutation must update both `parent_id` and `children`. This is error-prone:

- Adding child: update child's `parent_id`, add to parent's `children`
- Removing child: update parent's `children`, then cascade remove child
- Reparenting: update old parent, update new parent, update child

### 6. Cascade Removal During Iteration

When removing a node, we iterate its children to remove them. But removal modifies the data structure we're iterating. Current code clones the children list first:

```rust
if let Some(children) = &elem.element.children {
  for child_id in children.clone() {
    self.remove_element_recursive(child_id);
  }
}
```

This works but is inelegant.

## Future Requirement: Views

We need "simplified" views of the tree:

### Pruning

Remove generic leaf elements (`GenericElement` with no semantic value).

### Contraction

Collapse generic containers with single child:

- `Group → Group → Button` becomes `Button`
- Rule: Any `GenericGroup` with exactly 1 child gets collapsed

This view must:

- Stay synchronized with the source tree
- Be efficiently computable (ideally incremental)
- Support queries like "what's the simplified parent of X?"

## Design Questions

1. **Is there a standard data structure for incrementally-built trees with orphan queues?**

2. **How should we handle the partial/lazy nature?** Nodes have three states:

   - Not fetched (no entry)
   - Fetched, children unknown (`children: None`)
   - Fetched, children known (`children: Some([...])` or empty array if no children)

3. **Should relationships live in nodes or in separate indexes?**

   Current: `parent_id` and `children` are fields on `AXElement`

   Alternative: Keep data and relationships separate:

   ```rust
   elements: HashMap<ElementId, ElementData>,  // Just the data
   parent_of: HashMap<ElementId, ElementId>,
   children_of: HashMap<ElementId, Vec<ElementId>>,
   ```

4. **How should views be implemented?**

   - Materialized (computed on mutation, stored)
   - Virtual (computed on access)
   - Incremental (only recompute affected subtree)

5. **Is there prior art for "syncing to an external tree"?**

   This isn't a tree we build locally - it's a cache of an external tree (macOS accessibility) that we're partially mirroring. Standard tree libraries assume you own the structure.

## Relevant Characteristics

- **Read-heavy:** Most operations are reads (cache lookups)
- **Sparse:** Typically only a small fraction of the full accessibility tree is cached
- **Window-scoped:** Trees are independent per window
- **Event-driven removals:** Nodes are removed when OS sends destruction notification or via events like window removal.
- **Batch fetches:** `fetch_children` registers many nodes at once

## References

- Current implementation: `crates/axio/src/core/state.rs`
- Element type: `crates/axio/src/types/element.rs`
- Platform handle: `crates/axio/src/platform/macos/handles.rs`
