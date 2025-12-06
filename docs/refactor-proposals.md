# AXIO Refactor Proposals

Status tracking for architectural improvements.

> **Note:** API compatibility does not need to be maintained during this refactor.

---

## Implementation Plan

### Phase 1: Quick Wins ✓

- [x] **1. parking_lot migration** - Replace `std::sync::Mutex` with `parking_lot::Mutex`
- [x] **2. Double-Option removal** - `Lazy<Mutex<Option<T>>>` → `LazyLock<Mutex<T>>`
- [x] **3. Bounds helpers** - Extract `Bounds::matches()` and `Bounds::contains()`
- [x] **4. Bundle ID from x-win** - Add `bundle_id` to `x-win::WindowInfo`, remove `BUNDLE_ID_CACHE`

### Phase 2: Type System ✓

- [x] **5. Consistent ID types** - `WindowId`, `ProcessId`, `Point` everywhere
- [x] **6. Type helpers** - `AXRole`, `AXValue`, `TextRange` methods

### Phase 3: Registry Consolidation ✓

- [x] **7. Create `WindowRegistry`** - Merged `CURRENT_WINDOWS`, `ACTIVE_WINDOW`, `WINDOW_CACHE`
- [x] **8. Simplify events** - `WindowRemoved` now just sends `window_id`
- [x] **9. ElementRegistry helpers** - Added `count()`, `count_for_window()`, `get_for_window()`

### Phase 4: Future Work

- [ ] **10. Merge x-win into windowing** - Inline x-win functionality directly into axio
- [ ] **11. CFHash for element dedup** - Try O(1) lookup (might not work)
- [ ] **12. Element eviction strategy** - Prevent unbounded growth
- [ ] **13. Better error handling** - Graceful degradation

### Decided Against

- **Full platform module separation** - Adds complexity for cross-platform future that may never come. Keeping platform handles in registries for now.

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

## Current State

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Global Statics                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  window_registry.rs                                                  │
│  └── REGISTRY: RwLock<WindowRegistry>                               │
│       ├── windows: HashMap<WindowId, StoredWindow>                  │
│       │    ├── info: AXWindow                                       │
│       │    └── handle: Option<AXUIElement>                          │
│       ├── active: Option<WindowId>                                  │
│       └── depth_order: Vec<WindowId>                                │
│                                                                      │
│  element_registry.rs                                                 │
│  └── ELEMENT_REGISTRY: Mutex<ElementRegistry>                       │
│       ├── windows: HashMap<WindowId, WindowState>                   │
│       │    ├── elements: HashMap<ElementId, StoredElement>          │
│       │    └── observer: Option<AXObserverRef>                      │
│       └── element_to_window: HashMap<ElementId, WindowId>           │
│                                                                      │
│  events.rs                                                           │
│  └── EVENT_SINK: OnceLock<Box<dyn EventSink>>                       │
│                                                                      │
│  platform/macos.rs                                                   │
│  ├── NEXT_CONTEXT_ID: AtomicU64                                     │
│  ├── OBSERVER_CONTEXT_REGISTRY: Mutex<HashMap>                      │
│  ├── APP_CONTEXT_REGISTRY: Mutex<HashMap>                           │
│  └── APP_OBSERVERS: Mutex<HashMap<u32, AppState>>                   │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Key Principle: IDs as Interface

Public API operates on IDs (`WindowId`, `ElementId`). Platform handles are internal:

```rust
// Public API - returns pure data
let window = window_registry::get_window(&window_id);
let element = ElementRegistry::get(&element_id);

// Operations use handles internally
ElementRegistry::click(&element_id)?;  // Looks up AXUIElement internally
```

---

## Future Work

- **Platform abstraction** - Trait-based platform support for Windows/Linux
- **Client-side caching** - Strategic caching in TypeScript client
- **Element eviction** - Prevent unbounded growth of element registry
