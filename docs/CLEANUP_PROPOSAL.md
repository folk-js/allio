# Axio Cleanup Proposal

A cleaner architecture with unified APIs and proper layer separation.

---

## 1. Unified Public Element API

### Current (messy)

```rust
// 4+ ways to get an element
pub fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;
pub fn get_cached(&self, id: ElementId) -> Option<Element>;  // convenience
pub fn get_element(&self, id: ElementId) -> Option<Element>;  // deprecated
pub fn refresh_element(&self, id: ElementId) -> AxioResult<Element>;
pub fn fetch_element(&self, id: ElementId) -> AxioResult<Element>;  // deprecated
```

### Proposed (clean)

```rust
/// The ONE way to get an element.
///
/// - `Freshness::Cached` - return cached, might be stale, never fails
/// - `Freshness::Fresh` - always fetch from OS, fails if element gone
/// - `Freshness::MaxAge(d)` - fetch if older than d
pub fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;
```

If we really need a no-error convenience:

```rust
#[inline]
pub fn get_cached(&self, id: ElementId) -> Option<Element> {
    self.get(id, Freshness::Cached).ok().flatten()
}
```

That's it. Remove `get_element`, `fetch_element`, `refresh_element`.

---

## 2. Unified Discovery API

Discovery operations find NEW elements. They always hit the OS.

### Current

```rust
pub fn fetch_element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;
pub fn fetch_children(&self, element_id: ElementId, max_children: usize) -> AxioResult<Vec<Element>>;
pub fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<Element>>;
pub fn fetch_window_root(&self, window_id: WindowId) -> AxioResult<Element>;
pub fn fetch_window_focus(&self, window_id: WindowId) -> AxioResult<(Option<Element>, Option<TextSelection>)>;
```

### Proposed

Keep these - they're honest about what they do. But consider:

```rust
// Maybe rename for consistency with get()
pub fn element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;
pub fn children(&self, id: ElementId) -> AxioResult<Vec<Element>>;  // no max_children?
pub fn parent(&self, id: ElementId) -> AxioResult<Option<Element>>;
pub fn window_root(&self, id: WindowId) -> AxioResult<Element>;
pub fn window_focus(&self, id: WindowId) -> AxioResult<(Option<Element>, Option<TextSelection>)>;
```

Or keep `fetch_` prefix to distinguish from `get()`. Either way, the semantics are clear.

---

## 3. Clean Internal Methods

### Current Problem

```rust
// In Axio:
fn build_element_entry(&self, handle, window_id, pid) -> ElementEntry;  // Makes Platform calls!
fn build_and_register(&self, handle, window_id, pid) -> Option<Element>;  // Calls above + register
fn register_element(&self, entry) -> Option<Element>;  // Registry + watch setup
fn update_element_data(&self, id, data) -> AxioResult<Element>;  // Pass-through to Registry
fn get_element_handle(&self, id) -> AxioResult<(Handle, WindowId, u32)>;  // Extract from Registry
fn get_element_for_refresh(&self, id) -> AxioResult<(Handle, WindowId, u32, bool)>;  // Similar

// In Registry:
fn get_or_insert_element(&mut self, entry) -> (ElementId, bool);  // Insert OR update, confusing
fn update_element_data(&mut self, id, data) -> (bool, bool);  // Returns (exists, changed)
```

### Proposed

**Registry has simple primitives:**

```rust
impl Registry {
    // === Pure queries ===
    fn get(&self, id: ElementId) -> Option<Element>;
    fn get_entry(&self, id: ElementId) -> Option<&ElementEntry>;
    fn find_by_hash(&self, hash: u64, window_id: WindowId) -> Option<ElementId>;

    // === Pure mutations (return what changed) ===

    /// Insert a new element. Caller ensures it doesn't exist.
    fn insert(&mut self, entry: ElementEntry) -> ElementId;

    /// Update element data. Returns true if data changed.
    fn update(&mut self, id: ElementId, data: ElementData) -> bool;

    /// Remove element and descendants. Returns removed IDs.
    fn remove(&mut self, id: ElementId) -> Vec<ElementId>;

    /// Set children for an element.
    fn set_children(&mut self, id: ElementId, children: Vec<ElementId>);
}
```

**Axio orchestrates:**

```rust
impl Axio {
    /// Register element from Platform data. Handles dedup, events, watches.
    pub(crate) fn register(&self, entry: ElementEntry) -> Option<Element> {
        let hash = entry.hash;
        let window_id = entry.data.window_id;

        // Check if exists
        if let Some(existing_id) = self.read(|r| r.find_by_hash(hash, window_id)) {
            // Update existing
            self.write(|r| r.update(existing_id, entry.data));
            return self.read(|r| r.get(existing_id));
        }

        // Insert new
        let id = self.write(|r| r.insert(entry));

        // Setup destruction watch (side effect - Axio's job)
        self.setup_watch(id);

        self.read(|r| r.get(id))
    }
}
```

**ElementEntry construction is a free function:**

```rust
/// Build ElementEntry from Platform handle. Makes OS calls.
/// This is the boundary between Platform and Core.
pub(crate) fn build_entry(
    handle: &Handle,
    window_id: WindowId,
    pid: ProcessId,
) -> ElementEntry {
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

---

## 4. Platform Decoupling via Channels

### Current Problem

Platform callbacks call directly into Axio:

```rust
// In observer.rs:
axio.refresh_element(element_id);
axio.on_element_destroyed(element_id);
axio.fetch_children(element_id, 1000);

// In focus.rs:
axio.build_and_register(handle, window_id, pid);
axio.on_focus_changed(pid, ax_element);
```

This is circular: Platform → Axio → Platform (via refresh calls).

### Proposed: Event-Based Decoupling

**Define platform events:**

```rust
/// Events from OS callbacks. Platform sends these, Axio processes them.
pub(crate) enum PlatformEvent {
    /// Element was destroyed by OS.
    ElementDestroyed {
        element_id: ElementId,
    },

    /// Element attributes may have changed.
    ElementChanged {
        element_id: ElementId,
        notification: Notification,
    },

    /// App focus changed to a new element.
    FocusChanged {
        pid: u32,
        element_handle: Handle,
    },

    /// Text selection changed.
    SelectionChanged {
        pid: u32,
        element_handle: Handle,
        text: String,
        range: Option<(u32, u32)>,
    },

    /// Children structure changed.
    ChildrenChanged {
        element_id: ElementId,
    },
}
```

**Observer takes a channel, not Axio:**

```rust
pub(crate) trait Platform {
    // ...

    /// Create observer that sends events to the channel.
    fn create_observer(
        pid: u32,
        event_tx: Sender<PlatformEvent>,
    ) -> AxioResult<Self::Observer>;
}

pub(crate) trait PlatformObserver: Send + Sync {
    /// Subscribe to app notifications. Events go to the channel.
    fn subscribe_app_notifications(
        &self,
        pid: u32,
        event_tx: Sender<PlatformEvent>,
    ) -> AxioResult<AppNotificationHandle>;

    /// Create element watch. Events go to the channel.
    fn create_watch(
        &self,
        handle: &Self::Handle,
        element_id: ElementId,
        notifications: &[Notification],
        event_tx: Sender<PlatformEvent>,
    ) -> AxioResult<WatchHandle>;
}
```

**OS callback just sends events:**

```rust
// In observer.rs callback:
fn handle_element_notification(
    event_tx: &Sender<PlatformEvent>,
    element_id: ElementId,
    notification: Notification,
    _handle: Handle,
) {
    let event = match notification {
        Notification::Destroyed => PlatformEvent::ElementDestroyed { element_id },
        Notification::ChildrenChanged => PlatformEvent::ChildrenChanged { element_id },
        _ => PlatformEvent::ElementChanged { element_id, notification },
    };
    let _ = event_tx.try_send(event);
}

fn handle_app_notification(
    event_tx: &Sender<PlatformEvent>,
    pid: u32,
    notification: Notification,
    handle: Handle,
) {
    let event = match notification {
        Notification::FocusChanged => PlatformEvent::FocusChanged {
            pid,
            element_handle: handle,
        },
        Notification::SelectionChanged => {
            let (text, range) = handle.fetch_selection().unwrap_or_default();
            PlatformEvent::SelectionChanged {
                pid,
                element_handle: handle,
                text,
                range,
            }
        },
        _ => return,
    };
    let _ = event_tx.try_send(event);
}
```

**Axio processes events:**

```rust
impl Axio {
    /// Process platform events. Called from polling loop.
    pub(crate) fn process_platform_events(&self) {
        while let Ok(event) = self.platform_events_rx.try_recv() {
            self.handle_platform_event(event);
        }
    }

    fn handle_platform_event(&self, event: PlatformEvent) {
        match event {
            PlatformEvent::ElementDestroyed { element_id } => {
                self.write(|r| r.remove(element_id));
            }

            PlatformEvent::ElementChanged { element_id, notification } => {
                // Refresh element from OS
                let _ = self.get(element_id, Freshness::Fresh);
            }

            PlatformEvent::ChildrenChanged { element_id } => {
                // Re-fetch children
                let _ = self.children(element_id);
            }

            PlatformEvent::FocusChanged { pid, element_handle } => {
                // Axio does ALL the work now
                let window_id = self.find_window_for_handle(&element_handle, pid);
                if let Some(window_id) = window_id {
                    let entry = build_entry(&element_handle, window_id, ProcessId(pid));
                    if let Some(element) = self.register(entry) {
                        self.update_focus(pid, element);
                    }
                }
            }

            PlatformEvent::SelectionChanged { pid, element_handle, text, range } => {
                // Similar - Axio handles everything
                let window_id = self.find_window_for_handle(&element_handle, pid);
                if let Some(window_id) = window_id {
                    let entry = build_entry(&element_handle, window_id, ProcessId(pid));
                    if let Some(element) = self.register(entry) {
                        self.update_selection(pid, window_id, element.id, text, range);
                    }
                }
            }
        }
    }
}
```

### Benefits

1. **No circular dependency** - Platform sends events, Axio processes them
2. **Cleaner threading** - Callbacks don't block on Axio operations
3. **Testable** - Can unit test event processing without Platform
4. **Predictable ordering** - Events processed in order during poll loop

---

## 5. Summary: Method Count

### Public API

| Before                    | After                  |
| ------------------------- | ---------------------- |
| `get(id, freshness)`      | `get(id, freshness)`   |
| `get_cached(id)`          | (optional 1-liner)     |
| `get_element(id)`         | ❌ removed             |
| `fetch_element(id)`       | ❌ removed             |
| `refresh_element(id)`     | ❌ removed             |
| `fetch_element_at(x, y)`  | `element_at(x, y)`     |
| `fetch_children(id, max)` | `children(id)`         |
| `fetch_parent(id)`        | `parent(id)`           |
| `fetch_window_root(id)`   | `window_root(id)`      |
| `fetch_window_focus(id)`  | `window_focus(id)`     |
| `get_elements(ids)`       | keep or inline         |
| `get_all_elements()`      | ❌ remove (debug only) |

**Result: 6-7 public element methods** (from 12+)

### Internal API

| Before                      | After                     |
| --------------------------- | ------------------------- |
| `build_element_entry()`     | `build_entry()` free fn   |
| `build_and_register()`      | ❌ removed (inline)       |
| `register_element()`        | `register()`              |
| `update_element_data()`     | inline into `get()`       |
| `get_element_handle()`      | ❌ removed                |
| `get_element_for_refresh()` | ❌ removed                |
| `on_element_destroyed()`    | `handle_platform_event()` |
| `on_element_changed()`      | `handle_platform_event()` |
| `on_focus_changed()`        | `handle_platform_event()` |
| `on_selection_changed()`    | `handle_platform_event()` |

**Result: 3-4 internal element methods** (from 10+)

### Registry

| Before                    | After                         |
| ------------------------- | ----------------------------- |
| `get_or_insert_element()` | `find_by_hash()` + `insert()` |
| `update_element_data()`   | `update()`                    |
| `set_element_children()`  | `set_children()`              |
| `remove_element()`        | `remove()`                    |
| `get_element()`           | `get()`                       |
| `get_element_state()`     | `get_entry()`                 |
| `get_element_data()`      | ❌ removed                    |
| `find_element_by_hash()`  | `find_by_hash()`              |
| + watch methods           | keep                          |

**Result: cleaner primitives, less overloading**

---

## Implementation Order

1. **Add PlatformEvent enum and channel** (new code, no breaking)
2. **Refactor observer callbacks to send events** (update macos/)
3. **Add Axio::process_platform_events()** (new code)
4. **Simplify public API** (remove deprecated, unify get)
5. **Clean up Registry** (split get_or_insert, clarify semantics)
6. **Clean up Axio internals** (remove redundant helpers)
7. **Update TypeScript** (if RPC changes)
