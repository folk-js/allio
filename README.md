# Allio (Accessibility I/O)

> [!IMPORTANT]
> This is an experimental system to expose accessibility trees as read-write interfaces and augment existing apps with new UI affordances.
>
> For more background and motivation see our [paper](https://folkjs.org/live-2025/).
>
> See also the [Contributing Guide](/CONTRIBUTING.md).

Every GUI application must implement accessibility. It's not optional—there are legal requirements (ADA, WCAG, Section 508). Apple, Microsoft, and GNOME all have accessibility frameworks that apps expose. This creates a system-wide, cross-application API that developers can't opt out of. We believe this is under-explored for interoperability and under-advocated for [policy](https://www.eff.org/deeplinks/2019/10/adversarial-interoperability).

## Open Problems

There are 3 things in the way of a11y-as-interop which we are fighting against:

1. pull-based architectures instead of [push-based](https://gitlab.gnome.org/GNOME/at-spi2-core/-/blob/2.58.2/devel-docs/new-protocol.rst) ones, making efficient queries and robust reactivity challenging.
2. bias of a11y towards read-only data, with inconsistent and unreliable writing of data depending on the app.
3. lack of _structured_ i/o. A11y biases towards readable metadata instead of structured types, which in the best-of-all-worlds would be higher-level semantic operations on native data storage like sqlite, automerge or filesystem representations. This goal overlaps heavily with interoperability advocates.

## API Status

| API       | Description                     | Status |
| --------- | ------------------------------- | ------ |
| get       | Get an element by id            | ✅     |
| set       | Set the value of an element     | 🚧     |
| perform   | Perform an action on an element | ✅     |
| discovery | parent, children, element_at    | ✅     |
| observe   | Observe changes to an element   | 🚧     |
| select    | Multi-select elements           | ❌     |
| query     | Query the tree                  | ❌     |
| views     | Simplified tree projections     | ❌     |
| windows   | all, focused, z-order           | ✅     |
| TS client | rpc, occlusion, passthrough     | ✅     |

## Architecture

At its core, Allio is:

1. A **cache** of accessibility state from the OS
2. A **query interface** to that cache
3. A **sync mechanism** that keeps the cache fresh (polling + notifications)
4. An **event stream** for clients to mirror state changes
5. A **JS client** to overlay new UI in/on/around existing apps

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

### Entities

```
Process (1:N)──→ Window (1:N)──→ Element (1:N)──→ Children
```

Each entity has:

- **Data**: The info we expose (`Window`, `Element`)
- **Handle**: OS reference for operations and HashMap key for deduplication

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

### Layer Responsibilities

**Allio (Coordinator)**

- Public API for consumers
- Orchestrates Registry + Platform calls
- Implements `EventHandler` trait for OS notifications

**Registry (Cache + Events)**

- Pure data management
- Maintains indexes (`handle_to_id`, `window_handle_to_id`)
- Maintains tree relationships via `ElementTree`
- Cascading removals
- Emits events when data changes

**Platform (OS Interface)**

- Trait-based abstraction over OS APIs
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

## Element Identity

Elements are deduplicated using their OS handle:

- **Handle** (`ElementHandle`): Wraps macOS `AXUIElement`, implements `Hash + Eq`
- **ElementId**: Our stable u32 ID given to clients

For macOS the handle's `Hash` uses `CFHash` (computed once, cached). The handle's `Eq` uses `CFEqual` for collision resolution.

Registry maintains `handle_to_id: HashMap<Handle, ElementId>` for deduplication.

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

## Public API

### Construction & Events

```rust
pub fn new() -> AllioResult<Self>;
pub fn builder() -> AllioBuilder;  // .exclude_pid(u32).build()
pub fn has_permissions() -> bool;
pub fn subscribe(&self) -> Receiver<Event>;
```

### Element Retrieval

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

## Events

- `upsert_element` → emits `ElementAdded` if truly new
- `update_element` → emits `ElementChanged` if data has changed
- `remove_element` → emits `ElementRemoved` for element + all descendants
- `remove_window` → emits `WindowRemoved` + `ElementRemoved` for all elements
