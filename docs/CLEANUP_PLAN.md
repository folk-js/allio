# Axio Cleanup Plan

Revised architecture with unified APIs, clean layer separation, and trait-based Platform decoupling.

---

## Implementation Status

**Phase 1: Foundation** ✅
- [x] Created `PlatformCallbacks` trait (`platform/callbacks.rs`)
- [x] Created `build_entry` free function (`core/queries.rs`)
- [x] Added `find_by_hash_in_window` Registry primitive

**Phase 2: Platform Decoupling** ✅
- [x] Implemented `PlatformCallbacks` for `Axio`
- [x] Updated macOS observer to use trait callbacks
- [x] Updated focus handlers to use trait callbacks

**Phase 3: API Cleanup** ✅
- [x] Added `children(id, freshness)` and `parent(id, freshness)` methods
- [x] Renamed `fetch_screen_size` → `screen_size`
- [x] Renamed `fetch_element_at` → `element_at`
- [x] Renamed `fetch_window_root` → `window_root`
- [x] Renamed `fetch_window_focus` → `window_focus`
- [x] Made internal `fetch_children` and `fetch_parent` `pub(crate)`
- [x] Updated all callers (axio-ws, axio-app)

All tests passing ✅

---

## Goals

1. **Unified public API** - `get(id, freshness)` is THE element retrieval method
2. **Consistent discovery API** - `children`, `parent`, etc. also take freshness
3. **Registry as pure data** - mutations, queries, events, no orchestration
4. **Platform decoupled via trait** - callbacks defined by a trait, not Axio directly
5. **Axio as coordinator** - orchestrates Platform + Registry, handles side effects

---

## 1. Public Element API

### The `get` Method

```rust
/// Get element by ID with specified freshness.
///
/// - `Freshness::Cached` - return cached value, might be stale, never hits OS
/// - `Freshness::Fresh` - always fetch from OS, guaranteed current
/// - `Freshness::MaxAge(d)` - fetch if cached value is older than d
///
/// Returns `Ok(None)` if element doesn't exist in cache (for Cached)
/// or was destroyed (for Fresh).
pub fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;
```

### Discovery Methods

Discovery finds elements by traversing relationships. Takes freshness.

```rust
/// Get children of an element.
///
/// - `Freshness::Cached` - return known children (might be incomplete if never fetched)
/// - `Freshness::Fresh` - fetch from OS, register new children
/// - `Freshness::MaxAge(d)` - fetch if children list is stale
pub fn children(&self, id: ElementId, freshness: Freshness) -> AxioResult<Vec<Element>>;

/// Get parent of an element.
pub fn parent(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;

/// Get root element of a window.
pub fn window_root(&self, id: WindowId) -> AxioResult<Element>;

/// Get focused element and selection in a window.
pub fn window_focus(&self, id: WindowId) -> AxioResult<(Option<Element>, Option<TextSelection>)>;
```

### Hit Testing

Always fresh - you want current truth when hit testing.

```rust
/// Get element at screen coordinates. Always fresh.
pub fn element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;
```

### Complete Public Element API

| Method                    | Freshness    | Notes                        |
| ------------------------- | ------------ | ---------------------------- |
| `get(id, freshness)`      | explicit     | THE element retrieval method |
| `children(id, freshness)` | explicit     | traversal with freshness     |
| `parent(id, freshness)`   | explicit     | traversal with freshness     |
| `element_at(x, y)`        | always Fresh | hit testing                  |
| `window_root(id)`         | always Fresh | window traversal             |
| `window_focus(id)`        | always Fresh | focus query                  |

**6 methods total** (down from 12+)

---

## 2. Registry: Pure Data + Events

Registry is a pure data store that:

- Stores elements, windows, processes
- Maintains indexes (hash → id, tree relationships)
- Emits events when data changes
- Does NOT make OS calls
- Does NOT coordinate (that's Axio's job)

### Element Primitives

```rust
impl Registry {
    // === Queries (pure, no side effects) ===

    /// Get element snapshot by ID.
    pub(crate) fn get(&self, id: ElementId) -> Option<Element>;

    /// Get element entry (internal data) by ID.
    pub(crate) fn get_entry(&self, id: ElementId) -> Option<&ElementEntry>;

    /// Find element ID by platform hash within a window.
    pub(crate) fn find_by_hash(&self, hash: u64, window_id: WindowId) -> Option<ElementId>;

    /// Get children IDs from tree (no OS call).
    pub(crate) fn get_children(&self, id: ElementId) -> Option<&[ElementId]>;

    // === Mutations (emit events) ===

    /// Insert a new element. Emits ElementAdded.
    /// Caller must ensure element doesn't exist (use find_by_hash first).
    pub(crate) fn insert(&mut self, entry: ElementEntry) -> ElementId;

    /// Update element data. Emits ElementChanged if data differs.
    /// Returns true if data changed.
    pub(crate) fn update(&mut self, id: ElementId, data: ElementData) -> bool;

    /// Set children for an element. Updates tree, emits ElementChanged.
    pub(crate) fn set_children(&mut self, id: ElementId, children: Vec<ElementId>);

    /// Remove element and all descendants. Emits ElementRemoved for each.
    /// Returns list of removed IDs.
    pub(crate) fn remove(&mut self, id: ElementId) -> Vec<ElementId>;

    // === Watch management ===

    /// Set watch handle for element.
    pub(crate) fn set_watch(&mut self, id: ElementId, watch: WatchHandle);

    /// Take watch handle (for cleanup).
    pub(crate) fn take_watch(&mut self, id: ElementId) -> Option<WatchHandle>;
}
```

### Key Change: No `get_or_insert`

The current `get_or_insert_element` does too much:

- Looks up by hash
- If exists: updates data
- If new: inserts, resolves orphans, emits event

Split into explicit operations. Axio decides the logic:

```rust
// In Axio:
pub(crate) fn register(&self, entry: ElementEntry) -> Option<Element> {
    let hash = entry.hash;
    let window_id = entry.data.window_id;

    // Check if exists (pure query)
    let existing = self.read(|r| r.find_by_hash(hash, window_id));

    if let Some(existing_id) = existing {
        // Update existing element
        let changed = self.write(|r| r.update(existing_id, entry.data));
        // Note: event already emitted by Registry if changed
        return self.read(|r| r.get(existing_id));
    }

    // Insert new element
    let id = self.write(|r| r.insert(entry));

    // Setup watch (Axio's responsibility)
    self.setup_destruction_watch(id);

    self.read(|r| r.get(id))
}
```

---

## 3. Platform: Trait-Based Callbacks

### The Problem

Current Platform methods take `Axio` directly:

```rust
fn create_observer(pid: u32, axio: Axio) -> Observer;
fn subscribe_app_notifications(&self, pid: u32, axio: Axio) -> Handle;
fn create_watch(&self, handle, id, notifs, axio: Axio) -> Handle;
```

Then callbacks call back into Axio's full API:

```rust
// In callbacks:
axio.refresh_element(element_id);
axio.build_and_register(handle, window_id, pid);
axio.get_element_by_hash(hash, pid);
```

This creates circular dependency and unclear boundaries.

### Solution: Callback Trait

Define a narrow trait for what Platform callbacks need:

```rust
/// Callbacks from Platform to Core when OS events fire.
///
/// This trait defines the ONLY interface Platform uses to communicate
/// with Core. Axio implements this trait.
pub(crate) trait PlatformCallbacks: Send + Sync + 'static {
    /// Called when an element is destroyed by the OS.
    fn on_element_destroyed(&self, element_id: ElementId);

    /// Called when an element's value/title/bounds changed.
    fn on_element_changed(&self, element_id: ElementId, notification: Notification);

    /// Called when an element's children structure changed.
    fn on_children_changed(&self, element_id: ElementId);

    /// Called when app focus changes.
    /// Provides the raw handle - callback implementation does registration.
    fn on_focus_changed(&self, pid: u32, focused_handle: Handle);

    /// Called when text selection changes.
    fn on_selection_changed(
        &self,
        pid: u32,
        element_handle: Handle,
        text: String,
        range: Option<(u32, u32)>,
    );
}
```

### Platform Takes the Trait

```rust
pub(crate) trait Platform {
    type Handle: PlatformHandle;
    type Observer: PlatformObserver<Handle = Self::Handle>;

    // ... existing pure methods ...

    /// Create observer. Takes callback trait instead of Axio.
    fn create_observer<C: PlatformCallbacks>(
        pid: u32,
        callbacks: Arc<C>,
    ) -> AxioResult<Self::Observer>;
}

pub(crate) trait PlatformObserver: Send + Sync {
    type Handle: PlatformHandle;

    /// Subscribe to app notifications.
    fn subscribe_app_notifications<C: PlatformCallbacks>(
        &self,
        pid: u32,
        callbacks: Arc<C>,
    ) -> AxioResult<AppNotificationHandle>;

    /// Create element watch.
    fn create_watch<C: PlatformCallbacks>(
        &self,
        handle: &Self::Handle,
        element_id: ElementId,
        notifications: &[Notification],
        callbacks: Arc<C>,
    ) -> AxioResult<WatchHandle>;
}
```

### Axio Implements the Trait

```rust
impl PlatformCallbacks for Axio {
    fn on_element_destroyed(&self, element_id: ElementId) {
        self.write(|r| r.remove(element_id));
    }

    fn on_element_changed(&self, element_id: ElementId, notification: Notification) {
        // Refresh element from OS
        let _ = self.get(element_id, Freshness::Fresh);
    }

    fn on_children_changed(&self, element_id: ElementId) {
        // Re-fetch children
        let _ = self.children(element_id, Freshness::Fresh);
    }

    fn on_focus_changed(&self, pid: u32, focused_handle: Handle) {
        // Find window for this element
        let window_id = self.find_window_for_handle(&focused_handle, pid);
        let Some(window_id) = window_id else { return };

        // Build and register element
        let entry = build_entry(&focused_handle, window_id, ProcessId(pid));
        let Some(element) = self.register(entry) else { return };

        // Update focus state
        if element.focused == Some(true) {
            self.write(|r| r.set_focused_element(pid, element.clone()));
        }
    }

    fn on_selection_changed(
        &self,
        pid: u32,
        element_handle: Handle,
        text: String,
        range: Option<(u32, u32)>,
    ) {
        let window_id = self.find_window_for_handle(&element_handle, pid);
        let Some(window_id) = window_id else { return };

        let entry = build_entry(&element_handle, window_id, ProcessId(pid));
        let Some(element) = self.register(entry) else { return };

        self.write(|r| r.set_selection(pid, window_id, element.id, text, range));
    }
}
```

### Benefits

1. **Narrow interface** - Platform only sees 5 callback methods, not all of Axio
2. **Same callback characteristics** - Still synchronous, same threading model
3. **Testable** - Can mock `PlatformCallbacks` for testing
4. **Clear contract** - The trait documents exactly what Platform can do

---

## 4. Element Building

### Current Problem

`build_element_entry` is an Axio method that makes Platform calls:

```rust
// In Axio:
fn build_element_entry(&self, handle: Handle, window_id: WindowId, pid: u32) -> ElementEntry {
    let attrs = handle.fetch_attributes();  // Platform call!
    let parent = handle.fetch_parent();     // Platform call!
    // ...
}
```

This is called from Platform callbacks, so Platform→Axio→Platform.

### Solution: Free Function

Element building is just data transformation. Make it a free function:

```rust
/// Build ElementEntry from a Platform handle.
///
/// Makes OS calls via the handle, but doesn't touch Axio or Registry.
/// This is the boundary between Platform data and Core data.
pub(crate) fn build_entry(
    handle: &Handle,
    window_id: WindowId,
    pid: ProcessId,
) -> ElementEntry {
    use crate::accessibility::Role;

    let attrs = handle.fetch_attributes();
    let parent_handle = handle.fetch_parent();
    let hash = handle.element_hash();

    let is_root = parent_handle
        .as_ref()
        .map(|p| p.fetch_attributes().role == Role::Application)
        .unwrap_or(false);

    let parent_hash = if is_root {
        None
    } else {
        parent_handle.as_ref().map(|p| p.element_hash())
    };

    let data = ElementData::from_attributes(
        ElementId::new(),
        window_id,
        pid,
        is_root,
        attrs,
    );

    ElementEntry::new(data, handle.clone(), hash, parent_hash)
}
```

Called from Axio's callback implementation, not from Platform.

---

## 5. Internal Method Cleanup

### Current Axio Internal Methods

| Method                      | Issue                                   |
| --------------------------- | --------------------------------------- |
| `build_element_entry()`     | Makes Platform calls, should be free fn |
| `build_and_register()`      | Wraps above, unclear value              |
| `register_element()`        | Good, keep as `register()`              |
| `update_element_data()`     | Pass-through, inline                    |
| `get_element_handle()`      | Helper for refresh, keep                |
| `get_element_for_refresh()` | Similar to above, merge                 |
| `on_element_destroyed()`    | Move to PlatformCallbacks impl          |
| `on_element_changed()`      | Move to PlatformCallbacks impl          |
| `on_focus_changed()`        | Move to PlatformCallbacks impl          |
| `on_selection_changed()`    | Move to PlatformCallbacks impl          |

### Target Internal Methods

```rust
impl Axio {
    // === Registration ===

    /// Register element from entry. Handles dedup, events, watches.
    pub(crate) fn register(&self, entry: ElementEntry) -> Option<Element>;

    // === Helpers ===

    /// Get handle and metadata for an element.
    pub(crate) fn get_handle(&self, id: ElementId) -> AxioResult<(Handle, WindowId, ProcessId)>;

    /// Find window that contains an element handle.
    pub(crate) fn find_window_for_handle(&self, handle: &Handle, pid: u32) -> Option<WindowId>;

    // === Watch management ===

    /// Setup destruction watch for element.
    pub(crate) fn setup_destruction_watch(&self, id: ElementId);
}
```

**4 internal methods** (down from 10+)

---

## 6. Summary

### Layer Responsibilities

| Layer        | Responsibility                     | Does NOT                        |
| ------------ | ---------------------------------- | ------------------------------- |
| **Platform** | OS calls, sends callbacks          | Access Registry, coordinate     |
| **Registry** | Data storage, indexes, events      | Make OS calls, coordinate       |
| **Axio**     | Coordination, implements callbacks | Direct OS calls (uses Platform) |

### Method Counts

| Layer             | Before          | After              |
| ----------------- | --------------- | ------------------ |
| Axio Public       | 12+             | 6                  |
| Axio Internal     | 10+             | 4                  |
| Registry Element  | ~10             | 8                  |
| PlatformCallbacks | (mixed in Axio) | 5 (separate trait) |

### Data Flow

```
OS Event
    ↓
Platform callback (observer.rs, focus.rs)
    ↓
PlatformCallbacks trait method
    ↓
Axio implementation
    ├── build_entry() [free fn, makes Platform calls]
    ├── self.register() [coordinates Registry]
    └── Registry.insert/update/remove [pure data + events]
```

---

## Implementation Order

### Phase 1: Define Structures

1. Create `PlatformCallbacks` trait
2. Create `build_entry` free function
3. Add new Registry primitives (`find_by_hash`, `insert`, `update`)

### Phase 2: Implement Trait

4. Implement `PlatformCallbacks` for Axio
5. Update Platform observer to use trait instead of Axio
6. Update macOS callbacks (observer.rs, focus.rs)

### Phase 3: Unify Public API

7. Add freshness to `children`, `parent`
8. Rename `fetch_*` to drop prefix
9. Remove deprecated methods

### Phase 4: Cleanup

10. Remove old Registry `get_or_insert_element`
11. Remove old Axio internal methods
12. Update tests
13. Update TypeScript client if needed

---

## Files to Change

| File                         | Changes                                                                           |
| ---------------------------- | --------------------------------------------------------------------------------- |
| `platform/traits.rs`         | Add `PlatformCallbacks` trait, update observer traits                             |
| `platform/macos/observer.rs` | Use trait instead of Axio, simplify callbacks                                     |
| `platform/macos/focus.rs`    | Move logic to Axio's trait impl                                                   |
| `core/state.rs`              | Add `find_by_hash`, `insert`, `update`; eventually remove `get_or_insert_element` |
| `core/queries.rs`            | Unify public API, add `build_entry` fn, cleanup internal methods                  |
| `core/mutations.rs`          | Move `on_*` handlers to trait impl                                                |
| `core/mod.rs`                | Implement `PlatformCallbacks` for Axio                                            |
