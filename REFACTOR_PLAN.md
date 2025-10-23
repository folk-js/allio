# Reference-Based Accessibility Tree Refactor

## Goal

Replace path-based (`Vec<usize>`) navigation with pointer/reference-based system using AXUIElement directly.

## Architecture Changes

### 1. Element Registry ✅

- Global registry mapping UUID -> AXUIElement
- `element_registry.rs` - DONE

### 2. Private API Bindings ✅

- FFI for `_AXUIElementGetWindow`
- `platform/ax_private.rs` - DONE

### 3. WindowInfo Enhancement

**Before:**

```rust
pub struct WindowInfo {
    pub id: String,  // x-win's ID
    pub process_id: u32,
    // ... geometry fields
}
```

**After:**

```rust
pub struct WindowInfo {
    pub id: String,  // Still use x-win's ID for tracking
    pub process_id: u32,
    pub ax_window_id: Option<u32>,  // CGWindowID from _AXUIElementGetWindow
    pub ax_element: Option<AXUIElement>,  // The window element itself!
    pub ax_element_id: Option<String>,  // UUID in registry
    // ... geometry fields
}
```

### 4. AXNode Modification

**Before:**

```rust
pub struct AXNode {
    pub pid: u32,
    pub path: Vec<usize>,  // ❌ Remove this
    pub id: String,  // Generated composite ID
    // ...
}
```

**After:**

```rust
pub struct AXNode {
    pub pid: u32,
    pub element_id: String,  // UUID from registry
    pub id: String,  // Keep for frontend (same as element_id)
    pub parent_id: Option<String>,  // UUID of parent
    // ...
}
```

### 5. Navigation Changes

**Old:**

```rust
navigate_to_element(root, &[0, 2, 1]) -> walks tree with indices
```

**New:**

```rust
// Get element directly from registry
let element = ElementRegistry::get(&element_id)?;

// Navigate to parent
let parent_element = element.attribute(&AXAttribute::parent())?;
let parent_id = ElementRegistry::register(parent_element);

// Navigate to children
let children_array = element.attribute(&AXAttribute::children())?;
for i in 0..children_array.len() {
    let child = children_array.get(i)?;
    let child_id = ElementRegistry::register(child);
}
```

### 6. Tree Building

Instead of building full tree with paths, we:

1. Start with window element (already registered)
2. Load children on-demand, registering each
3. Return AXNodes with element_ids
4. Frontend can request children by element_id

## Implementation Steps

- [x] 1. Create element_registry.rs
- [x] 2. Create platform/ax_private.rs with FFI
- [ ] 3. Add method to get window elements from PID
- [ ] 4. Modify WindowInfo to store AXUIElement
- [ ] 5. Update window polling to populate ax_element
- [ ] 6. Modify AXNode struct (remove path, add element_id/parent_id)
- [ ] 7. Refactor element_to_axnode to use registry
- [ ] 8. Update get_children_by_path -> get_children_by_element_id
- [ ] 9. Update write_to_element to use element_id
- [ ] 10. Update node_watcher to use element_id
- [ ] 11. Update websocket protocol (remove path, use element_id)
- [ ] 12. Test and fix edge cases

## Benefits

✅ **Stable Identity**: Elements don't shift when tree changes
✅ **No Re-acquisition**: Hold references, don't re-navigate
✅ **Fast Operations**: Direct element access via HashMap lookup
✅ **Cleaner Code**: No path manipulation logic
✅ **Better Window Matching**: Use CGWindowID for exact matching
