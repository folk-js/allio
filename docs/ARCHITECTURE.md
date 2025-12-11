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

- **Data**: The info we expose (`Window`, `Element`)
- **Handle**: OS reference for operations
- **Tracking metadata**: hashes, parent links, etc.

### Cascade Rules

Removal cascades down the hierarchy:

- Remove Process → removes all its Windows → removes all their Elements
- Remove Window → removes all its Elements
- Remove Element → removes all child Elements

### Operations Vocabulary

| Prefix          | Meaning                                   | Hits OS? | Modifies Cache? |
| --------------- | ----------------------------------------- | -------- | --------------- |
| `get`/`get_`    | Read from cache (with optional freshness) | Maybe\*  | Maybe\*         |
| `fetch_`        | Discovery: find new elements from OS      | Yes      | Yes             |
| `refresh_`      | Update existing element from OS           | Yes      | Yes             |
| `set_`          | Write value to OS                         | Yes      | Maybe           |
| `perform_`      | Execute action on OS                      | Yes      | No              |
| `watch/unwatch` | Subscribe to element changes              | Yes      | Yes (metadata)  |
| `sync_`         | Bulk update from polling                  | No       | Yes             |
| `on_`           | Handle OS notification                    | No       | Yes             |

\*`get(id, freshness)` respects the `Freshness` parameter: `Cached` never hits OS, `Fresh` always does, `MaxAge(d)` hits OS only if cached data is older than `d`.

### Freshness Model

The `Freshness` enum controls how up-to-date data should be:

```rust
pub enum Freshness {
    Cached,         // Use cached value, never hit OS
    Fresh,          // Always fetch from OS
    MaxAge(Duration), // Fetch if cached data is older than this
}
```

This enables callers to make explicit tradeoffs between latency and freshness.

### get vs fetch Distinction

- **`get(id, freshness)`**: Retrieve a known element. The element must already be in the cache (discovered previously). Freshness controls whether to refresh from OS.
- **`fetch_*()`**: Discovery operations that find new elements. These always hit the OS because we don't know what we'll find. Examples:
  - `fetch_element_at(x, y)` - Find element at screen position
  - `fetch_children(id)` - Discover children of an element
  - `fetch_window_root(id)` - Get root element for a window
  - `fetch_window_focus(id)` - Find focused element in window

## Architecture Layers

```
┌─────────────────────────────────────────────────┐
│              Public API (Axio)                  │
│   get_*, fetch_*, set_*, perform_*, watch       │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│              Registry (Cache)                   │
│   get_or_insert, update, remove, get            │
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
- Orchestrates Registry + Platform calls
- Sets up watches after inserts
- Handles errors and edge cases

**Registry (Cache + Events)**

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
- **Note**: Observer creation requires an `Axio` reference for callbacks (intentional coupling - see below)

### Platform/Axio Coupling for Observers

The `Platform::create_observer()` and `PlatformObserver::subscribe_*` methods take an `Axio` reference. This is intentional:

1. **OS callbacks need state access**: When macOS fires an accessibility notification, the callback must access Axio to update state and emit events.

2. **C callback constraint**: macOS `AXObserver` callbacks receive a raw pointer that we map back to context. We can't pass Rust closures directly to C code.

3. **Alternative considered**: Using channels to decouple Platform from Axio would add latency and complexity. The current design keeps notification handling fast and atomic.

The coupling is contained to observer creation - all other Platform operations (`fetch_*` on handles) are pure and return data without side effects.

## Registry Operations

Registry is the single source of truth for cached data. All mutations emit corresponding events.

### Why Not Upsert?

Elements have two identities:

- **Hash** (from OS): Identifies the same OS element across fetches
- **ElementId** (ours): Stable ID we give to clients

When we fetch an element from OS:

- If hash already exists → return the **existing** ElementId (no update, no event)
- If hash is new → insert with **new** ElementId, emit ElementAdded

This is "get or insert" semantics, not upsert. We don't want to update data when hash matches because:

1. The existing cached data is already valid
2. Updating would require reconciling two different ElementIds
3. Clients have references to the existing ID

Updating data happens separately via `update_element_data(id, data)` when we explicitly refresh.

### Registry Methods

```rust
impl Registry {
  // === Get or Insert (ensure tracked, emit *Added only if new) ===
  fn get_or_insert_element(&mut self, elem: ElementEntry) -> ElementId;
  fn get_or_insert_window(&mut self, window: WindowEntry) -> WindowId;
  fn get_or_insert_process(&mut self, process: ProcessEntry) -> ProcessId;

  // === Update (modify existing, emit *Changed if different) ===
  fn update_element_data(&mut self, id: ElementId, data: ElementData) -> Result<bool>;
  fn update_window(&mut self, id: WindowId, data: Window) -> Result<bool>;

  // === Remove (cascade + cleanup, emit *Removed events) ===
  fn remove_element(&mut self, id: ElementId);
  fn remove_window(&mut self, id: WindowId);   // cascades to elements
  fn remove_process(&mut self, id: ProcessId); // cascades to windows

  // === Query (read-only, no events) ===
  fn get_element(&self, id: ElementId) -> Option<Element>;
  fn get_window(&self, id: WindowId) -> Option<&Window>;
  fn find_element_by_hash(&self, hash: u64) -> Option<ElementId>;
}
```

### Operation Summary

| Operation               | Key  | Behavior                         | Event                          |
| ----------------------- | ---- | -------------------------------- | ------------------------------ |
| `get_or_insert_element` | hash | Return existing ID or insert new | ElementAdded (if inserted)     |
| `update_element`        | ID   | Update data of existing          | ElementChanged (if changed)    |
| `remove_element`        | ID   | Remove + cascade children        | ElementRemoved (for all)       |
| `get_or_insert_window`  | ID   | Return existing or insert new    | WindowAdded (if inserted)      |
| `update_window`         | ID   | Update data of existing          | WindowChanged (if changed)     |
| `remove_window`         | ID   | Remove + cascade elements        | WindowRemoved + ElementRemoved |

### Key Invariant

**Registry fields are private.** All mutations go through these methods, guaranteeing:

- Indexes are always updated
- Events are always emitted
- Cascades always happen

## The "Fetch Always Caches" Rule

Every `fetch_*` operation follows this pattern:

1. Call OS (via Platform)
2. Insert/update cache (via Registry)
3. Events emitted automatically by Registry
4. Return the cached data

```rust
pub fn fetch_children(&self, parent_id: ElementId, max: usize) -> AxioResult<Vec<Element>> {
  // 1. Get context from cache
  let (handle, window_id, pid) = ...;

  // 2. Call OS
  let child_handles = handle.fetch_children();

  // 3. Cache each (Registry emits events only for truly new elements)
  for child_handle in child_handles {
    let elem_entry = build_element_entry(child_handle, window_id, pid);
    registry.get_or_insert_element(elem_entry);
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
pub fn has_permissions() -> bool;
pub fn subscribe(&self) -> Receiver<Event>;
```

### Unified Get API

The primary way to retrieve elements with explicit freshness control:

```rust
// Get element with freshness control
pub fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;

// Convenience: always cached
pub fn get_cached(&self, id: ElementId) -> Option<Element>;

// Convenience: always fresh
pub fn refresh_element(&self, id: ElementId) -> AxioResult<Element>;
```

### Queries (cache only, fast)

```rust
pub fn get_window(&self, id: WindowId) -> Option<Window>;
pub fn get_windows(&self) -> Vec<Window>;
pub fn get_elements(&self, ids: &[ElementId]) -> Vec<Element>;
pub fn get_focused_window(&self) -> Option<WindowId>;
pub fn get_z_order(&self) -> Vec<WindowId>;
pub fn snapshot(&self) -> Snapshot;
```

### Discovery (OS calls, cache result)

These find new elements - they always hit the OS:

```rust
pub fn fetch_element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;
pub fn fetch_children(&self, id: ElementId, max: usize) -> AxioResult<Vec<Element>>;
pub fn fetch_parent(&self, id: ElementId) -> AxioResult<Option<Element>>;
pub fn fetch_window_root(&self, id: WindowId) -> AxioResult<Element>;
pub fn fetch_window_focus(&self, id: WindowId) -> AxioResult<(Option<Element>, Option<TextSelection>)>;
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
pub(crate) fn sync_windows(&self, windows: Vec<Window>);
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

Because Registry owns event emission:

- `get_or_insert_element` → emits `ElementAdded` only if actually inserted (new hash)
- `update_element` → emits `ElementChanged` only if data differs
- `remove_element` → emits `ElementRemoved` for element + all descendants
- `remove_window` → emits `WindowRemoved` + `ElementRemoved` for all elements

**You cannot change state without emitting the correct events.**

## File Structure

```
crates/axio/src/
├── lib.rs              # Re-exports only
├── core/
│   ├── mod.rs          # Axio struct, construction, events
│   ├── state.rs        # Registry with private fields + operations
│   ├── queries.rs      # get, get_*, fetch_*, element building
│   ├── mutations.rs    # set_*, perform_*, on_* handlers
│   ├── subscriptions.rs # watch/unwatch
│   └── tree.rs         # ElementTree for parent/child relationships
├── platform/
│   ├── mod.rs          # Traits + type aliases
│   ├── traits.rs       # Platform, PlatformHandle, PlatformObserver
│   └── macos/          # macOS implementation
├── polling/            # Window/focus sync loop
└── types/              # Element, Window, Event, Freshness, etc.
```

## Summary

| Concept                         | Meaning                                             |
| ------------------------------- | --------------------------------------------------- |
| **Registry**                    | Cache with automatic event emission                 |
| **Freshness**                   | How up-to-date data should be (Cached/Fresh/MaxAge) |
| **get(id, freshness)**          | Unified element retrieval with freshness control    |
| **get\_**                       | Cache lookup (fast, no OS)                          |
| **fetch\_**                     | Discovery: find new elements from OS                |
| **refresh\_**                   | Update existing element from OS                     |
| **set\_/perform\_**             | Write to OS                                         |
| **watch/unwatch**               | Element change subscriptions                        |
| **sync\_**                      | Bulk updates from polling                           |
| **on\_**                        | Notification handlers                               |
| **get_or_insert/update/remove** | State operations (internal)                         |
