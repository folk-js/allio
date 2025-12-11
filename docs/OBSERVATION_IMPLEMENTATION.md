# Observation System Implementation Plan

A multi-phase plan to implement the honest freshness model and observation system.

---

## Overview

**Phase 1: The Honest Model** - Comprehensive refactor to the freshness-based API. Eliminate confusion between get/fetch, simplify data flow, review all invariants. This is foundational work that makes Phase 2 tractable.

**Phase 2: Observation System** - Add `observe()` and related machinery on top of the clean foundation. Should be relatively small and independently testable.

---

## Phase 1: The Honest Model

### Goals

1. **Honest freshness API**: Replace the confusing get/fetch distinction with `get(id, freshness)` where freshness is explicit.

2. **Clean layer separation**:

   - **Registry** = pure data store (get/insert/update/remove, maintains invariants, NO side effects)
   - **Platform** = pure OS interface (fetch\_\* returns data, NO state mutation)
   - **Axio** = orchestrator (coordinates Platform + Registry, owns ALL side effects)

3. **Eliminate element_ops**: This file is a symptom of unclear layer boundaries. Its logic should be distributed to the appropriate layers.

4. **TS client compatibility**: Minimal changes required (add freshness option, keep existing methods).

### 1.1: Define the Freshness Type

**File:** `crates/axio/src/types/freshness.rs` (new)

```rust
use std::time::Duration;

/// How fresh a value should be when retrieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Use cached value. No OS calls. Might be arbitrarily stale.
    Cached,

    /// Always fetch from OS. Guaranteed current.
    Fresh,

    /// Value must be at most this old. Fetch if stale.
    MaxAge(Duration),
}

impl Freshness {
    /// Convenience for common max ages.
    pub const fn max_age_ms(ms: u64) -> Self {
        Self::MaxAge(Duration::from_millis(ms))
    }
}

impl Default for Freshness {
    fn default() -> Self {
        Self::Cached
    }
}
```

**Tasks:**

- [ ] Create `freshness.rs`
- [ ] Add to `types/mod.rs`
- [ ] Export from `lib.rs`

---

### 1.2: Add Timestamp Tracking to ElementEntry

Elements need to track when they were last refreshed.

**File:** `crates/axio/src/core/state.rs`

```rust
pub(crate) struct ElementEntry {
    pub(crate) data: ElementData,
    pub(crate) handle: Handle,
    pub(crate) hash: u64,
    pub(crate) parent_hash: Option<u64>,
    pub(crate) watch: Option<WatchHandle>,
    pub(crate) last_refreshed: Instant,  // NEW
}
```

**Tasks:**

- [ ] Add `last_refreshed: Instant` to `ElementEntry`
- [ ] Set `last_refreshed = Instant::now()` on insert and refresh
- [ ] Add helper: `fn is_stale(&self, max_age: Duration) -> bool`

---

### 1.3: Unified `get` API on Axio

Replace `get_element` + `fetch_element` with a single method.

**Current API:**

```rust
// Cache only
fn get_element(&self, id: ElementId) -> Option<Element>;

// Always OS
fn fetch_element(&self, id: ElementId) -> AxioResult<Element>;
```

**New API:**

```rust
/// Get element with specified freshness.
fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>>;

/// Convenience: get from cache (might be stale).
fn get_cached(&self, id: ElementId) -> Option<Element> {
    self.get(id, Freshness::Cached).ok().flatten()
}
```

**Implementation:**

```rust
fn get(&self, id: ElementId, freshness: Freshness) -> AxioResult<Option<Element>> {
    match freshness {
        Freshness::Cached => {
            Ok(self.read(|r| r.get_element(id)))
        }
        Freshness::Fresh => {
            self.refresh_element(id)?;
            Ok(self.read(|r| r.get_element(id)))
        }
        Freshness::MaxAge(max_age) => {
            let needs_refresh = self.read(|r| {
                r.get_element_entry(id)
                    .map(|e| e.is_stale(max_age))
                    .unwrap_or(false)
            });
            if needs_refresh {
                self.refresh_element(id)?;
            }
            Ok(self.read(|r| r.get_element(id)))
        }
    }
}
```

**Tasks:**

- [ ] Add `get(&self, id, freshness)` method
- [ ] Add `get_cached(&self, id)` convenience method
- [ ] Deprecate or remove `get_element` and `fetch_element`
- [ ] Update all internal callers

---

### 1.4: Unified Discovery Methods

Methods that discover elements from OS should use consistent naming.

**Current:**

```rust
fn fetch_element_at(&self, x, y) -> AxioResult<Option<Element>>;
fn fetch_children(&self, id, max) -> AxioResult<Vec<Element>>;
fn fetch_parent(&self, id) -> AxioResult<Option<Element>>;
fn fetch_window_root(&self, id) -> AxioResult<Element>;
```

**New (keep `fetch_` for discovery, it's honest):**

```rust
// Discovery operations - always hit OS, always return fresh
fn fetch_element_at(&self, x, y) -> AxioResult<Option<Element>>;
fn fetch_children(&self, id, max) -> AxioResult<Vec<Element>>;
fn fetch_parent(&self, id) -> AxioResult<Option<Element>>;
fn fetch_window_root(&self, id) -> AxioResult<Element>;
```

These stay as `fetch_` because they're discovering new elements, not reading cached ones. The distinction is:

- `get(id, freshness)` - read element by ID (might refresh)
- `fetch_*` - discover elements (always OS call)

**Tasks:**

- [ ] Document the get vs fetch distinction clearly
- [ ] Ensure all fetch methods set `last_refreshed` on returned elements

---

### 1.5: Establish Clean Layer Separation

The architecture should have three clearly separated layers:

```
┌─────────────────────────────────────────────────────────────────┐
│                         Axio (Orchestrator)                      │
│  - Coordinates Platform and Registry                            │
│  - Manages side effects (watches, events)                       │
│  - Public API lives here                                        │
└───────────────────────────┬─────────────────────────────────────┘
                            │
         ┌──────────────────┴──────────────────┐
         ▼                                     ▼
┌─────────────────────────────┐   ┌─────────────────────────────┐
│   Registry (Pure Data)       │   │   Platform (Pure OS Calls)  │
│   - get/insert/update/remove │   │   - fetch_* returns data    │
│   - Maintains invariants     │   │   - NO side effects         │
│   - NO OS calls              │   │   - NO state mutation       │
│   - NO side effects          │   │                             │
└─────────────────────────────┘   └─────────────────────────────┘
```

**Key principles:**

1. **Registry is pure data** - Only manages in-memory state. Methods like `insert_element`, `update_element`, `remove_element`, `get_element`. Maintains indexes and tree invariants. NO OS calls. NO event emission (Axio handles that).

2. **Platform is pure OS interface** - All `fetch_*` methods return data, never mutate state. `fetch_children(handle)` returns `Vec<Handle>`. `fetch_attributes(handle)` returns `ElementAttributes`. Pure data in, pure data out.

3. **Axio orchestrates** - Takes data from Platform, decides what to do with it, updates Registry, emits events, manages watches. This is where side effects live.

**Implications for element_ops:**

`element_ops` currently mixes all three concerns. It should be eliminated by:

- Moving pure OS calls to `Platform` trait methods
- Moving pure data operations to `Registry` methods
- Moving orchestration to `Axio` methods

---

### 1.6: Refactor Platform Trait (Pure OS Calls)

Platform methods should be pure: data in, data out, no side effects.

**Current Platform trait issues:**

- `create_observer(pid, axio)` takes Axio reference (side effect coupling)
- `PlatformHandle::fetch_*` methods are good (pure data)

**New Platform trait design:**

```rust
/// Platform-global operations. All methods are pure (no side effects).
pub(crate) trait Platform {
    type Handle: PlatformHandle;

    // === Pure queries ===
    fn has_permissions() -> bool;
    fn fetch_windows(exclude_pid: Option<u32>) -> Vec<WindowData>;
    fn fetch_screen_size() -> (f64, f64);
    fn fetch_mouse_position() -> Point;
    fn fetch_window_handle(window: &WindowData) -> Option<Self::Handle>;
    fn app_element(pid: u32) -> Self::Handle;
    fn fetch_focused_element(app_handle: &Self::Handle) -> Option<Self::Handle>;
}

/// Per-element operations. All methods are pure (no side effects).
pub(crate) trait PlatformHandle: Clone + Send + Sync + 'static {
    // === Pure queries (return data, no state mutation) ===
    fn fetch_children(&self) -> Vec<Self>;
    fn fetch_parent(&self) -> Option<Self>;
    fn fetch_attributes(&self) -> ElementAttributes;
    fn element_hash(&self) -> u64;
    fn fetch_element_at_position(&self, x: f64, y: f64) -> Option<Self>;
    fn fetch_selection(&self) -> Option<(String, Option<(u32, u32)>)>;

    // === Pure mutations (affect OS, not our state) ===
    fn set_value(&self, value: &Value) -> AxioResult<()>;
    fn perform_action(&self, action: &str) -> AxioResult<()>;
}
```

**Observer creation moves to Axio:**

The observer creation involves side effects (callbacks that mutate state), so it stays in Axio, not Platform. Platform only provides the raw OS primitives.

```rust
impl Axio {
    /// Create observer for a process. This is Axio's responsibility
    /// because observers have callbacks that mutate state.
    fn create_process_observer(&self, pid: u32) -> AxioResult<Observer> {
        // Uses platform primitives but manages the callbacks
    }
}
```

**Tasks:**

- [ ] Audit `Platform` trait - ensure all methods are pure
- [ ] Audit `PlatformHandle` trait - ensure all methods are pure
- [ ] Move observer creation logic to Axio
- [ ] Remove `Axio` parameter from Platform traits where possible

---

### 1.7: Refactor Registry (Pure Data Store)

Registry should be a pure data store with no OS calls and no event emission.

**Current Registry issues:**

- `get_or_insert_element` emits events (side effect)
- Methods mix "query" and "mutation" concerns

**New Registry design:**

```rust
/// Pure data store. NO OS calls. NO event emission.
/// Axio wraps these methods and handles events.
pub(crate) struct Registry {
    elements: HashMap<ElementId, ElementEntry>,
    hash_to_element: HashMap<u64, ElementId>,
    tree: ElementTree,
    windows: HashMap<WindowId, WindowEntry>,
    processes: HashMap<ProcessId, ProcessEntry>,
    // ... indexes ...
}

impl Registry {
    // === Element operations (pure, return what changed) ===

    /// Insert element. Returns (id, was_existing).
    /// Does NOT emit events - caller handles that.
    fn insert_element(&mut self, entry: ElementEntry) -> (ElementId, bool);

    /// Update element data. Returns true if changed.
    fn update_element(&mut self, id: ElementId, data: ElementData) -> bool;

    /// Remove element and descendants. Returns removed IDs.
    fn remove_element(&mut self, id: ElementId) -> Vec<ElementId>;

    /// Get element (pure query).
    fn get_element(&self, id: ElementId) -> Option<Element>;

    /// Find by hash (pure query).
    fn find_by_hash(&self, hash: u64, window_id: WindowId) -> Option<ElementId>;

    // === Tree operations ===
    fn set_children(&mut self, parent: ElementId, children: Vec<ElementId>);
    fn add_child(&mut self, parent: ElementId, child: ElementId);

    // === Window/Process operations (similar pattern) ===
    fn insert_window(&mut self, entry: WindowEntry) -> bool;
    fn update_window(&mut self, id: WindowId, data: Window) -> bool;
    fn remove_window(&mut self, id: WindowId) -> Vec<ElementId>;  // Returns removed element IDs
}
```

**Event emission moves to Axio:**

```rust
impl Axio {
    fn register_element(&self, entry: ElementEntry) -> Option<Element> {
        // 1. Insert into registry (pure)
        let (id, was_existing) = self.write(|r| r.insert_element(entry));

        // 2. Emit event (side effect, Axio's responsibility)
        if !was_existing {
            let element = self.read(|r| r.get_element(id));
            if let Some(elem) = &element {
                self.emit(Event::ElementAdded { element: elem.clone() });
            }
        }

        // 3. Setup watch (side effect)
        // ...

        element
    }
}
```

**Invariants Registry maintains:**

| Collection           | Invariant                  | Maintained by                        |
| -------------------- | -------------------------- | ------------------------------------ |
| `elements`           | Valid entries with handles | `insert_element` validates           |
| `hash_to_element`    | Consistent with elements   | `insert_element`, `remove_element`   |
| `tree`               | Bidirectional parent-child | `ElementTree` methods                |
| `waiting_for_parent` | Orphans by hash            | `insert_element` (orphan resolution) |

**Tasks:**

- [ ] Remove event emission from Registry methods
- [ ] Rename methods to be clearer (`get_or_insert` → `insert`)
- [ ] Return "what changed" from mutation methods
- [ ] Move event emission to Axio wrapper methods
- [ ] Document invariants with doc comments

---

### 1.8: Eliminate element_ops

With the clean separation above, `element_ops` becomes unnecessary.

**What element_ops currently does:**

| Function                     | Where it goes                                     |
| ---------------------------- | ------------------------------------------------- |
| `build_element_state`        | Inline in Axio methods (just data construction)   |
| `build_and_register_element` | `Axio::register_from_handle`                      |
| `fetch_children`             | `Axio::fetch_children` (uses Platform + Registry) |
| `fetch_parent`               | `Axio::fetch_parent`                              |
| `fetch_element`              | `Axio::refresh_element`                           |
| `fetch_window_root`          | `Axio::fetch_window_root`                         |
| `fetch_element_at_position`  | `Axio::fetch_element_at`                          |
| `fetch_focus`                | `Axio::fetch_focus`                               |

**Example: fetch_children after refactor:**

```rust
impl Axio {
    pub fn fetch_children(&self, parent_id: ElementId, max: usize) -> AxioResult<Vec<Element>> {
        // 1. Get handle from registry (pure query)
        let (handle, window_id, pid) = self.read(|r| {
            let entry = r.get_element_entry(parent_id)?;
            Some((entry.handle.clone(), entry.data.window_id, entry.data.pid))
        }).ok_or(AxioError::ElementNotFound(parent_id))?;

        // 2. Fetch from platform (pure OS call)
        let child_handles = handle.fetch_children();

        // 3. Register each child (orchestration with side effects)
        let mut children = Vec::new();
        for child_handle in child_handles.into_iter().take(max) {
            if let Some(child) = self.register_from_handle(child_handle, window_id, pid.0) {
                children.push(child);
            }
        }

        // 4. Update tree (registry mutation)
        let child_ids: Vec<_> = children.iter().map(|c| c.id).collect();
        self.write(|r| r.set_children(parent_id, child_ids));

        // 5. Emit parent changed event (side effect)
        if let Some(parent) = self.read(|r| r.get_element(parent_id)) {
            self.emit(Event::ElementChanged { element: parent });
        }

        Ok(children)
    }

    /// Register element from platform handle. Internal helper.
    fn register_from_handle(&self, handle: Handle, window_id: WindowId, pid: u32) -> Option<Element> {
        // Fetch attributes (pure platform call)
        let attrs = handle.fetch_attributes();
        let parent_handle = handle.fetch_parent();
        let hash = handle.element_hash();
        let parent_hash = parent_handle.as_ref().map(|p| p.element_hash());
        let is_root = parent_handle.as_ref()
            .map(|p| p.fetch_attributes().role == Role::Application)
            .unwrap_or(false);

        // Build entry (pure data construction)
        let entry = ElementEntry::new(
            ElementData::from_attributes(ElementId::new(), window_id, ProcessId(pid), is_root, attrs),
            handle.clone(),
            hash,
            if is_root { None } else { parent_hash },
        );

        // Insert into registry (pure, returns what happened)
        let (id, was_existing) = self.write(|r| r.insert_element(entry));

        // Emit event if new (side effect)
        let element = self.read(|r| r.get_element(id));
        if !was_existing {
            if let Some(elem) = &element {
                self.emit(Event::ElementAdded { element: elem.clone() });
            }
            // Setup destruction watch (side effect)
            self.setup_destruction_watch(id, ProcessId(pid), &handle);
        }

        element
    }
}
```

**Tasks:**

- [ ] Implement `Axio::register_from_handle` as shown above
- [ ] Refactor each `fetch_*` method to use Platform + Registry + emit events
- [ ] Delete `platform/element_ops.rs`
- [ ] Update `platform/mod.rs`

---

### 1.9: Review Data Flow

With the clean separation, data flow becomes clear:

**Layer responsibilities:**

```
Platform (fetch)     →  Pure data  →  Axio (orchestrate)  →  Registry (store)
                                            ↓
                                      Events emitted
```

**Inbound flows:**

1. **Discovery (user-initiated):**

   ```
   Axio::fetch_children(id)
   → Registry::get_element_entry(id)     [get handle]
   → Platform::fetch_children(handle)    [OS call, pure data]
   → Axio::register_from_handle(...)     [for each child]
     → Platform::fetch_attributes(...)   [OS call, pure data]
     → Registry::insert_element(...)     [pure insert]
     → Axio::emit(ElementAdded)          [side effect]
     → Axio::setup_watch(...)            [side effect]
   → Registry::set_children(...)         [pure update]
   → Axio::emit(ElementChanged)          [side effect]
   ```

2. **Polling (background):**

   ```
   poll_iteration()
   → Platform::fetch_windows()           [OS call, pure data]
   → Platform::fetch_mouse_position()    [OS call, pure data]
   → Axio::sync_windows(data)            [orchestration]
     → Registry::insert/update/remove    [pure mutations]
     → Axio::emit(Window*)               [side effects]
   ```

3. **Notifications (OS-initiated):**
   ```
   OS callback fires
   → Axio::on_element_destroyed(id)      [orchestration]
     → Registry::remove_element(id)      [pure mutation]
     → Axio::emit(ElementRemoved)        [side effect]
   ```

**Outbound flows:**

1. **Mutations:**
   ```
   Axio::set_value(id, value)
   → Registry::get_element_entry(id)     [get handle]
   → Platform::set_value(handle, value)  [OS call]
   ```

**Tasks:**

- [ ] Verify all flows follow this pattern
- [ ] Document in ARCHITECTURE.md
- [ ] Ensure no Platform→Registry or Registry→Platform calls

---

### 1.9: Update TypeScript Client

Minimal changes to match the Rust API.

**Current:**

```typescript
axio.focusedElement; // always fresh (tier 1)
await axio.fetchChildren(id); // always fresh
// no explicit freshness
```

**New (conceptually same, maybe add freshness option later):**

```typescript
axio.focusedElement; // always fresh (tier 1)
await axio.fetchChildren(id); // always fresh (discovery)
axio.get(id); // cached
axio.get(id, { freshness: "fresh" }); // explicit
```

**Tasks:**

- [ ] Add `get(id, options?)` method to client
- [ ] Add `Freshness` type to client
- [ ] Keep existing methods working (backwards compat)
- [ ] Update RPC protocol if needed

---

### 1.10: Testing Phase 1

**Tasks:**

- [ ] Test `get(id, Freshness::Cached)` returns cached value
- [ ] Test `get(id, Freshness::Fresh)` makes OS call
- [ ] Test `get(id, Freshness::MaxAge(100ms))` respects age
- [ ] Test all fetch\_\* methods still work
- [ ] Test events still fire correctly
- [ ] Test watch/unwatch still works
- [ ] Verify no regressions in existing functionality

---

## Phase 2: Observation System

### Goal

Add `observe()` function that keeps elements fresh according to a freshness spec. System handles notification vs polling internally.

### 2.1: Observation Handle Type

**File:** `crates/axio/src/core/observation.rs` (new)

```rust
/// Handle to an active observation. Observation stops when dropped.
pub struct Observation {
    id: ObservationId,
    axio: Axio,
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.axio.stop_observation(self.id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObservationId(u64);
```

**Tasks:**

- [ ] Create `observation.rs`
- [ ] Define `Observation` handle
- [ ] Define `ObservationId`

---

### 2.2: Observation State in Registry

Track active observations.

```rust
pub(crate) struct ObservationEntry {
    element_id: ElementId,
    attrs: Vec<Attribute>,
    freshness: Freshness,
    /// Notification handles (if using notifications)
    notifications: Vec<NotificationHandle>,
    /// Poll interval (if polling)
    poll_interval: Option<Duration>,
    /// Last poll time
    last_polled: Instant,
}
```

**In Registry:**

```rust
pub(crate) struct Registry {
    // ... existing fields ...

    /// Active observations
    observations: HashMap<ObservationId, ObservationEntry>,

    /// Element → observations mapping (for change dispatch)
    element_observations: HashMap<ElementId, Vec<ObservationId>>,
}
```

**Tasks:**

- [ ] Add `ObservationEntry` struct
- [ ] Add `observations` to Registry
- [ ] Add `element_observations` index
- [ ] Add methods: `add_observation`, `remove_observation`, `get_observations_for_element`

---

### 2.3: The observe() Method

```rust
impl Axio {
    /// Observe an element's attributes with specified freshness.
    ///
    /// While the observation is active:
    /// - Element is kept fresh according to `freshness`
    /// - Changes emit events
    ///
    /// Observation stops when the handle is dropped.
    pub fn observe(
        &self,
        id: ElementId,
        attrs: &[Attribute],
        freshness: Freshness,
    ) -> AxioResult<Observation>;
}
```

**Implementation sketch:**

```rust
fn observe(&self, id: ElementId, attrs: &[Attribute], freshness: Freshness) -> AxioResult<Observation> {
    // Verify element exists
    let element = self.get(id, Freshness::Cached)?
        .ok_or(AxioError::ElementNotFound(id))?;

    // Determine strategy for each attribute
    let strategies: Vec<_> = attrs.iter()
        .map(|a| strategy_for(element.role, *a))
        .collect();

    // Set up notifications where available
    let notifications = self.setup_observation_notifications(id, &strategies)?;

    // Determine poll interval
    let poll_interval = self.compute_poll_interval(&strategies, freshness);

    // Register observation
    let obs_id = self.write(|r| {
        r.add_observation(ObservationEntry {
            element_id: id,
            attrs: attrs.to_vec(),
            freshness,
            notifications,
            poll_interval,
            last_polled: Instant::now(),
        })
    });

    Ok(Observation { id: obs_id, axio: self.clone() })
}
```

**Tasks:**

- [ ] Implement `Axio::observe`
- [ ] Implement `Axio::stop_observation`
- [ ] Implement `setup_observation_notifications`
- [ ] Implement `compute_poll_interval`

---

### 2.4: Strategy Matrix

Start with a minimal matrix, expand through research.

```rust
/// Observation strategy for an attribute.
pub(crate) struct ObservationStrategy {
    /// OS notification to use (if any)
    pub notification: Option<Notification>,
    /// Whether the notification is reliable
    pub notification_reliable: bool,
}

pub(crate) fn strategy_for(role: Role, attr: Attribute) -> ObservationStrategy {
    use Attribute::*;
    use Role::*;

    match (role, attr) {
        // Value notifications
        (TextField | TextArea, Value) => ObservationStrategy {
            notification: Some(Notification::ValueChanged),
            notification_reliable: true,
        },
        (StaticText, Value) => ObservationStrategy {
            notification: None,  // StaticText doesn't emit ValueChanged
            notification_reliable: false,
        },

        // Bounds - unreliable
        (_, Bounds) => ObservationStrategy {
            notification: Some(Notification::BoundsChanged),
            notification_reliable: false,  // Needs polling backup
        },

        // Children
        (_, Children) => ObservationStrategy {
            notification: Some(Notification::ChildrenChanged),
            notification_reliable: true,  // Seems reliable
        },

        // Default: assume unreliable
        _ => ObservationStrategy {
            notification: None,
            notification_reliable: false,
        },
    }
}
```

**Tasks:**

- [ ] Create initial strategy matrix
- [ ] Document known reliable/unreliable notifications
- [ ] Add TODO for research to expand matrix

---

### 2.5: Observation Polling Loop

Poll observed elements that need it.

```rust
impl Axio {
    /// Called from polling loop. Refreshes observed elements that are due.
    pub(crate) fn poll_observations(&self) {
        let now = Instant::now();

        // Get observations that need polling
        let due: Vec<(ObservationId, ElementId)> = self.read(|r| {
            r.observations.iter()
                .filter(|(_, obs)| {
                    obs.poll_interval.is_some_and(|interval| {
                        now.duration_since(obs.last_polled) >= interval
                    })
                })
                .map(|(id, obs)| (*id, obs.element_id))
                .collect()
        });

        // Refresh each (outside lock)
        for (obs_id, elem_id) in due {
            if let Err(e) = self.refresh_element(elem_id) {
                log::debug!("Failed to refresh observed element {elem_id}: {e}");
            }

            // Update last_polled
            self.write(|r| {
                if let Some(obs) = r.observations.get_mut(&obs_id) {
                    obs.last_polled = now;
                }
            });
        }
    }
}
```

**Integrate into polling loop:**

```rust
fn poll_iteration(axio: &Axio, config: &AxioOptions) {
    // Existing: mouse, windows, focus
    axio.sync_mouse(pos);
    axio.sync_windows(windows, skip_removal);
    axio.sync_focused_window(focused_window_id);

    // NEW: observation polling
    axio.poll_observations();
}
```

**Tasks:**

- [ ] Implement `poll_observations`
- [ ] Add to `poll_iteration`
- [ ] Consider: should observation polling be separate from window polling?

---

### 2.6: Change Events for Observations

When an observed element changes, emit events.

**Already handled:** Registry emits `ElementChanged` on any change.

**Additional consideration:** Should we emit a more specific event for observation changes?

```rust
Event::ObservationChanged {
    observation_id: ObservationId,
    element: Element,
    changed_attrs: Vec<Attribute>,
}
```

**Tasks:**

- [ ] Decide: use existing `ElementChanged` or add `ObservationChanged`
- [ ] If new event type, add to Event enum
- [ ] Update TypeScript types

---

### 2.7: TypeScript Client observe()

```typescript
interface ObserveOptions {
  attrs?: Attribute[];
  freshness?: Freshness;
}

interface Observation {
  readonly element: Element;
  readonly staleness: number;

  on(
    event: "change",
    fn: (element: Element, changed: Attribute[]) => void
  ): void;
  on(event: "removed", fn: () => void): void;

  dispose(): void;
}

class AXIO {
  observe(id: ElementId, options?: ObserveOptions): Observation;
}
```

**Tasks:**

- [ ] Add `observe` method to client
- [ ] Add `Observation` class
- [ ] Add RPC messages for observe/unobserve
- [ ] Handle observation events from server

---

### 2.8: Testing Phase 2

**Tasks:**

- [ ] Test `observe()` returns handle
- [ ] Test handle disposal stops observation
- [ ] Test notification-based changes fire events
- [ ] Test polling-based changes fire events
- [ ] Test freshness is respected
- [ ] Test multiple observations on same element
- [ ] Test observation cleanup on element removal

---

## Summary

### Phase 1 Deliverables

**New types:**

- `Freshness` enum (`Cached | Fresh | MaxAge(Duration)`)

**API changes:**

- `get(id, freshness)` unified API on Axio
- `fetch_*` methods remain for discovery (honest about OS calls)

**Architectural cleanup:**

- **Clean layer separation**: Platform (pure OS) → Axio (orchestration) → Registry (pure data)
- **Registry becomes pure**: No OS calls, no event emission, just data + invariants
- **Platform becomes pure**: All `fetch_*` return data, no side effects
- **Axio owns side effects**: Event emission, watch management, coordination
- `element_ops` eliminated (logic distributed to appropriate layers)

**Documentation:**

- Data flow documented
- Registry invariants documented
- Layer responsibilities clear

**TS client:**

- `get(id, options?)` with freshness support
- Backwards compatible

### Phase 2 Deliverables

- `Observation` handle type
- `observe()` method
- Strategy matrix (initial)
- Observation polling integration
- TS client `observe()` support

### Out of Scope (Future)

- Budget management
- Graceful degradation
- Simplified view observation
- Tree observation helper
- Cross-platform strategy matrix
