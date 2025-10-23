# Reference-Based Accessibility Refactor - Progress

## ✅ Phase 1: Foundation (COMPLETE)

### 1. Element Registry (`element_registry.rs`)

**Status:** ✅ Complete

- Global UUID → `AXUIElement` mapping
- Thread-safe with `once_cell::Lazy<Mutex<>>`
- Methods: `register()`, `get()`, `unregister()`, `is_valid()`, `clear()`

**Usage:**

```rust
let element_id = ElementRegistry::register(ax_element);
let element = ElementRegistry::get(&element_id);
```

### 2. Private API Bindings (`platform/ax_private.rs`)

**Status:** ✅ Complete

- FFI for `_AXUIElementGetWindow()`
- Safe wrapper: `get_window_id_from_element()`
- Returns `Option<u32>` (CGWindowID for matching)

**Usage:**

```rust
let window_id = get_window_id_from_element(element.as_concrete_TypeRef());
```

### 3. Window Element Getter (`platform/macos.rs`)

**Status:** ✅ Complete

- `get_window_elements(pid)` returns `Vec<(AXUIElement, Option<u32>)>`
- Filters app children for `AXWindow` role
- Gets CGWindowID for each window

**Usage:**

```rust
let windows = get_window_elements(pid)?;
for (element, cg_window_id) in windows {
    // Use window element as root, not app element!
}
```

### 4. Window Manager (`window_manager.rs`)

**Status:** ✅ Complete

- `ManagedWindow` struct: combines `WindowInfo` + `AXUIElement` + `CGWindowID`
- Caches windows with their AX elements
- **Only fetches AX elements when windows are added** (not on every poll!)
- Tracks additions/removals automatically

**Usage:**

```rust
// In polling loop - happens ~120 FPS
let (managed_windows, added_ids, removed_ids) =
    WindowManager::update_windows(window_infos);

// Logs:
// ➕ Windows added: ["window_123"]
// ➖ Windows removed: ["window_456"]
```

**Key Benefit:** Window polling stays lightweight (x-win only). AX element fetching happens ONCE per window, not 120 times per second!

### 5. Integration with Window Polling

**Status:** ✅ Complete

- Modified `window_polling_loop()` to use `WindowManager`
- Logs when windows are added/removed
- AX elements fetched only for new windows
- Cache maintained automatically

## 📊 Architecture Before vs After

### Before (Path-Based):

```
Every operation:
1. get_ax_tree_by_pid(pid)
2. AXUIElement::application(pid)      ← Gets APP element
3. navigate_to_element(&app, [0,2,1]) ← Walks tree with paths
4. Do operation
```

**Problems:**

- ❌ Using APP element (not window)
- ❌ Paths break when tree changes
- ❌ Re-navigation every operation
- ❌ Fetching AX elements 120 times/sec

### After (Reference-Based):

```
On window add (once):
1. WindowManager detects new window
2. get_window_elements(pid)           ← Gets WINDOW elements
3. Cache element + CGWindowID

On operation:
1. Get window from cache               ← Direct lookup, O(1)
2. Use window element as root          ← No APP element!
3. Do operation
```

**Benefits:**

- ✅ Using WINDOW element (correct root!)
- ✅ Elements cached, no re-fetching
- ✅ No path navigation needed
- ✅ AX fetching only on window add

## 🎯 Next Steps

### Phase 2: Tree Building with Window Roots ✅ COMPLETE

- [x] Modify `get_ax_tree_by_pid` to accept window element instead of PID
- [x] Create `get_ax_tree_by_window_id(window_id)` that uses cached element
- [x] Update tree building to use window as root (not app)
- [x] Update WebSocket handler to support window_id parameter
- [ ] Register each element as we traverse, building element_id → element map (Phase 3)

### Phase 3: Remove Paths, Add Element IDs

- [ ] Modify `AXNode` struct:
  - Remove: `path: Vec<usize>`
  - Add: `element_id: String` (UUID from registry)
  - Add: `parent_id: Option<String>`
- [ ] Update `element_to_axnode()` to register elements and return IDs
- [ ] Remove `navigate_to_element()` function (no longer needed!)

### Phase 4: Update Operations

- [ ] `get_children_by_path` → `get_children_by_element_id`
- [ ] `write_to_element(path)` → `write_to_element(element_id)`
- [ ] Update `node_watcher` to use element_ids
- [ ] Update WebSocket protocol (path → element_id)

### Phase 5: Cleanup

- [ ] Remove all path-related code
- [ ] Update frontend `axio.ts` to use element_ids
- [ ] Add staleness detection (element validity checks)
- [ ] Consider AXObserver for window events (replace polling)

## 🏗️ Current State

**What Works:**

- ✅ Window detection and tracking
- ✅ AX element caching (efficient!)
- ✅ CGWindowID matching capability
- ✅ Automatic add/remove detection

**What's Still Using Old System:**

- ⚠️ Tree building (still uses APP element + paths)
- ⚠️ Node operations (still navigate by path)
- ⚠️ WebSocket protocol (still sends paths)
- ⚠️ Frontend (still uses paths)

**Next Milestone:** Make tree building use window elements as roots and return element_ids instead of paths!
