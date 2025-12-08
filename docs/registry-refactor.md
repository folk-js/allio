# Registry Refactor: Unified Event Emission & Parent-Child Linking

## Overview

The registry is the single source of truth for all accessibility state. All state mutations happen through the registry, and the registry emits all events. Streaming data (mouse position) is the exception.

---

## 1. Event Emission Model

### Principle

**State changes → Registry methods → Events emitted from registry**

Streaming data (mouse position) stays in polling since it's not registry state.

### Target State

| Event              | Registry Method                  | Notes                                 |
| ------------------ | -------------------------------- | ------------------------------------- |
| `ElementAdded`     | `register_element`               | ✓ Done                                |
| `ElementChanged`   | `update_element`, `set_children` | ✓ Done                                |
| `ElementRemoved`   | `remove_element`                 | ✓ Done                                |
| `WindowAdded`      | `update_windows`                 | ✓ Done                                |
| `WindowChanged`    | `update_windows`                 | ✓ Done                                |
| `WindowRemoved`    | `update_windows`                 | ✓ Done                                |
| `FocusChanged`     | `set_focused_window`             | ✓ Done                                |
| `FocusElement`     | `set_process_focus` + emit       | Registry deduplicates, focus.rs emits |
| `SelectionChanged` | `set_process_selection` + emit   | Registry deduplicates, focus.rs emits |
| `MousePosition`    | —                                | Streaming data, stays in polling      |

### Simplifications Applied

- **Dropped `activeWindow` / `ActiveChanged`** - With non-activating panel, the sticky "active" concept is no longer needed. Just use `focusedWindow` which reflects true OS focus state (can be `null` when desktop is focused).
- **Fixed duplicate `WindowRemoved`** - Was emitted from both registry and polling.rs.

---

## 2. Parent-Child Relationship

### Problem

Elements discovered via different paths have inconsistent `parent_id`:

- Via `children()`: parent_id is set ✓
- Via `focus:element` or `at()`: parent_id is `null` ✗

This creates "orphan" elements that appear as phantom tree roots.

### Solution: Lazy Bidirectional Linking

#### Internal State (Registry)

```rust
struct ElementState {
  element: AXElement,
  handle: ElementHandle,
  hash: u64,
  parent_hash: Option<u64>,  // Always populated from AXParent
  // ...
}

struct Registry {
  // Existing
  hash_to_element: HashMap<u64, ElementId>,

  // New: orphans waiting for their parent to be registered
  waiting_for_parent: HashMap<u64, Vec<ElementId>>,  // parent_hash → children
}
```

#### Client-Facing Type

```rust
pub struct AXElement {
  pub id: ElementId,
  pub window_id: WindowId,
  pub root: bool,                        // true only for window root elements
  pub parent_id: Option<ElementId>,      // None = parent not yet loaded (unless root=true)
  pub children: Option<Vec<ElementId>>,  // None = not yet fetched, Some([]) = no children
  // ...
}
```

**Semantics:**

- `root: true` → Window root element (parent is AXApplication in macOS)
- `root: false, parent_id: Some(id)` → Parent is loaded
- `root: false, parent_id: None` → Has parent but not loaded yet

**TypeScript usage:**

```typescript
if (element.root) {
  // This is a window root
}
```

#### Registration Flow

```rust
fn register_element(element, handle, ...) {
  let hash = element_hash(&handle);
  let parent_hash = get_parent_hash(&handle);  // AXParent lookup

  // Check if already exists
  if let Some(existing) = hash_to_element.get(&hash) {
    return existing;
  }

  // Determine if root (no parent in OS)
  element.root = parent_hash.is_none();

  // Link to parent if it exists in registry
  element.parent_id = parent_hash.and_then(|ph| hash_to_element.get(&ph).copied());

  // Register
  hash_to_element.insert(hash, element.id);
  elements.insert(element.id, ElementState { element, hash, parent_hash, ... });

  // Add to parent's children
  if let Some(pid) = element.parent_id {
    add_child_to_parent(pid, element.id);
  } else if !element.root {
    // Orphan - register in waiting list
    waiting_for_parent.entry(parent_hash.unwrap()).or_default().push(element.id);
  }

  // Check if any orphans are waiting for us
  if let Some(orphans) = waiting_for_parent.remove(&hash) {
    for orphan_id in &orphans {
      link_child_to_parent(*orphan_id, element.id);
    }
    add_children(element.id, orphans);
  }

  emit(ElementAdded { element });
}
```

#### Removal Flow

```rust
fn remove_element(element_id) {
  let state = elements.remove(element_id);

  // Remove from parent's children
  if let Some(parent_id) = state.element.parent_id {
    remove_child_from_parent(parent_id, element_id);
  }

  // Cascade: remove all children recursively
  if let Some(children) = state.element.children {
    for child_id in children {
      remove_element(child_id);
    }
  }

  // Clean up
  hash_to_element.remove(&state.hash);
  waiting_for_parent.remove(&state.hash);
  if let Some(ph) = state.parent_hash {
    waiting_for_parent.get_mut(&ph).map(|v| v.retain(|&id| id != element_id));
  }
  unsubscribe_observers(state);

  emit(ElementRemoved { element_id });
}
```

---

## 3. ChildrenChanged Notification

Currently defined but not handled. Should:

1. Re-fetch children from OS
2. Register new children (triggers ElementAdded, linking)
3. Remove children no longer present (triggers ElementRemoved, parent update)
4. Update element's children list (triggers ElementChanged)

```rust
Notification::ChildrenChanged => {
  refresh_children(element_id);  // Handles all the above
}
```

---

## 4. API Naming

### Rust Public API (`api/elements.rs`, `api/windows.rs`)

```rust
// Elements
pub fn at(x: f64, y: f64) -> Result<AXElement>       // Element at screen position
pub fn get(id: &ElementId) -> Result<AXElement>      // Get by ID (from registry)
pub fn refresh(id: &ElementId) -> Result<AXElement>  // Re-fetch attributes from OS
pub fn children(id: &ElementId) -> Result<Vec<AXElement>>  // Fetch children from OS
pub fn parent(id: &ElementId) -> Result<Option<AXElement>> // Fetch parent from OS

// Windows
pub fn root(window_id: &WindowId) -> Result<AXElement>  // Window's root element
pub fn all() -> Vec<AXWindow>
pub fn get(id: &WindowId) -> Option<AXWindow>
pub fn focused() -> Option<WindowId>  // Currently focused window (null if desktop)
```

### TypeScript Client (mirrors Rust)

```typescript
// RPC calls (fetch from OS, return fresh data)
axio.at(x, y): Promise<AXElement>
axio.get(elementId): Promise<AXElement>
axio.refresh(elementId): Promise<AXElement>
axio.children(elementId): Promise<AXElement[]>
axio.parent(elementId): Promise<AXElement | null>
axio.root(windowId): Promise<AXElement>

// Local cache access (no RPC, immediate)
axio.elements: Map<ElementId, AXElement>
axio.windows: Map<WindowId, AXWindow>
axio.focusedWindow: WindowId | null    // null when desktop focused
axio.focusedElement: AXElement | null
axio.focused: AXWindow | null          // Convenience getter
```

**Note:** `getRootElements()` becomes unnecessary. To find window roots:

```typescript
const roots = [...axio.elements.values()].filter(
  (el) => el.root && el.window_id === windowId
);
// Or just: await axio.root(windowId)
```

---

## 5. Implementation Order

1. ~~**Bug fix:** Remove duplicate `WindowRemoved` emission from polling.rs~~ ✓
2. ~~**Simplify focus:** Drop `activeWindow`/`ActiveChanged`, keep only `focusedWindow`/`FocusChanged`~~ ✓
3. ~~**Move window events to registry:** `update_windows` emits events~~ ✓
4. ~~**Move focus to registry:** `set_focused_window` emits `FocusChanged`~~ ✓
5. ~~**Element focus/selection:** Registry deduplicates, focus.rs emits (current pattern is fine)~~ ✓
6. ~~**Add `root` field to AXElement**~~ ✓
7. ~~**Add `parent_hash` to internal ElementState**~~ ✓
8. ~~**Add `waiting_for_parent` index**~~ ✓
9. ~~**Update `register_element` with linking logic**~~ ✓
10. ~~**Update `remove_element` with cascade + parent update**~~ ✓
11. ~~**Add `parent()` RPC**~~ ✓
12. ~~**Handle `ChildrenChanged` notification**~~ ✓
13. ~~**Update TypeScript client**~~ ✓
14. ~~**Remove `windowRoots` hack from axtrees.ts**~~ ✓
15. ~~**Naming cleanup:** Rename `discover_children` → `children`~~ ✓

Note: I think the websocket does a lot of heavy lifting for SyncInit. feels like something that axio should do more for, like, its kinda just dumping the state of the registry in a way?

Other note for future: maybe the root thing could be nicer, perhaps it would be better if somehow merged with parent id.. not sure, we need Rust and TS to both be happy with its design...

Down the line we need to work with more than just text which is what most of our tests have been about. We'll need a nice way for a node of a given role to know its value type, e.g. a checkbox should know its value type is boolean, a slider should know its value type is float, etc.

We'll also want ways to get more structured data out, as one of our next milestones is to be able to query and pipe data out of apps in a more structured way, e.g. piping Apple Reminders todos into a markdown app as an MD list or into a spreadsheet app as a table. Something like a structured "standard IO" comes to mind...

Next up for us: we need to have a way to validate the tree linking and cleanup logic. Thinking of a force directed graph layout demo to test the tree stitching and events, we can add a lib to the demos and wire it up in tauri as 'graph'.

Could be simple, where you just click UI nodes to register them and click again to expand the subgraph through parents+children.. should be enough to be able to make subgraphs and see them join, then if you switch your UI view (e.g. to a different todo list) you should see part of the graph disappear
