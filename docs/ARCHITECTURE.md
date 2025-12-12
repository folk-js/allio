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
- **Handle**: OS reference for operations (used as HashMap key for deduplication)

### Cascade Rules

Removal cascades down the hierarchy:

- Remove Process → removes all its Windows → removes all their Elements
- Remove Window → removes all its Elements
- Remove Element → removes all child Elements

### Recency Model

The `Recency` enum controls how up-to-date data should be:

```rust
pub enum Recency {
    Any,              // Use cached value, never hit OS
    Current,          // Always fetch from OS
    MaxAge(Duration), // Fetch if cached data is older than this
}
```

This enables callers to make explicit tradeoffs between latency and recency.

## Architecture Layers

```
┌─────────────────────────────────────────────────┐
│              Public API (Axio)                  │
│   get, children, parent, element_at, etc.       │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│              Registry (Cache)                   │
│   upsert, update, remove, element, window       │
│   Owns: data, indexes, tree, event emission     │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│              Platform (OS Interface)            │
│   Traits: Platform, PlatformHandle, Observer    │
│   fetch_*, set_*, perform_*                     │
└─────────────────────────────────────────────────┘
```

### Layer Responsibilities

**Axio (Coordinator)**

- Public API for consumers
- Orchestrates Registry + Platform calls
- Sets up watches after inserts
- Implements `PlatformCallbacks` trait for OS notifications

**Registry (Cache + Events)**

- Pure data management
- Maintains indexes (`handle_to_id`, `window_handle_to_id`)
- Maintains tree relationships via `ElementTree`
- Cascading removals
- Emits events when data changes
- **No OS calls, no subscriptions**

**Platform (OS Interface)**

- Trait-based abstraction over OS APIs
- macOS implementation via Accessibility APIs
- Handles all FFI and unsafe code
- Callbacks go through `PlatformCallbacks` trait (implemented by Axio)

### Platform/Axio Decoupling

Platform callbacks use the `PlatformCallbacks` trait:

```rust
pub(crate) trait PlatformCallbacks: Send + Sync + 'static {
    type Handle: PlatformHandle;
    fn on_element_event(&self, event: ElementEvent<Self::Handle>);
}

pub enum ElementEvent<H> {
    Destroyed(ElementId),
    Changed(ElementId, Notification),
    ChildrenChanged(ElementId),
    FocusChanged(H),
    SelectionChanged { handle: H, text: String, range: Option<(u32, u32)> },
}
```

Axio implements this trait, keeping Platform unaware of Axio internals.

## Element Identity

Elements are deduplicated using their OS handle:

- **Handle** (`ElementHandle`): Wraps macOS `AXUIElement`, implements `Hash + Eq`
- **ElementId**: Our stable u32 ID given to clients

The handle's `Hash` uses `CFHash` (computed once, cached). The handle's `Eq` uses `CFEqual` for collision resolution (local comparison, no IPC).

Registry maintains `handle_to_id: HashMap<Handle, ElementId>` for O(1) deduplication.

## Registry Operations

Registry is the single source of truth for cached data. All mutations emit corresponding events.

### Registry Methods

```rust
impl Registry {
    // === Upsert (insert or update, emit events appropriately) ===
    fn upsert_element(&mut self, elem: ElementEntry) -> ElementId;
    fn upsert_window(&mut self, window: WindowEntry) -> WindowId;
    fn upsert_process(&mut self, process: ProcessEntry) -> ProcessId;

    // === Update (modify existing, emit *Changed if different) ===
    fn update_element(&mut self, id: ElementId, data: ElementData);
    fn update_window(&mut self, id: WindowId, info: Window);

    // === Remove (cascade + cleanup, emit *Removed events) ===
    fn remove_element(&mut self, id: ElementId);
    fn remove_window(&mut self, id: WindowId);
    fn remove_process(&mut self, id: ProcessId);

    // === Query (read-only, no events) ===
    fn element(&self, id: ElementId) -> Option<&ElementEntry>;
    fn window(&self, id: WindowId) -> Option<&WindowEntry>;
    fn find_element(&self, handle: &Handle) -> Option<ElementId>;
}
```

### Key Invariant

**Registry fields are private.** All mutations go through these methods, guaranteeing:

- Indexes are always updated
- Events are always emitted
- Cascades always happen

## Public API

### Construction & Events

```rust
pub fn new() -> AxioResult<Self>;
pub fn builder() -> AxioBuilder;  // .exclude_pid(u32).build()
pub fn has_permissions() -> bool;
pub fn subscribe(&self) -> Receiver<Event>;
```

### Element Retrieval

The unified `get` method with recency:

```rust
/// Get element by ID with specified recency.
/// Returns Err(ElementNotFound) if element doesn't exist.
pub fn get(&self, id: ElementId, recency: Recency) -> AxioResult<Element>;

/// Get children with recency control.
pub fn children(&self, id: ElementId, recency: Recency) -> AxioResult<Vec<Element>>;

/// Get parent with recency control.
/// Returns Ok(None) if element is root (has no parent).
pub fn parent(&self, id: ElementId, recency: Recency) -> AxioResult<Option<Element>>;
```

### Discovery (always fresh from OS)

```rust
/// Get element at screen position.
pub fn element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;

/// Get root element for a window.
pub fn window_root(&self, window_id: WindowId) -> AxioResult<Option<Element>>;

/// Get screen dimensions (cached after first call).
pub fn screen_size(&self) -> (f64, f64);
```

### Window/State Queries (cache only, fast)

```rust
pub fn window(&self, id: WindowId) -> Option<Window>;
pub fn all_windows(&self) -> Vec<Window>;
pub fn focused_window(&self) -> Option<WindowId>;
pub fn z_order(&self) -> Vec<WindowId>;
pub fn all_elements(&self) -> Vec<Element>;
pub fn snapshot(&self) -> Snapshot;
```

### Mutations (write to OS)

```rust
pub fn set_value(&self, id: ElementId, value: &Value) -> AxioResult<()>;
pub fn perform_action(&self, id: ElementId, action: Action) -> AxioResult<()>;
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

// Element caching helper
pub(crate) fn cache_from_handle(&self, handle: Handle, window_id: WindowId, pid: ProcessId) -> ElementId;

// Watch setup
pub(crate) fn ensure_watched(&self, element_id: ElementId);
```

## Watch System

Two kinds of watching:

1. **Destruction tracking**: Automatic for every element (cleans up cache when element dies)
2. **Change watching**: Opt-in via `watch()` (value, title, children changes)

```rust
// Internal: called on insert
fn ensure_watched(&self, id: ElementId);

// Public: add change notifications
pub fn watch(&self, id: ElementId) -> AxioResult<()>;

// Public: remove change notifications (keeps destruction)
pub fn unwatch(&self, id: ElementId) -> AxioResult<()>;
```

## Event Guarantees

Because Registry owns event emission:

- `upsert_element` → emits `ElementAdded` only if truly new
- `update_element` → emits `ElementChanged` only if data differs
- `remove_element` → emits `ElementRemoved` for element + all descendants
- `remove_window` → emits `WindowRemoved` + `ElementRemoved` for all elements

**You cannot change state without emitting the correct events.**

## File Structure

```
crates/axio/src/
├── lib.rs              # Re-exports only
├── core/
│   ├── mod.rs          # Axio struct, construction, PlatformCallbacks impl
│   ├── queries.rs      # get, children, parent, element_at, etc.
│   ├── mutations.rs    # set_value, perform_action, sync_*, handlers
│   ├── subscriptions.rs # watch/unwatch
│   ├── builders.rs     # build_element, build_snapshot (Registry → public types)
│   └── registry/
│       ├── mod.rs      # Registry struct, global state, ElementEntry
│       ├── elements.rs # upsert_element, update_element, remove_element
│       ├── windows.rs  # upsert_window, update_window, remove_window
│       ├── processes.rs # upsert_process, remove_process
│       └── tree.rs     # ElementTree for parent/child relationships
├── platform/
│   ├── mod.rs          # Re-exports
│   ├── traits.rs       # Platform, PlatformHandle, PlatformCallbacks, ElementEvent
│   └── macos/          # macOS implementation
│       ├── mod.rs      # Platform trait impl
│       ├── handles.rs  # ElementHandle (with Hash + Eq)
│       ├── observer.rs # AXObserver management
│       └── ...
├── polling/            # Window/focus sync loop
└── types/              # Element, Window, Event, Recency, etc.
```

## Summary

| Concept                  | Meaning                                             |
| ------------------------ | --------------------------------------------------- |
| **Registry**             | Cache with automatic event emission                 |
| **Recency**              | How up-to-date data should be (Any/Current/MaxAge)  |
| **get(id, recency)**     | Element retrieval with recency control              |
| **Handle**               | OS reference, used as HashMap key for deduplication |
| **ElementId**            | Our stable ID given to clients                      |
| **PlatformCallbacks**    | Trait for OS notifications → Axio                   |
| **upsert/update/remove** | Registry mutation operations                        |
| **watch/unwatch**        | Element change subscriptions                        |
| **sync\_**               | Bulk updates from polling                           |
