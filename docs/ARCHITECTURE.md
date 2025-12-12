# Allio Architecture

## What is Allio?

At its core, Allio is:

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
│              Public API (Allio)                 │
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

**Allio (Coordinator)**

- Public API for consumers
- Orchestrates Registry + Platform calls
- Sets up watches after inserts
- Implements `EventHandler` trait for OS notifications

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
- Callbacks go through `EventHandler` trait (implemented by Allio)

### Platform/Allio Decoupling

Platform callbacks use the `EventHandler` trait:

```rust
pub(crate) trait EventHandler: Send + Sync + 'static {
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

Allio implements this trait, keeping Platform unaware of Allio internals.

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
    fn upsert_element(&mut self, elem: CachedElement) -> ElementId;
    fn upsert_window(&mut self, window: CachedWindow) -> WindowId;
    fn upsert_process(&mut self, process: CachedProcess) -> ProcessId;

    // === Update (modify existing, emit *Changed if different) ===
    fn update_element(&mut self, id: ElementId, data: ElementState);
    fn update_window(&mut self, id: WindowId, info: Window);

    // === Remove (cascade + cleanup, emit *Removed events) ===
    fn remove_element(&mut self, id: ElementId);
    fn remove_window(&mut self, id: WindowId);
    fn remove_process(&mut self, id: ProcessId);

    // === Query (read-only, no events) ===
    fn element(&self, id: ElementId) -> Option<&CachedElement>;
    fn window(&self, id: WindowId) -> Option<&CachedWindow>;
    fn find_element(&self, handle: &Handle) -> Option<ElementId>;
}
```

### Key Invariant

**Registry fields are private.** All changes go through these methods, guaranteeing:

- Indexes are always updated
- Events are always emitted
- Cascades always happen

## Public API

### Construction & Events

```rust
pub fn new() -> AllioResult<Self>;
pub fn builder() -> AllioBuilder;  // .exclude_pid(u32).build()
pub fn has_permissions() -> bool;
pub fn subscribe(&self) -> Receiver<Event>;
```

### Element Retrieval

The unified `get` method with recency:

```rust
/// Get element by ID with specified recency.
/// Returns Err(ElementNotFound) if element doesn't exist.
pub fn get(&self, id: ElementId, recency: Recency) -> AllioResult<Element>;

/// Get children with recency control.
pub fn children(&self, id: ElementId, recency: Recency) -> AllioResult<Vec<Element>>;

/// Get parent with recency control.
/// Returns Ok(None) if element is root (has no parent).
pub fn parent(&self, id: ElementId, recency: Recency) -> AllioResult<Option<Element>>;
```

### Discovery (always fresh from OS)

```rust
/// Get element at screen position.
pub fn element_at(&self, x: f64, y: f64) -> AllioResult<Option<Element>>;

/// Get root element for a window.
pub fn window_root(&self, window_id: WindowId) -> AllioResult<Option<Element>>;

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

### Actions (write to OS)

```rust
pub fn set_value(&self, id: ElementId, value: &Value) -> AllioResult<()>;
pub fn perform_action(&self, id: ElementId, action: Action) -> AllioResult<()>;
```

### Subscriptions

```rust
pub fn watch(&self, id: ElementId) -> AllioResult<()>;
pub fn unwatch(&self, id: ElementId) -> AllioResult<()>;
```

## Internal API

Used by polling and notification handlers:

```rust
// Polling updates (bulk sync)
pub(crate) fn sync_windows(&self, windows: Vec<Window>);
pub(crate) fn sync_mouse(&self, pos: Point);
pub(crate) fn sync_focused_window(&self, id: Option<WindowId>);

// Element caching helper
pub(crate) fn upsert_from_handle(&self, handle: Handle, window_id: WindowId, pid: ProcessId) -> ElementId;

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
pub fn watch(&self, id: ElementId) -> AllioResult<()>;

// Public: remove change notifications (keeps destruction)
pub fn unwatch(&self, id: ElementId) -> AllioResult<()>;
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
crates/allio/src/
├── lib.rs              # Re-exports only
├── core/
│   ├── mod.rs          # Allio struct, construction, EventHandler impl
│   ├── queries.rs      # get, children, parent, element_at, etc.
│   ├── actions.rs      # set_value, perform_action (write to OS)
│   ├── sync.rs         # sync_windows, sync_mouse (bulk updates from polling)
│   ├── handlers.rs     # handle_* methods (process OS notifications)
│   ├── subscriptions.rs # watch/unwatch
│   ├── adapters.rs     # build_element, build_snapshot (Registry → public types)
│   └── registry/
│       ├── mod.rs      # Registry struct, global state, CachedElement/Window/Process
│       ├── elements.rs # upsert_element, update_element, remove_element
│       ├── windows.rs  # upsert_window, update_window, remove_window
│       ├── processes.rs # upsert_process, remove_process
│       └── tree.rs     # ElementTree for parent/child relationships
├── platform/
│   ├── mod.rs          # Re-exports
│   ├── traits.rs       # Platform, PlatformHandle, EventHandler, ElementEvent
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
| **EventHandler**         | Trait for OS notifications → Allio                  |
| **upsert/update/remove** | Registry mutation operations                        |
| **watch/unwatch**        | Element change subscriptions                        |
| **sync\_**               | Bulk updates from polling                           |
