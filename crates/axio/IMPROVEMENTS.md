# AXIO Improvements Roadmap

Code review findings and proposed improvements.

## Design Principles

- **Cross-platform ready**: Platform-specific code (macOS, Windows, Linux) lives in `src/platform/<os>.rs`
- **Types are platform-agnostic**: `src/types.rs` contains only types usable on all platforms
- `AXNotification` is in `platform/macos.rs` with macOS-specific notification strings and role mappings

---

## Completed ✅

### 1. Macro for ID Types

**File:** `src/types.rs`
Created `define_id_type!` macro that generates `ElementId` and `WindowId` with 2 lines each.

### 2. Document unsafe Send/Sync

**Files:** `src/ui_element.rs`, `src/window_manager.rs`
Added detailed SAFETY comments explaining thread-safety guarantees.

### 3. Remove Callback Duplication in Polling

**Files:** `src/windows.rs`, `crates/axio-ws/src/websocket.rs`
Removed callbacks from `PollingConfig`, `WsEventSink` now shares window cache.

### 4. Reduce Excessive Cloning

**Files:** `src/platform/macos.rs`, `src/element_registry.rs`
Functions now take `&WindowId`, `Option<&ElementId>`, `&str` instead of owned values.

### 5. Remove RPC Method Aliases

**File:** `src/rpc.rs`
Canonical methods: `element_at`, `tree`, `write`, `watch`, `unwatch`, `click`.

### 6. Clean Up dead_code Attributes

**Files:** `src/api.rs`, `src/types.rs`, `src/events.rs`
Removed blanket allows, added targeted allows with comments.

### 7. Use AxioError Consistently

**Files:** `src/platform/macos.rs`, `src/ui_element.rs`, `src/element_registry.rs`, `src/api.rs`
All functions now return `AxioResult<T>` with proper error variants.

### 8. Consolidate Watch API

**Files:** `src/ui_element.rs`, `src/element_registry.rs`
`UIElement::watch/unwatch` now `pub(crate)`, `ElementRegistry` is the public API.

### 9. Type-Safe Notifications

**Files:** `src/platform/macos.rs`, `src/ui_element.rs`, `src/element_registry.rs`
Created `AXNotification` enum in platform module with `as_str()`, `from_str()`, `for_role()` methods.

---

## Remaining

### Replace Global Statics with Context Struct

**Files:** All  
**Problem:** 4 global statics make testing hard, prevent multiple instances:

- `ELEMENT_REGISTRY` in `element_registry.rs`
- `WINDOW_CACHE` in `window_manager.rs`
- `BUNDLE_ID_CACHE` in `windows.rs`
- `EVENT_SINK` in `events.rs`

**Fix:** Create an `Axio` context struct that owns all state:

```rust
pub struct Axio {
    element_registry: ElementRegistry,
    window_manager: WindowManager,
    event_sink: Box<dyn EventSink>,
}

impl Axio {
    pub fn new(event_sink: impl EventSink + 'static) -> Self { ... }

    pub fn element_at(&self, x: f64, y: f64) -> AxioResult<AXNode> { ... }
    pub fn watch(&mut self, element_id: &ElementId) -> AxioResult<()> { ... }
    // etc.
}
```

**Benefits:**

- Testable (can create isolated instances)
- Multiple instances (e.g., for different accessibility contexts)
- Clear ownership and lifetime
- No initialization order dependencies

---

## Progress

| #   | Task                     | Status  |
| --- | ------------------------ | ------- |
| 1   | ID type macro            | ✅ Done |
| 2   | Document unsafe          | ✅ Done |
| 3   | Remove polling callbacks | ✅ Done |
| 4   | Reduce cloning           | ✅ Done |
| 5   | Remove RPC aliases       | ✅ Done |
| 6   | Clean up dead_code       | ✅ Done |
| 7   | Use AxioError            | ✅ Done |
| 8   | Consolidate Watch API    | ✅ Done |
| 9   | Type-safe notifications  | ✅ Done |
| 10  | Global statics → context | ⬜ TODO |

# Misc open questions

- why does axio-ws need to care about clickthrough?
- axio-ws is currently more than a thin wrapper, does this need to be the case? what is it doing that axio itself does not?
