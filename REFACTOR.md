# AXIO Code Review & Refactor Proposals

## Executive Summary

AXIO has a solid conceptual foundation (bridging platform accessibility APIs to a web interface via WebSocket) but suffers from architectural issues that compound into the problems described. The core issues aren't just code quality—they're design decisions that need revisiting.

## Architectural Goal

AXIO should be structured as **layered crates**:

```
┌─────────────────────────────────────────┐
│        src-tauri (Overlay App)          │  Tauri application
│     webview, system tray, hotkeys       │  depends on axio + axio-ws
└─────────────────────────────────────────┘
                    │
                    ├──────────────────────────────────────┐
                    │                                      │
┌───────────────────▼─────────────────────┐  ┌─────────────▼───────────────┐
│          axio-ws (optional)             │  │      Your CLI Tool          │
│   Exposes AXIO over WebSocket protocol  │  │   (depends on axio only)    │
│   For browser/remote clients            │  │                             │
└─────────────────────────────────────────┘  └─────────────────────────────┘
                    │                                      │
                    └──────────────────┬───────────────────┘
                                       │
┌──────────────────────────────────────▼──┐
│              axio (core)                │  Standalone Rust crate
│   Platform accessibility abstraction    │  No Tauri, no async runtime
│   - Element registry & lifecycle        │  No WebSocket dependency
│   - Tree queries & mutations            │
│   - Event subscriptions (observers)     │
│   - Geometry tracking                   │
└─────────────────────────────────────────┘
```

This enables:

- Using AXIO from CLI tools, daemons, or other Rust applications
- Embedding AXIO in non-Tauri GUI frameworks (egui, iced, etc.)
- Testing the core without Tauri/WebSocket overhead
- Clean separation of concerns
- Different frontends can share the same core

---

## The Hard Problem: Live Element Data

The most challenging architectural problem is keeping element data **live** on the frontend—particularly positions and geometry. This section explores what's actually possible with the macOS Accessibility API and how we might design around its limitations.

### What macOS AXObserver Actually Provides

AXObserver can subscribe to notifications, but **not all elements emit all notifications**:

| Notification                             | Emitted By                 | Notes                            |
| ---------------------------------------- | -------------------------- | -------------------------------- |
| `kAXValueChangedNotification`            | Text fields, sliders, etc. | Reliable for input elements      |
| `kAXTitleChangedNotification`            | Windows                    | Window title changes             |
| `kAXFocusedUIElementChangedNotification` | Application                | Focus moved to different element |
| `kAXMovedNotification`                   | **Windows only**           | Elements don't emit this         |
| `kAXResizedNotification`                 | **Windows only**           | Elements don't emit this         |
| `kAXUIElementDestroyedNotification`      | Most elements              | Element was destroyed            |

**Critical limitation:** Individual UI elements (buttons, text fields, etc.) do **NOT** emit move/resize notifications. Only windows do.

### Coordinate System

All AX bounds are in **screen coordinates**:

- Origin at top-left of primary display
- Y increases downward
- Multi-monitor: secondary displays can have negative X or Y
- Retina: coordinates are in logical points, not physical pixels

The frontend (overlay) needs to know its own screen offset to correctly position elements.

### Possible Approaches for Live Element Geometry

**Approach A: Accept Staleness + Refresh on Demand**

```
Frontend requests tree → Gets bounds at that moment
Frontend caches bounds → May become stale
User hovers element → Frontend requests fresh bounds for that element
```

Pros: Simple, low overhead
Cons: Visual lag when user interacts, bounds can be stale during animations

**Approach B: Subscription-Based Polling**

```
Frontend subscribes to elements it cares about
Backend polls subscribed elements at configurable interval (e.g., 60fps for visible, 1fps for offscreen)
Backend pushes deltas when bounds change
```

Pros: Frontend gets updates automatically
Cons: CPU overhead scales with subscribed elements, still polling

**Approach C: Window-Relative + Window Events**

```
Store element bounds RELATIVE to their window
Subscribe to window move/resize notifications (these ARE reliable)
When window moves → Recalculate all element screen positions client-side
Only re-query elements when tree structure changes
```

Pros: Efficient for window movement (most common case)
Cons: Doesn't handle in-window layout changes (e.g., scrolling, tab switches)

**Approach D: Hybrid with Dirty Tracking**

```
Track "freshness" of bounds (timestamp or generation counter)
Window move/resize → Mark all elements in window as "bounds dirty"
Frontend can request "refresh bounds for dirty elements in viewport"
Backend batches AX queries for efficiency
```

### Current Thinking

The most promising approach seems to be **Approach C + D hybrid**:

1. **Store element positions relative to window** when possible
2. **Subscribe to window geometry events** (these are reliable)
3. **Mark elements dirty** when window moves or content likely changed
4. **Batch refresh** dirty elements that are in the viewport
5. **Accept some staleness** for elements outside viewport

### Open Questions

- How do we detect in-window layout changes (scroll, tab switch)?
- Should the frontend or backend track "viewport" for dirty refresh?
- What's the right polling interval for actively-watched elements?
- How do we handle coordinate transformation for multi-monitor setups?
- Should `AXNode.bounds` be optional/nullable to indicate "unknown/stale"?

---

## The Other Hard Problem: Trees vs Elements

There's a fundamental tension between two access patterns:

### Tree-Centric Pattern

"Give me the accessibility tree for this window, I'll traverse it"

```typescript
const tree = await axio.getTree(windowId);
for (const node of tree.children) {
  if (node.role === 'textbox') { ... }
}
```

- Natural for visualization (like axtrees.ts overlay)
- Matches how AX APIs are structured
- Good for bulk operations ("find all buttons")
- **Problem:** Tree structure changes, IDs could become paths that invalidate

### Element-Centric Pattern

"Give me the element at this point, I want to track it"

```typescript
const element = await axio.getElementAtPosition(x, y);
await axio.watch(element.id);
// Later, element updates arrive...
// Even later, element might be destroyed
```

- Natural for direct manipulation (like ports.ts overlay)
- User points at something → creates a reference
- Reference should remain valid as long as element exists
- **Problem:** How do you find the element in the first place? What context does it have?

### The Conceptual Model: Elements Are Primary, Trees Are Views

```
┌─────────────────────────────────────────────────────────────────┐
│                        Element Universe                          │
│                                                                  │
│   ┌─────┐    ┌─────┐    ┌─────┐    ┌─────┐    ┌─────┐          │
│   │ E1  │    │ E2  │    │ E3  │    │ E4  │    │ E5  │   ...    │
│   │ btn │    │ txt │    │ win │    │ menu│    │ lbl │          │
│   └─────┘    └─────┘    └─────┘    └─────┘    └─────┘          │
│       │          │          │                    │               │
│       └──────────┴──────────┴────────────────────┘               │
│                         │                                        │
│                    parent/child                                  │
│                    relationships                                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Query
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  "Give me the tree rooted at E3"     →    Tree View             │
│  "Give me element at (100, 200)"     →    Direct Element        │
│  "Find elements where role=textbox"  →    Query Results         │
└─────────────────────────────────────────────────────────────────┘
```

**Key insight:** Elements exist independently. Trees are just one way to query/view them.

### Proposed Dual API

```rust
impl Axio {
    // ══════════════════════════════════════════════════════════
    // ELEMENT-CENTRIC API (for direct manipulation)
    // ══════════════════════════════════════════════════════════

    /// Get element at screen position (hit test)
    fn element_at(&self, x: f64, y: f64) -> Option<ElementRef>;

    /// Get element by stable ID (if still valid)
    fn element(&self, id: &ElementId) -> Option<ElementRef>;

    /// Watch an element for changes
    fn watch(&mut self, id: &ElementId) -> Result<(), Error>;

    /// Get current bounds of an element (fresh query)
    fn bounds(&self, id: &ElementId) -> Option<Bounds>;

    /// Perform action on element
    fn click(&self, id: &ElementId) -> Result<(), Error>;
    fn set_value(&self, id: &ElementId, value: &str) -> Result<(), Error>;

    // ══════════════════════════════════════════════════════════
    // TREE-CENTRIC API (for exploration/visualization)
    // ══════════════════════════════════════════════════════════

    /// Get tree rooted at element (or window)
    fn tree(&mut self, root: &ElementId, depth: usize) -> AXNode;

    /// Get children of element (lazy loading)
    fn children(&mut self, id: &ElementId) -> Vec<ElementRef>;

    /// Get parent of element
    fn parent(&self, id: &ElementId) -> Option<ElementRef>;

    // ══════════════════════════════════════════════════════════
    // QUERY API (find elements by criteria)
    // ══════════════════════════════════════════════════════════

    /// Find elements matching predicate
    fn find(&mut self, root: &ElementId, predicate: impl Fn(&AXNode) -> bool) -> Vec<ElementRef>;

    /// Find elements by role
    fn find_by_role(&mut self, root: &ElementId, role: AXRole) -> Vec<ElementRef>;
}
```

### ElementRef vs AXNode

Two different representations for different purposes:

```rust
/// Lightweight reference to an element (for direct access)
/// Contains only the stable ID - properties fetched on demand
pub struct ElementRef {
    pub id: ElementId,
    // Optionally cache some immutable properties
    pub role: AXRole,
}

impl ElementRef {
    /// Fetch current properties (fresh from AX API)
    fn properties(&self, axio: &Axio) -> ElementProperties;

    /// Fetch current bounds (fresh from AX API)
    fn bounds(&self, axio: &Axio) -> Option<Bounds>;
}

/// Full node snapshot (for tree visualization)
/// Contains all properties at time of query
pub struct AXNode {
    pub id: ElementId,
    pub role: AXRole,
    pub label: Option<String>,
    pub value: Option<AXValue>,
    pub bounds: Option<Bounds>,
    pub children: Vec<AXNode>,
    // ...
}
```

### Protocol Implications

The WebSocket protocol should support both patterns:

```typescript
// Element-centric messages
{ type: "get_element_at", x: 100, y: 200 }
{ type: "watch_element", element_id: "..." }
{ type: "get_bounds", element_id: "..." }
{ type: "click", element_id: "..." }

// Tree-centric messages
{ type: "get_tree", root_id: "...", max_depth: 3 }
{ type: "get_children", element_id: "..." }

// Query messages
{ type: "find", root_id: "...", role: "textbox" }

// Push events (both patterns benefit)
{ type: "element_update", element_id: "...", ... }
{ type: "element_destroyed", element_id: "..." }
```

### The Ports Use Case

For ports.ts (linking element state between apps):

1. **Discovery:** User hovers → `get_element_at(x, y)` → get `ElementRef`
2. **Binding:** User clicks to "pin" → `watch(element.id)`
3. **Live updates:** Backend pushes `element_update` events
4. **Geometry:** When drawing connections, call `get_bounds(id)` for fresh positions
5. **Cleanup:** When element is destroyed → `element_destroyed` event → remove port

```typescript
// Ports overlay pseudocode
const ports = new Map<ElementId, Port>();

axio.onElementAtPosition(x, y).then((element) => {
  // Show preview port at element bounds
  showPreview(element);
});

onClick(() => {
  const port = createPort(element.id);
  ports.set(element.id, port);
  axio.watch(element.id);
});

axio.onElementUpdate((update) => {
  const port = ports.get(update.element_id);
  if (port) {
    if (update.type === "destroyed") {
      ports.delete(update.element_id);
      removePort(port);
    } else {
      updatePort(port, update);
    }
  }
});

// On render frame, refresh geometry for visible ports
for (const [id, port] of ports) {
  const bounds = await axio.getBounds(id);
  port.updatePosition(bounds);
}
```

### Compromises

| Aspect         | Tree-Centric           | Element-Centric  | Compromise                                     |
| -------------- | ---------------------- | ---------------- | ---------------------------------------------- |
| Discovery      | Tree traversal         | Hit-test, search | Support both                                   |
| Identity       | Path-based             | UUID-based       | UUID (current approach)                        |
| Data freshness | Snapshot at query time | On-demand + push | Hybrid (push what you can, poll what you must) |
| Memory         | Hold full tree         | Hold only refs   | Refs + lazy loading                            |
| Relationships  | Embedded in tree       | Navigate via API | Both work                                      |

### Open Questions

- Should `ElementRef` cache anything, or always be a pure reference?
- How do we efficiently batch `get_bounds` calls for many elements?
- Should there be a "viewport subscription" that auto-tracks visible elements?
- How does the frontend know an element ID is stale without calling the API?

---

## Critical Issues

### 1. **The Root Cause of Position Sync Issues: Bounds-Based Window Matching**

> **Note:** This bounds-matching issue (window ↔ AXUIElement correlation) is separate from the harder "live element data" problem above. The bounds matching currently works, but is fragile. The live element data problem is more fundamental.

The `window_manager.rs` matches AX elements to windows using position+size comparison:

```rust
// Match windows by bounds (position + size) with 2px margin
const POSITION_MARGIN: i32 = 2;
const SIZE_MARGIN: i32 = 2;

for element in window_elements.iter() {
    // ... extracts position and size ...
    if position_matches && size_matches {
        return (Some(element.clone()), None);
    }
}
```

**Problems:**

- When a window moves/resizes, the match breaks until the next poll
- Multiple windows at similar positions can get mismatched
- The 2px margin is arbitrary and fragile
- AXUIElements don't stay associated with their correct window_id

**This is the fundamental cause of position sync issues**—element IDs remain stable (via CFEqual), but their window association doesn't.

### 2. **Global Mutable State Anti-Pattern**

The codebase uses multiple global statics with interior mutability:

```rust
// element_registry.rs
static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));

// window_manager.rs
static WINDOW_CACHE: Lazy<Mutex<WindowCache>> = Lazy::new(|| Mutex::new(WindowCache::new()));

// windows.rs
static BUNDLE_ID_CACHE: Lazy<Mutex<HashMap<u32, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
```

**Problems:**

- Hidden dependencies make code hard to reason about
- Testing becomes nearly impossible
- Race conditions are easy to introduce
- Non-idiomatic Rust (prefer dependency injection via Tauri state)

### 3. **Unsafe Send/Sync on AXUIElement Types**

```rust
// element_registry.rs
// Manual implementation - operations are thread-safe behind Mutex
unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

// ui_element.rs
// Manual Send/Sync implementation - AXUIElement operations are thread-safe behind Mutex
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}
```

**Problems:**

- The comment says "behind Mutex" but `AXUIElement` is not behind a mutex—the _container_ is
- CF types have specific threading rules (often main-thread only)
- This is papering over potential thread safety issues
- macOS accessibility APIs often need to run on the main thread

### 4. **Mixed Async/Sync Runtime Patterns**

```rust
// main.rs
thread::spawn(move || {
    // ... sync code ...
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async move {
        // ... async code ...
        window_polling_loop(ws_state);  // This is sync again!
    });
});
```

**Problems:**

- Creates a new tokio runtime in a spawned thread (wasteful)
- `window_polling_loop` is sync but called from async context
- Mixes `std::sync::Mutex` with `tokio::sync::broadcast`
- Can cause deadlocks when sync Mutex is held across await points

### 5. **Polling-Based Architecture Instead of Event-Driven**

```rust
// windows.rs
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS

// mouse.rs
thread::sleep(Duration::from_millis(8));
```

At 8ms intervals, you're:

- Calling `x_win::get_open_windows()` 125 times/second
- Calling `x_win::get_active_window()` 125 times/second
- Polling mouse position 125 times/second
- Broadcasting even when nothing changed

**This is inherently wasteful and still can't keep up with fast window operations.**

### 6. **Protocol Lacks Request Correlation**

The RPC pattern in TypeScript has a subtle race condition:

```typescript
// axio.ts
private async rpc<Req, Res extends { success: boolean; error?: string }>(
    config: RPCConfig<Req>,
    request: Req
  ): Promise<Res> {
    return new Promise((resolve, reject) => {
      const handler = (responseData: any) => {
        // First response of this type wins - what if multiple requests are in flight?
        const listeners = this.listeners.get(config.responseType);
        if (listeners) {
          listeners.delete(handler);
        }
        // ...
      };
```

If two `getChildren` calls are made concurrently, the responses can be mismatched.

### 7. **Complex Lifecycle Management Split Across Three Modules**

- `UIElement` stores watch state and operations
- `ElementRegistry` manages element lookup, registration, and observers
- `WindowManager` caches windows and their AX elements

The ownership and responsibilities are unclear:

- Who owns the AXObserver? ElementRegistry creates it but UIElement uses it
- Who owns the element cleanup? WindowManager triggers it but ElementRegistry does it
- Where should bounds be tracked? Stored in AXNode, but stale immediately

---

## Medium Issues

### 8. **Observer Context Defined Twice**

```rust
// element_registry.rs
#[derive(Clone)]
#[repr(C)]
struct ObserverContext {
    element_id: String,
    sender: Arc<broadcast::Sender<String>>,
}

// ui_element.rs
#[derive(Clone)]
struct ObserverContext {
    element_id: String,
    sender: Arc<broadcast::Sender<String>>,
}
```

Same struct defined in two places—easy to diverge.

### 9. **TypeScript Breaks Its Own Readonly Contract**

```typescript
// axio.ts - Mutating "readonly" nodes
switch (update.update_type) {
    case "ValueChanged":
        (node as any).value = update.value;  // Breaking readonly!

// Also in attachNodeMethods
if (node.children && node.children.length > 0) {
    (node as any).children = node.children.map((child) =>
        this.attachNodeMethods(child)
    );
}
```

### 10. **Method Attachment Pattern is Fragile**

```typescript
// axio.ts
private attachNodeMethods(node: AXNode): AXNode {
    // Attach setValue method
    (node as any).setValue = async (text: string) => {
      return this.writeByElementId(node.id, text);
    };
    // ... more method attachments ...
}
```

This creates closures that capture `node.id`, but if the node gets replaced/updated, these closures still point to the old data.

---

## High-Value Proposals

### Proposal 1: **Adopt Event-Driven Window Tracking via NSWorkspace Notifications**

**Instead of polling at 8ms**, subscribe to macOS system events:

```rust
// Pseudo-code for event-driven window tracking
use objc::*;

// NSWorkspace notifications:
// - NSWorkspaceDidActivateApplicationNotification
// - NSWorkspaceDidDeactivateApplicationNotification
// - NSWorkspaceActiveSpaceDidChangeNotification

// AXObserver notifications for windows:
// - kAXWindowCreatedNotification
// - kAXWindowMovedNotification
// - kAXWindowResizedNotification
// - kAXFocusedWindowChangedNotification
```

**Benefits:**

- Zero CPU when nothing changes
- Instant response to window events
- No sync issues from polling lag

### Proposal 2: **Replace Bounds-Matching with Application-Scoped Elements**

Instead of trying to match windows by position, **don't match at all**. Make the window list and accessibility tree orthogonal:

```rust
// Current: Try to match window to AXUIElement by bounds
// New: Track windows by CGWindowID, get AX elements on-demand by application

pub struct WindowState {
    // From x-win or CGWindow APIs - provides geometry
    window_id: CGWindowID,
    pid: pid_t,
    bounds: Bounds,
    title: String,

    // Lazily populated when needed
    ax_root: Option<AXUIElement>,
}

impl WindowState {
    fn get_ax_element(&mut self) -> Option<&AXUIElement> {
        if self.ax_root.is_none() {
            // Get by PID + window matching via CGWindowID if available,
            // or just use the application element
            let app = AXUIElement::application(self.pid);
            self.ax_root = find_window_element(&app, self.window_id);
        }
        self.ax_root.as_ref()
    }
}
```

### Proposal 3: **Unify State into Single Instance-Based State (No Global Statics)**

Replace all global statics with an instance-based `Axio` struct that owns all state:

```rust
/// Core AXIO instance - no global state, no Tauri dependency
pub struct Axio {
    // Window tracking
    windows: HashMap<WindowId, WindowState>,

    // Element registry - maps element_id -> registered element
    elements: HashMap<ElementId, RegisteredElement>,

    // Observer management
    observers: HashMap<pid_t, AXObserverRef>,

    // Event callback registration
    event_handlers: EventHandlers,
}

impl Axio {
    /// Create a new AXIO instance
    pub fn new() -> Result<Self, AxioError> {
        // Check accessibility permissions, initialize platform layer
        // ...
    }

    /// Get all visible windows
    pub fn windows(&self) -> &[Window] { ... }

    /// Get accessibility tree for a window
    pub fn get_tree(&mut self, window_id: &WindowId) -> Result<AXNode, AxioError> { ... }

    /// Write to an element
    pub fn write(&self, element_id: &ElementId, text: &str) -> Result<(), AxioError> { ... }

    /// Subscribe to element changes
    pub fn watch(&mut self, element_id: &ElementId) -> Result<(), AxioError> { ... }

    /// Poll for events (or integrate with event loop)
    pub fn poll_events(&mut self) -> Vec<AxioEvent> { ... }
}

// Usage from any Rust program:
fn main() {
    let mut axio = Axio::new().expect("Failed to initialize AXIO");

    for window in axio.windows() {
        println!("Window: {}", window.title);
    }
}
```

**Benefits:**

- Single source of truth
- No global state - multiple instances possible (useful for testing)
- No Tauri dependency - usable from any Rust program
- Clear ownership - caller owns the `Axio` instance
- Testable - can create isolated instances in tests

### Proposal 4: **Add Request IDs to Protocol**

```typescript
// Before
type ClientMessage =
  | { type: "get_children"; element_id: string; ... }

// After
type ClientMessage =
  | { type: "get_children"; request_id: string; element_id: string; ... }

type ServerMessage =
  | { type: "get_children_response"; request_id: string; ... }
```

This enables:

- Proper request/response correlation
- Concurrent requests without race conditions
- Request timeouts per-request
- Better debugging

### Proposal 5: **Simplify TypeScript API with Immutable Updates**

Instead of mutating nodes and attaching methods, use a functional approach:

```typescript
// Create a proper store with immutable updates
class AxioStore {
  private state: AxioState;
  private subscribers: Set<(state: AxioState) => void>;

  // Immutable update - returns new state
  updateElement(elementId: string, update: Partial<AXNode>): void {
    this.state = produce(this.state, (draft) => {
      const node = findNode(draft.trees, elementId);
      if (node) Object.assign(node, update);
    });
    this.notify();
  }

  // Operations are separate from data
  async setValue(elementId: string, text: string): Promise<void> {
    await this.rpc("write_to_element", { element_id: elementId, text });
  }
}
```

### Proposal 6: **Run AX Operations on Main Thread (With Clear Threading Model)**

macOS accessibility often requires main thread. Instead of `unsafe impl Send`, the core crate should have a clear threading model:

**Option A: Single-threaded API (simplest)**

```rust
/// Axio is !Send and !Sync - must be used from main thread
pub struct Axio {
    // Contains AXUIElement refs which are not thread-safe
    _not_send: PhantomData<*const ()>,
    // ...
}

// This won't compile - enforced at compile time:
// std::thread::spawn(move || { axio.get_tree(...); });
```

**Option B: Thread-safe API with internal dispatcher**

```rust
/// AxioHandle is Send + Sync - can be used from any thread
/// Operations are dispatched to the main thread internally
#[derive(Clone)]
pub struct AxioHandle {
    tx: mpsc::Sender<AxOperation>,
}

impl AxioHandle {
    /// All operations return futures that complete when main thread processes them
    pub async fn get_tree(&self, window_id: &WindowId) -> Result<AXNode, AxioError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx.send(AxOperation::GetTree {
            window_id: window_id.clone(),
            response: response_tx
        })?;
        response_rx.await?
    }
}

/// Must be called from main thread - processes operations
pub struct AxioRuntime {
    rx: mpsc::Receiver<AxOperation>,
    state: AxioState,
}

impl AxioRuntime {
    /// Run on main thread (integrates with CFRunLoop)
    pub fn run(&mut self) {
        // Process AX operations + CFRunLoop events
    }
}
```

**Option C: Callback-based (for embedding in event loops)**

```rust
impl Axio {
    /// Get file descriptor for integration with external event loops
    pub fn event_fd(&self) -> RawFd { ... }

    /// Process pending events - call when event_fd is readable
    pub fn process_events(&mut self) -> Vec<AxioEvent> { ... }
}
```

**Recommendation:** Start with Option A (single-threaded, !Send) for simplicity. The `axio-ws` crate can handle the threading complexity internally by running the Axio instance on a dedicated thread with a channel interface.

### Proposal 7: **Split into Layered Crates**

Current structure mixes concerns. Proposed crate structure:

```
axio/                           # Workspace root
├── crates/
│   ├── axio/                   # Core crate (no Tauri, no WebSocket)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs          # Public API: Axio struct, AXNode, etc.
│   │       ├── error.rs        # AxioError type (using thiserror)
│   │       ├── types.rs        # AXNode, AXRole, AXValue, Bounds, etc.
│   │       ├── element.rs      # Element registry & UIElement
│   │       ├── observer.rs     # AXObserver management
│   │       ├── tree.rs         # Tree traversal and conversion
│   │       └── platform/
│   │           ├── mod.rs      # Platform trait
│   │           ├── macos/
│   │           │   ├── mod.rs
│   │           │   ├── accessibility.rs  # AXUIElement operations
│   │           │   ├── events.rs         # NSWorkspace notifications
│   │           │   └── window.rs         # CGWindow integration
│   │           └── windows/    # Future Windows support
│   │
│   └── axio-ws/                # WebSocket server (optional)
│       ├── Cargo.toml          # depends on axio
│       └── src/
│           ├── lib.rs          # WebSocket server
│           ├── protocol.rs     # Message types
│           └── handlers.rs     # Request handlers
│
├── src-tauri/                  # Tauri app (overlay, tray, etc.)
│   ├── Cargo.toml              # depends on axio, axio-ws
│   └── src/
│       └── main.rs             # App setup, overlay management
│
└── src-web/                    # Frontend (unchanged)
```

**Crate responsibilities:**

| Crate     | Dependencies            | Purpose                            |
| --------- | ----------------------- | ---------------------------------- |
| `axio`    | Platform libs only      | Core accessibility abstraction     |
| `axio-ws` | `axio`, `axum`, `tokio` | WebSocket server for remote access |

Note: `src-tauri` is an application, not a library crate. It depends directly on `axio` and `axio-ws`. No need for a separate `axio-tauri` crate unless we later find reusable Tauri-specific abstractions worth extracting.

**Example: Using `axio` from a CLI tool:**

```rust
// In a separate project
use axio::Axio;

fn main() -> Result<(), axio::Error> {
    let mut axio = Axio::new()?;

    // Find all text fields in the focused window
    if let Some(window) = axio.focused_window() {
        let tree = axio.get_tree(&window.id)?;
        for node in tree.find_all(|n| n.role == AXRole::Textbox) {
            println!("Textbox: {:?}", node.value);
        }
    }

    Ok(())
}
```

---

## Quick Wins

1. **Remove all the debug `println!`** statements or gate them behind a feature flag
2. **Delete deprecated protocol types** in `protocol.ts` (lines 240-347)
3. **Use `thiserror` for proper error types** instead of `String`
4. **Remove redundant `clone()` calls** - many are unnecessary
5. **Add `#[must_use]` to Result-returning functions** to catch ignored errors

---

## Recommended Priority

### Phase 0: Design Decisions (Before Major Refactoring)

Two fundamental architectural questions need answers first:

**1. Live element data architecture** (see "The Hard Problem: Live Element Data"):

- How does the core crate expose element geometry?
- What's the subscription/refresh model?
- How do we handle coordinate systems?
- What does the protocol look like for geometry updates?

**2. Tree vs Element access patterns** (see "The Other Hard Problem: Trees vs Elements"):

- Do we have two types (`ElementRef` vs `AXNode`) or one?
- How does the API surface both tree navigation and direct element access?
- What's the caching/freshness story for each pattern?
- How do subscriptions work (watch whole subtrees? individual elements?)?

These decisions ripple through everything: core API, protocol, TypeScript client, overlays.

### Phase 1: Core Crate Extraction

1. **Split into crates** (Proposal 7) - establishes clean architecture for all other work
2. **Instance-based state** (Proposal 3) - remove global statics, enable testing
3. **Clear threading model** (Proposal 6) - define !Send or dispatcher pattern

### Phase 2: Fix Window Tracking

4. **Event-driven window tracking** (Proposal 1) - use NSWorkspace notifications
5. **Remove bounds-matching** (Proposal 2) - use CGWindowID or other stable identifier

### Phase 3: Live Element Data Implementation

6. Implement chosen approach for element geometry freshness
7. Design efficient "subscribe to elements" API
8. Handle window-relative vs screen coordinates

### Phase 4: Protocol & API Improvements

9. **Request IDs** (Proposal 4) - fixes protocol race conditions
10. **Immutable TypeScript API** (Proposal 5) - improves frontend DX

### Quick Wins (Anytime)

- Remove debug `println!` statements
- Delete deprecated protocol types in `protocol.ts`
- Add `thiserror` for proper error types
- Remove redundant `clone()` calls
- Add `#[must_use]` to Result-returning functions
