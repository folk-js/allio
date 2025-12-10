# Axio Code Review Notes

Review performed Dec 2024. Issues grouped by theme and priority.

## Completed (Dec 2024)

- [x] **Event overflow logging** - Now logs error when channel drops events
- [x] **Removed polling clone** - Windows Vec is moved, not cloned
- [x] **Fixed `fetch_focus` return type** - Now returns `AxioResult<...>`
- [x] **Hit test uses `is_fallback` flag** - No blocking/sleeping, client retries on next frame
- [x] **Removed dead recursive hit testing** - Simplified hit test logic
- [x] **Improved error context** - Notification registration errors include element ID
- [x] **Read-Write-Read races (core paths)** - `register_element` and `update_element` now return needed data from write closure

---

## Remaining Quick Fixes

### 1. Use HashSet for Window IDs

Currently iterating all windows to find removals every poll cycle. Add `window_ids: HashSet<WindowId>` to State for O(1) membership checks.

### 2. ElementId Counter Wrap

Using `AtomicU32` which wraps at ~4 billion. Since this goes to JS:

- Detect wrap and reset counter (with generation tracking?)
- Or accept the edge case (50 days at 1000/sec)
- Document the limitation

---

## Concurrency Issues

### 1. Read-Write-Read Race Windows (Partially Fixed)

Core paths have been fixed by returning needed data from write closures:

- `get_or_insert_element` returns `(ElementId, bool)` - includes whether watch existed
- `update_element` returns `(bool, bool)` - exists and changed flags

**Remaining:** Audit other code paths for this pattern. The `watch`/`unwatch` functions still have a variant of this issue (see issue #3 below).

### 2. OBSERVER_CONTEXTS Global

The global `Mutex<HashMap<u64, ObserverContext>>` is:

- A contention point (every notification callback locks it)
- The only global left (unconfirmed)

**Options:**

1. **Sharding:** Split into N maps, hash context_id to pick which one
2. **RwLock:** Reads far outnumber writes, so `RwLock<HashMap>>`
3. **DashMap:** Lock-free concurrent map
4. **Move into Axio:** Store contexts in `State`, pass `Arc<Axio>` to callbacks

Option 4 is cleanest but requires refactoring how we pass context to C callbacks.

### 3. Watch/Unwatch Can Leave Inconsistent State

If element is removed between taking watch handle and putting it back:

- Watch handle is dropped
- OS notifications still registered with dangling context

**Fix:** Check element still exists before storing watch back, unsubscribe if not.

---

## Tree Maintenance Complexity

See **[TREE_DESIGN.md](./TREE_DESIGN.md)** for a detailed writeup of the tree/element management challenges, intended for research into principled solutions.

**Summary:** The current home-grown tree maintenance is fragile. Key issues include non-deterministic children ordering, orphan resolution order, potential dangling references, and the need for future "view" support (pruning/contraction). Looking for established data structures or algorithms for incrementally-discovered, partially-cached trees with orphan queues.

---

## Lower Priority / Future Consideration

### Cache Staleness / TTL

No way to know if cached element data is fresh. `children: Some([...])` could be arbitrarily old.

**Future:** Add `last_fetched: Instant` to `ElementState`, expose staleness to clients.

### No Cache Invalidation API

Can't clear stale data. Only option is `fetch_element()` which updates one node.

**Future:** Add `invalidate_element(id)`, `invalidate_window(id)` that remove cached data without removing from tracking.

### Selection Change Events Lack Previous State

`SelectionChanged` only has new state. Undo/redo awareness would need old state too.

**Future:** Add `previous_text: Option<String>`, `previous_range: Option<(u32, u32)>`.

---

## Notes on Specific Decisions

### Why Not AXUIElementCopyAttributeValue(kAXIdentifierAttribute)?

More stable identity than CFHash, but requires IPC call per element. System is designed for tens to hundreds of calls per frame - constant IPC is not viable.

### CFHash Characteristics

Observed behavior (not guaranteed by Apple):

- Returns consistent value for lifetime of element
- Different elements have different hashes (no observed collisions)
- May change if element is destroyed and recreated

**TODO:** Find research/experience reports from others using CFHash for accessibility.
