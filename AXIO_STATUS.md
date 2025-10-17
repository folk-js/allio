# AXIO - Clean Node-Based Architecture

## Core Principle

**Nodes perform operations on themselves** - just like DOM nodes.

```typescript
const tree = await axio.getTree(pid);

// Find a textbox
const textbox = findNode(tree, (n) => n.role === "textbox");

// Node knows how to operate on itself!
await textbox.setValue("Hello, world!");
```

## AXNode Structure

```typescript
interface AXNode {
  // Location (nodes know where they are)
  pid: number;
  path: number[]; // [0, 1, 2] = root -> child 0 -> child 1 -> child 2

  // Identity
  id: string;
  role: AXRole; // ARIA-based: "button", "textbox", "window", etc.
  subrole?: string; // Platform name for unknown roles (e.g., "AXSomething")

  // Content
  title?: string;
  value?: AXValue; // Typed: String | Integer | Float | Boolean
  description?: string;
  placeholder?: string;

  // State
  focused: boolean;
  enabled: boolean;
  selected?: boolean;

  // Geometry
  bounds?: { position: { x; y }; size: { width; height } };

  // Tree structure
  children: AXNode[];

  // Operations (nodes operate on themselves!)
  setValue(text: string): Promise<void>;
}
```

## Usage Examples

```typescript
const axio = new AXIO();
await axio.connect();

// Get tree for a process
const tree = await axio.getTree(pid);
// Returns: { pid: 123, path: [], role: "application", title: "Finder", children: [...] }

// Nodes can operate on themselves
await tree.children[0].setValue("new text");
await tree.children[1].children[2].setValue("more text");

// Or find a node and use it
function findTextbox(node: AXNode): AXNode | null {
  if (node.role === "textbox") return node;
  for (const child of node.children) {
    const found = findTextbox(child);
    if (found) return found;
  }
  return null;
}

const textbox = findTextbox(tree);
if (textbox) {
  await textbox.setValue("Hello!");
}

// Listen for window updates
axio.onWindowUpdate((windows) => {
  // Handle window position/size changes
});
```

## Clean Methods

- `axio.getTree(pid)` → `Promise<AXNode>` - Returns tree with all nodes having methods
- `node.setValue(text)` → `Promise<void>` - Node operates on itself!
- `axio.onWindowUpdate(callback)` → Register window update handler
- `axio.connect()` → `Promise<void>`
- `axio.disconnect()` → `void`

## Backend (Rust)

### Nodes Have Location

Every node in the tree includes:

- `pid` - Process ID
- `path` - Child indices from root (e.g., `[0, 1, 2]`)

### Platform Abstraction

- `src-tauri/src/platform/macos.rs` - macOS-specific code
- `src-tauri/src/platform/mod.rs` - Platform interface
- Future: `windows.rs`, `linux.rs`

### WebSocket Protocol

- `get_accessibility_tree` → Returns tree structure with pid/path on all nodes
- `write_to_element` → Writes text to element at path
- Window updates broadcast automatically

### No Overlay-Specific Code

- No `OverlayInfoResponse`
- No handle-based protocols
- Just clean tree operations

## Unknown Nodes

When a role doesn't map to ARIA, the native name is preserved:

```json
{
  "role": "unknown",
  "subrole": "AXSplitGroup", // Debug info!
  "title": "..."
}
```

## Value Display

Simple string conversion - no special formatting:

```typescript
String(node.value.value); // Works for all types
```

## Path-Based Navigation

Operations re-navigate from root each time:

```rust
let app = AXUIElement::application(pid);
let element = navigate_to_element(&app, &[0, 1, 2]);
// Now operate on element
```

Simple, reliable, works. Future optimizations can add caching if needed.

## The "Handle" Concept

Every node **IS** a handle - it knows:

- Where it is (`pid`, `path`)
- What it is (`role`, `title`, `value`, etc.)
- How to operate on itself (`setValue()`)

No separate handle type. No wrapper classes. Just nodes.

## TODOs for Future

1. **Element Reference Caching**: Can we hold AXUIElement refs for performance?
2. **Staleness Detection**: Detect when path navigation fails
3. **Stable Identity**: Research AXUIElement lifecycle beyond paths
4. **Change Detection**: Explore AXObserver API for live events
5. **More Operations**: Add `click()`, `focus()`, `getValue()`, etc.

## What's Clean Now

✅ Nodes have `pid` and `path` - they know where they are  
✅ Nodes have `.setValue()` - they operate on themselves  
✅ No separate "handle" type - nodes ARE handles  
✅ Tree structure natural - just children arrays  
✅ Simple API - `await node.setValue("text")`  
✅ Unknown nodes show native name in `subrole`  
✅ No `axValueToString` - use `String(value.value)`  
✅ No overlay-specific code in WebSocket

## Ready to Use

```typescript
// This is the whole API
const axio = new AXIO();
await axio.connect();
const tree = await axio.getTree(pid);
await tree.children[5].setValue("Hello!");
```

Both Rust and TypeScript compile cleanly. Nodes can operate on themselves. Simple and powerful.
