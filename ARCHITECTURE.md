# Axio Architecture

## What is Axio?

At its core, Axio is:

1. A **cache** of accessibility state from the OS
2. A **query interface** to that cache
3. A **sync mechanism** that keeps the cache fresh (polling + notifications)
4. An **event stream** for clients to mirror state changes

## Core Concepts

### Entities

```
Process (1:N)──→ Window (1:N)──→ Element (1:N)──→ Children
```

Each entity has:

- **Data**: The info we expose (`AXWindow`, `AXElement`)
- **Handle**: OS reference for operations
- **Tracking metadata**: hashes, parent links, etc.

### Cascade Rules

Removal cascades down the hierarchy:

- Remove Process → removes all its Windows → removes all their Elements
- Remove Window → removes all its Elements
- Remove Element → removes all child Elements

### Operations Vocabulary

| Prefix          | Meaning                         | Hits OS? | Modifies Cache? |
| --------------- | ------------------------------- | -------- | --------------- |
| `get_`          | Read from cache                 | No       | No              |
| `fetch_`        | Read from OS, cache result      | Yes      | Yes             |
| `get_or_fetch_` | Read from cache, fallback to OS | Maybe    | Yes             |
| `set_`          | Write value to OS               | Yes      | Maybe (?)       |
| `perform_`      | Execute action on OS            | Yes      | No              |
| `watch/unwatch` | Subscribe to element changes    | Yes      | Yes (metadata)  |
| `sync_`         | Bulk update from polling        | No (?)   | Yes             |
| `on_`           | Handle OS notification          | No (?)   | Yes             |

## Architecture Layers

```
┌─────────────────────────────────────────────────┐
│              Public API (Axio)                  │
│   get_*, fetch_*, set_*, perform_*, watch       │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│              State (Cache)                      │
│   insert, update, remove, get                   │
│   Owns: data, indexes, event emission           │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│              Platform (OS Interface)            │
│   Traits: Platform, PlatformHandle, Observer    │
│   fetch_*, subscribe_*, perform_*               │
└─────────────────────────────────────────────────┘
```

### Layer Responsibilities

**Axio (Coordinator)**

- Public API for consumers
- Orchestrates State + Platform calls
- Sets up watches after inserts
- Handles errors and edge cases

**State (Cache + Events)**

- Pure data management
- Maintains indexes for fast lookups
- Maintains relationships (parent-child, element-window)
- Cascading removals
- Emits events when data changes
- **No OS calls, no subscriptions**

**Platform (OS Interface)**

- Trait-based abstraction over OS APIs
- macOS implementation via Accessibility APIs
- Handles all FFI and unsafe code

## State Operations

State is the single source of truth for cached data. All mutations emit corresponding events.

```rust
impl State {
  // === Insert (add to cache, emit *Added event if not already in cache) ===
  fn insert_element(&mut self, elem: ElementState) -> Option<ElementId>;
  fn insert_window(&mut self, window: WindowState) -> WindowId;
  fn insert_process(&mut self, process: ProcessState) -> ProcessId;

  // === Update (modify, emit *Changed event if different) ===
  fn update_element(&mut self, id: ElementId, data: AXElement) -> bool;
  fn update_window(&mut self, id: WindowId, data: AXWindow) -> bool;

  // === Remove (cascade + cleanup, emit *Removed events) ===
  fn remove_element(&mut self, id: ElementId);
  fn remove_window(&mut self, id: WindowId);
  fn remove_process(&mut self, id: ProcessId);

  // === Query (read-only, no events) ===
  fn get_element(&self, id: ElementId) -> Option<&AXElement>;
  fn get_window(&self, id: WindowId) -> Option<&AXWindow>;
  fn find_element_by_hash(&self, hash: u64) -> Option<ElementId>;
}
```

### Key Invariant

**State fields are private.** All mutations go through these methods, guaranteeing:

- Indexes are always updated
- Events are always emitted
- Cascades always happen

## The "Fetch Always Caches" Rule

Every `fetch_*` operation follows this pattern:

1. Call OS (via Platform)
2. Insert/update cache (via State)
3. Events emitted automatically by State
4. Return the cached data

```rust
pub fn fetch_children(&self, parent_id: ElementId, max: usize) -> AxioResult<Vec<AXElement>> {
  // 1. Get context from cache
  let (handle, window_id, pid) = ...;

  // 2. Call OS
  let child_handles = handle.fetch_children();

  // 3. Cache each (State emits events)
  for child_handle in child_handles {
    let elem_state = build_element_state(child_handle, window_id, pid);
    state.insert_element(elem_state);
  }

  // 4. Return cached data
  Ok(children)
}
```

## Public API

### Construction & Events

```rust
pub fn new() -> AxioResult<Self>;
pub fn with_options(opts: AxioOptions) -> AxioResult<Self>;
pub fn verify_permissions() -> bool;
pub fn subscribe(&self) -> Receiver<Event>;
```

### Queries (cache only, fast)

```rust
pub fn get_window(&self, id: WindowId) -> Option<AXWindow>;
pub fn get_windows(&self) -> Vec<AXWindow>;
pub fn get_element(&self, id: ElementId) -> Option<AXElement>;
pub fn get_elements(&self, ids: &[ElementId]) -> Vec<AXElement>;
pub fn get_focused_window(&self) -> Option<WindowId>;
pub fn get_depth_order(&self) -> Vec<WindowId>;
pub fn snapshot(&self) -> Snapshot;
// TODO: ^ this should be get_snapshot()!
```

### Fetches (OS + cache)

```rust
pub fn fetch_element_at(&self, x: f64, y: f64) -> AxioResult<AXElement>;
pub fn fetch_children(&self, id: ElementId, max: usize) -> AxioResult<Vec<AXElement>>;
pub fn fetch_parent(&self, id: ElementId) -> AxioResult<Option<AXElement>>;
pub fn fetch_element(&self, id: ElementId) -> AxioResult<AXElement>;  // refresh
pub fn fetch_window_root(&self, id: WindowId) -> AxioResult<AXElement>;
pub fn fetch_window_focus(&self, id: WindowId) -> AxioResult<FocusInfo>;
pub fn fetch_screen_size(&self) -> (f64, f64);
```

### Writes (mutate OS)

```rust
pub fn set_value(&self, id: ElementId, value: &Value) -> AxioResult<()>;
pub fn perform_click(&self, id: ElementId) -> AxioResult<()>;
```

### Subscriptions

```rust
pub fn watch(&self, id: ElementId) -> AxioResult<()>;
pub fn unwatch(&self, id: ElementId) -> AxioResult<()>;
```

## Internal API

Used by polling and notification handlers:

```rust
// Polling updates (bulk sync)
pub(crate) fn sync_windows(&self, windows: Vec<AXWindow>);
pub(crate) fn sync_mouse(&self, pos: Point);
pub(crate) fn sync_focused_window(&self, id: Option<WindowId>);

// Notification handlers
pub(crate) fn on_element_destroyed(&self, id: ElementId);
pub(crate) fn on_focus_changed(&self, pid: u32, handle: Handle);
pub(crate) fn on_selection_changed(&self, pid: u32, handle: Handle);
pub(crate) fn on_element_changed(&self, id: ElementId, what: Notification);
```

## Watch System

Two kinds of watching:

1. **Destruction tracking**: Automatic for every element (cleans up cache when element dies)
2. **Change watching**: Opt-in via `watch()` (value, title, children changes)

TODO: think about this more... might want a single watch system, unclear atm.

```rust
// Internal: called on insert
fn setup_destruction_watch(&self, id: ElementId);

// Public: add change notifications
pub fn watch(&self, id: ElementId) -> AxioResult<()>;

// Public: remove change notifications (keeps destruction)
pub fn unwatch(&self, id: ElementId) -> AxioResult<()>;
```

## Event Guarantees

Because State owns event emission:

- `insert_element` → always emits `ElementAdded` if not already in cache
- `update_element` → emits `ElementChanged` if data differs
- `remove_element` → emits `ElementRemoved` for element + all descendants
- `remove_window` → emits `WindowRemoved` + `ElementRemoved` for all elements

**You cannot change state without emitting the correct events.**

## File Structure

```
crates/axio/src/
├── lib.rs              # Re-exports only
├── core/
│   ├── mod.rs          # Axio struct, construction, events
│   ├── state.rs        # State with private fields + operations
│   ├── queries.rs      # get_*, fetch_* implementations
│   ├── mutations.rs    # set_*, perform_* implementations
│   ├── subscriptions.rs # watch/unwatch
│   └── internal.rs     # sync_*, on_* handlers
├── platform/
│   ├── mod.rs          # Traits + type aliases
│   ├── traits.rs       # Platform, PlatformHandle, PlatformObserver
│   ├── element_ops.rs  # Element building + discovery
│   └── macos/          # macOS implementation
└── types/              # AXElement, AXWindow, Event, etc.
```

## Summary

| Concept                  | Meaning                             |
| ------------------------ | ----------------------------------- |
| **State**                | Cache with automatic event emission |
| **get\_**                | Cache lookup (fast, no OS)          |
| **fetch\_**              | OS call → cache → return            |
| **set*/perform***        | Write to OS                         |
| **watch/unwatch**        | Element change subscriptions        |
| **sync\_**               | Bulk updates from polling           |
| **on\_**                 | Notification handlers               |
| **insert/update/remove** | State operations (internal)         |

TODO: think if we want insert/update/remove, or just upsert/remove. Not sure yet what we need from the internal state mutations from elewhere in the code, what would be performant, etc.
