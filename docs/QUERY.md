# Observation & Query System

## Overview

This document outlines the observation and query systems for Allio, enabling reactive tree watching and declarative data extraction from accessibility trees.

## Goals

1. **Keep observed subtrees fresh** via polling, without manual intervention
2. **Provide simple change detection** for client-side re-querying
3. **Enable declarative queries** over cached data for structured extraction
4. **Maintain predictable semantics** with element-level change events

## Non-Goals (For Now)

- Notification-based optimization (stick with polling for correctness)
- Complex query syntax (CSS-like selectors only)
- Cross-window queries
- Attribute-level change tracking

---

## Observation System (Rust)

### Problem

Currently, keeping a subtree fresh requires manual recursive fetching and watch management. This is error-prone, verbose, and doesn't scale.

### Solution

A single `observe(root_id, { depth })` call that:

- Marks a subtree for periodic polling
- Runs on a separate thread (non-blocking)
- Emits element-level events for each change (via registry as normal)
- Emits ONE `subtree:changed` event per polling cycle (if anything changed)

### API

```rust
pub fn observe(&self, root_id: ElementId, config: ObserveConfig) -> AllioResult<ObservationHandle>;

pub struct ObserveConfig {
    pub depth: Option<usize>,            // None = infinite
    pub wait_between: Option<Duration>,  // Wait time after sweep completes (default: 100ms)
}

// Stops observation on drop
pub struct ObservationHandle { ... }
```

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Main Thread                               │
│  - Window polling (existing)                                │
│  - Event dispatch                                           │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│               Observation Thread (new)                       │
│  - Checks observed subtrees every ~10ms                     │
│  - Spawns sweep tasks to rayon pool when ready              │
│  - Each sweep runs to completion (no partial work)          │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│               Rayon Thread Pool                              │
│  - Executes subtree sweeps concurrently                     │
│  - Bounded concurrency (e.g., 4 threads)                    │
│  - Each sweep: fetch_node() recursively, detect changes     │
└─────────────────────────────────────────────────────────────┘
```

### Sweep Timing

Each observed subtree maintains:

- `in_progress: AtomicBool` - prevents overlapping sweeps
- `last_completed: Instant` - tracks when sweep finished
- `wait_between: Duration` - wait time after sweep before next one starts

**Timing model (wait-after, not fixed interval):**

```
Sweep 1 starts
  ↓ (takes 50ms)
Sweep 1 completes
  ↓ (wait 100ms)
Sweep 2 starts
  ↓ (takes 200ms - slow app)
Sweep 2 completes
  ↓ (wait 100ms)
Sweep 3 starts
  ...
```

This scales naturally: slow sweeps don't pile up, fast sweeps run frequently.

### Sweep Algorithm

Per subtree:

1. Skip if `in_progress` or `elapsed_since_completion < wait_between`
2. Mark `in_progress = true`
3. Recursively traverse tree:
   - `fetch_node()` for each element (liveness check + attributes + children)
   - Compare with cache, emit `element:changed` / `element:added` / `element:removed`
   - Discover new children, detect removed children
4. If any changes occurred, emit `subtree:changed`
5. Mark `in_progress = false`, update `last_completed`

### Events

```rust
// Element-level (existing events, still fire)
Event::ElementAdded { element }
Event::ElementChanged { element_id, ... }
Event::ElementRemoved { element_id }

// Subtree-level (new, aggregated)
Event::SubtreeChanged {
    root_id: ElementId,
    added: Vec<ElementId>,
    removed: Vec<ElementId>,
    modified: Vec<ElementId>,
}
```

**Semantics:**

- N element events fire during sweep (granular, predictable)
- 1 subtree event fires at end of sweep (only if any changes)
- Client can listen to either level depending on use case

### Non-Blocking Guarantees

- Sweep runs to completion on its own thread (no element limit)
- If sweep takes a long time, it just finishes later (no overlap)
- Main thread never blocks on observation
- Window polling continues independently

---

## Query System (TypeScript)

### Problem

Extracting structured data from accessibility trees requires imperative traversal with many async RPC calls. This is slow and verbose.

### Solution

A synchronous `query()` function that operates over the client's cached `elements` map, with CSS-like selector syntax and field extraction.

### API

```typescript
interface QueryOptions {
  selector: string; // CSS-like selector
  extract?: Record<string, string>; // Field → role mapping
}

function query(allio: Allio, rootId: ElementId, options: QueryOptions): any[];
```

### Selector Syntax

Minimal subset of CSS selectors:

```
"tree"                  // Matches if root.role === 'tree'
"tree > listitem"       // Direct children with role 'listitem'
"tree listitem"         // Any descendants with role 'listitem'
"tree > listitem > *"   // Any direct children of listitems (future)
```

### Field Extraction

The `extract` option maps field names to role selectors, extracting values from descendants:

```typescript
// Input tree structure:
// tree
//   └── listitem
//         ├── checkbox (value: true)
//         └── textfield (value: "Buy milk")

const result = query(allio, treeId, {
  selector: "tree > listitem",
  extract: {
    completed: "checkbox",
    text: "textfield",
  },
});

// Output:
// [{ completed: true, text: "Buy milk" }, ...]
```

### Implementation

Pure function, no RPC, operates on `allio.elements`:

1. Parse selector into steps: `[{role, combinator}, ...]`
2. Find all elements matching selector (tree traversal)
3. For each match, extract fields by finding descendant with matching role
4. Return array of extracted objects (or elements if no extraction)

---

## Usage Example

```typescript
// 1. Observe the tree
const treeId = await findTreeElement();
const handle = await allio.observe(treeId, { depth: 3 });

// 2. Define query
function getTodos(): TodoItem[] {
  return query(allio, treeId, {
    selector: "tree > listitem",
    extract: { text: "textfield", completed: "checkbox" },
  });
}

// 3. React to changes
allio.on("subtree:changed", (event) => {
  if (event.root_id === treeId) {
    const todos = getTodos(); // Sync, cheap
    updateUI(todos);
  }
});

// 4. Cleanup
handle.dispose();
```

---

## Implementation Plan

### Phase 1: Observation Infrastructure

- [ ] Add `ObservedSubtree` struct with polling state
- [ ] Create observation thread (separate from window polling)
- [ ] Implement `observe()` / `unobserve()` API
- [ ] Add RPC handlers

### Phase 2: Sweep Implementation

- [ ] Implement `sweep_subtree_recursive()`
- [ ] Integrate with rayon thread pool
- [ ] Track changes during sweep
- [ ] Emit element events during sweep
- [ ] Emit `subtree:changed` at end of sweep

### Phase 3: Query Engine (TypeScript)

- [ ] Implement selector parser
- [ ] Implement tree matcher
- [ ] Implement field extractor
- [ ] Add to allio-client package

### Phase 4: Integration

- [ ] Update query demo to use new APIs
- [ ] Test with various tree structures
- [ ] Performance testing with large trees

---

## Open Questions

1. **Wait time tuning**: Is 100ms the right default? Should it be configurable per-tree?
2. **Depth limits**: Should we warn if depth is very large (e.g., >10)?
3. **Memory**: Should we evict elements from cache when observation stops?
4. **Parallelism**: Max concurrent sweeps - 4 threads? Configurable?

---

## Future Considerations

- Notification-based optimization (reduce polling for apps that emit reliably)
- More selector syntax: attributes `[label="X"]`, wildcards `*`, combinators `+`, `~`
- Query result caching / memoization
- Differential updates (only re-query changed subtrees)
