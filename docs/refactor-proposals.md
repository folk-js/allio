# AXIO Refactor Proposals

Status tracking for architectural improvements.

> **Note:** API compatibility does not need to be maintained during this refactor.

---

## Implementation Plan

### Phase 1: Quick Wins (isolated, no dependencies)

- [x] **1. parking_lot migration** - Replace `std::sync::Mutex` with `parking_lot::Mutex`
- [x] **2. Double-Option removal** - `Lazy<Mutex<Option<T>>>` → `LazyLock<Mutex<T>>`
- [x] **3. Bounds helpers** - Extract `Bounds::matches()` and `Bounds::contains_point()`
- [x] **4. Bundle ID from x-win** - Add `bundle_id` to `x-win::WindowInfo`, remove `BUNDLE_ID_CACHE`

### Phase 2: Prep Work (makes Phase 3 easier)

- [x] **5. Consistent ID types** - Use `WindowId`, `ProcessId`, `Point` everywhere
- [ ] **6. Extract pure data from platform handles** - Separate `AXElement` data from `AXUIElement` handles

### Phase 3: Registry Restructuring

- [ ] **7. Create `WindowRegistry`** - Pure data, owns `active` and `depth_order`
- [ ] **8. Refactor `ElementRegistry`** - Remove platform types, add `by_window` index
- [ ] **9. Consolidate platform handles** - Platform module owns all native handles

### Phase 4: Polish & Experiments

- [ ] **10. Merge x-win into windowing** - Inline x-win functionality directly into axio
- [ ] **11. CFHash for element dedup** - Try O(1) lookup (might not work)
- [ ] **12. Element eviction strategy** - Prevent unbounded growth
- [ ] **13. Better error handling** - Graceful degradation

---

## Phase 1 Details

### 1. parking_lot Migration

**Problem:** Every `.lock().unwrap()` panics if a thread panics while holding the lock.

**Solution:**

```rust
// Cargo.toml
parking_lot = "0.12"

// Before
let cache = WINDOW_CACHE.lock().unwrap();

// After
use parking_lot::Mutex;
let cache = WINDOW_CACHE.lock();  // Never panics
```

**Files:** `window_manager.rs`, `element_registry.rs`, `windows.rs`, `platform/macos.rs`

---

### 2. Double-Option Removal

**Problem:** `ElementRegistry` uses `Lazy<Mutex<Option<T>>>` requiring explicit `initialize()`.

**Solution:**

```rust
// Before
static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));
let registry = guard.as_mut().expect("ElementRegistry not initialized");

// After
use std::sync::LazyLock;
static ELEMENT_REGISTRY: LazyLock<Mutex<ElementRegistry>> =
    LazyLock::new(|| Mutex::new(ElementRegistry::new()));
// Remove initialize() - auto-initialized on first access
```

---

### 3. Bounds Helpers

**Problem:** Three places do bounds comparison with different margins.

**Solution:**

```rust
impl Bounds {
    pub fn matches(&self, other: &Bounds, margin: f64) -> bool {
        (self.x - other.x).abs() <= margin &&
        (self.y - other.y).abs() <= margin &&
        (self.w - other.w).abs() <= margin &&
        (self.h - other.h).abs() <= margin
    }

    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w &&
        y >= self.y && y <= self.y + self.h
    }
}
```

---

### 4. Bundle ID from x-win

**Problem:** Spawning `lsappinfo` shell command (~10ms) when x-win already fetches bundle ID.

**Solution:** Add `bundle_id: Option<String>` to `x-win::WindowInfo`, remove `BUNDLE_ID_CACHE` entirely.

---

## State Management Design

### Current State (Before)

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Global Statics                                │
├─────────────────────────────────────────────────────────────────────┤
│  windows.rs                                                          │
│  ├── CURRENT_WINDOWS: RwLock<Vec<AXWindow>>     (snapshot)          │
│  ├── ACTIVE_WINDOW: RwLock<Option<String>>      (derived state)     │
│  └── BUNDLE_ID_CACHE: Mutex<HashMap<u32, _>>    (perf cache)        │
│                                                                      │
│  window_manager.rs                                                   │
│  └── WINDOW_CACHE: Mutex<WindowCache>           (windows + handles) │
│                                                                      │
│  element_registry.rs                                                 │
│  └── ELEMENT_REGISTRY: Mutex<Option<ElementRegistry>>               │
│       ├── windows: HashMap<WindowId, WindowState>                   │
│       │    ├── elements: HashMap<ElementId, StoredElement>          │
│       │    └── observer: Option<AXObserverRef>                      │
│       └── element_to_window: HashMap<ElementId, WindowId>           │
│                                                                      │
│  events.rs                                                           │
│  └── EVENT_SINK: OnceLock<Box<dyn EventSink>>   (output channel)    │
│                                                                      │
│  platform/macos.rs                                                   │
│  ├── NEXT_CONTEXT_ID: AtomicU64                 (ID generator)      │
│  ├── OBSERVER_CONTEXT_REGISTRY: Mutex<HashMap>  (callback safety)   │
│  ├── APP_CONTEXT_REGISTRY: Mutex<HashMap>       (callback safety)   │
│  └── APP_OBSERVERS: Mutex<HashMap<u32, AppState>>  (Tier 1)         │
└─────────────────────────────────────────────────────────────────────┘
```

### Target State (After)

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Target Structure                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    WindowRegistry                            │    │
│  │  (Pure data - no platform types)                            │    │
│  ├─────────────────────────────────────────────────────────────┤    │
│  │  windows: HashMap<WindowId, AXWindow>                       │    │
│  │  active: Option<WindowId>           ← was ACTIVE_WINDOW     │    │
│  │  depth_order: Vec<WindowId>                                 │    │
│  │  snapshot: Vec<AXWindow>            ← was CURRENT_WINDOWS   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    ElementRegistry                           │    │
│  │  (Pure data - no platform types)                            │    │
│  ├─────────────────────────────────────────────────────────────┤    │
│  │  elements: HashMap<ElementId, AXElement>                    │    │
│  │  by_window: HashMap<WindowId, HashSet<ElementId>>           │    │
│  │  watched: HashSet<ElementId>                                │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                 platform module (internal)                   │    │
│  │  Owns all native handles - callers use IDs                  │    │
│  ├─────────────────────────────────────────────────────────────┤    │
│  │  window_handles: HashMap<WindowId, AXUIElement>             │    │
│  │  element_handles: HashMap<ElementId, ElementHandle>         │    │
│  │  window_observers: HashMap<WindowId, AXObserverRef>         │    │
│  │  app_observers: HashMap<u32, AppObserverState>              │    │
│  │  context_counter: AtomicU64                                 │    │
│  │  element_contexts: HashMap<u64, ElementId>                  │    │
│  │  app_contexts: HashMap<u64, u32>                            │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    Infrastructure (unchanged)                │    │
│  ├─────────────────────────────────────────────────────────────┤    │
│  │  EVENT_SINK: OnceLock<Box<dyn EventSink>>  (set once)       │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  REMOVED / ABSORBED:                                                 │
│  ├── CURRENT_WINDOWS  → WindowRegistry.snapshot                     │
│  ├── ACTIVE_WINDOW    → WindowRegistry.active                       │
│  └── BUNDLE_ID_CACHE  → comes from x-win now                        │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Key Principle: IDs as Interface

Registries and platform module communicate via IDs, not types:

```
  WindowRegistry              platform module
  ┌──────────────┐           ┌──────────────┐
  │ AXWindow     │           │ AXUIElement  │
  │  id: "123"   │──────────▶│  key: "123"  │
  └──────────────┘           └──────────────┘

  ElementRegistry            platform module
  ┌──────────────┐           ┌──────────────┐
  │ AXElement    │           │ AXUIElement  │
  │  id: "abc"   │──────────▶│  key: "abc"  │
  └──────────────┘           └──────────────┘
```

```rust
// Pure data operation
let element = ELEMENT_REGISTRY.read().get(&element_id)?;

// When platform access needed, pass ID to platform module
platform::click(&element_id)?;  // Looks up handle internally
```

### Key Invariant

**When an element is removed from `ElementRegistry`, its handle MUST be removed from the platform module.**

```rust
pub fn remove_element(element_id: &ElementId) {
    ELEMENT_REGISTRY.write().remove(element_id);
    platform::remove_element(element_id);
}
```

---

## Parking Lot (Future)

- Platform abstraction trait
- Proper Windows/Linux support structure
- Client-side caching strategy
