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

### Decision: Push + Pull Hybrid

**See "Phase 0 Decisions" section for the resolved approach.**

Summary: Don't poll, don't track dirty state. Push events we get for free, pull on demand when caller needs fresh data. Keep it simple.

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

### Proposed API (Updated)

```rust
impl Axio {
    // ══════════════════════════════════════════════════════════
    // ELEMENT-CENTRIC (direct manipulation)
    // ══════════════════════════════════════════════════════════

    /// Get element at screen position (hit test) - returns minimal Element
    fn element_at(&self, x: f64, y: f64) -> Option<Element>;

    /// Get element by ID (if still valid)
    fn element(&self, id: &ElementId) -> Option<Element>;

    /// Watch an element for changes (push notifications)
    fn watch(&mut self, id: &ElementId) -> Result<(), Error>;
    fn unwatch(&mut self, id: &ElementId);

    /// Get fresh bounds (pull)
    fn bounds(&self, id: &ElementId) -> Option<Bounds>;

    /// Actions
    fn click(&self, id: &ElementId) -> Result<(), Error>;
    fn set_value(&self, id: &ElementId, value: &str) -> Result<(), Error>;

    // ══════════════════════════════════════════════════════════
    // TREE-CENTRIC (exploration/visualization)
    // ══════════════════════════════════════════════════════════

    /// Get tree - returns Elements with children populated
    fn tree(&mut self, root: &ElementId, depth: usize) -> Element;

    /// Get children only
    fn children(&mut self, id: &ElementId) -> Vec<Element>;

    /// Navigate up
    fn parent(&self, id: &ElementId) -> Option<Element>;

    // ══════════════════════════════════════════════════════════
    // WINDOWS
    // ══════════════════════════════════════════════════════════

    /// Get all windows (with z-order for occlusion)
    fn windows(&self) -> Vec<Window>;
}

struct Window {
    pub id: WindowId,
    pub bounds: Bounds,
    pub z_index: u32,  // 0 = frontmost, higher = further back
    pub focused: bool,
    // ... other fields
}

// Event delivery mechanism TBD (callbacks, channels, or for axio-ws just push to clients)
enum AxioEvent {
    WindowChanged { window: Window },
    ElementChanged { element_id: ElementId, element: Element },
    ElementDestroyed { element_id: ElementId },
    FocusChanged { element_id: Option<ElementId> },
}
```

### Decision: Single Element Type

**See "Phase 0 Decisions" section for the resolved approach.**

Summary: One `Element` type with `Option<T>` fields. Fields are populated based on what was queried. No separate ElementRef vs AXNode types.

### Protocol Implications

The WebSocket protocol supports push + pull:

```typescript
// ═══════════════════════════════════════════════════════════
// PULL (client requests)
// ═══════════════════════════════════════════════════════════

// Element-centric
{ type: "get_element_at", request_id: "r1", x: 100, y: 200 }
{ type: "get_bounds", request_id: "r2", element_id: "..." }
{ type: "click", request_id: "r3", element_id: "..." }
{ type: "set_value", request_id: "r4", element_id: "...", value: "..." }

// Tree-centric
{ type: "get_tree", request_id: "r5", root_id: "...", max_depth: 3 }
{ type: "get_children", request_id: "r6", element_id: "..." }

// Subscriptions
{ type: "watch", request_id: "r7", element_id: "..." }
{ type: "unwatch", request_id: "r8", element_id: "..." }

// ═══════════════════════════════════════════════════════════
// PUSH (server events)
// ═══════════════════════════════════════════════════════════

// Responses (correlated by request_id)
{ type: "response", request_id: "r1", success: true, element: {...} }

// Events (unsolicited, for watched elements)
{ type: "element_changed", element_id: "...", element: {...} }
{ type: "element_destroyed", element_id: "..." }
{ type: "window_moved", window_id: "...", bounds: {...} }
{ type: "focus_changed", element_id: "..." }
```

### The Ports Use Case

For ports.ts (linking element state between apps):

1. **Discovery:** User hovers → `get_element_at(x, y)` → get `Element` (minimal: id + role)
2. **Binding:** User clicks to "pin" → `watch(element.id)`
3. **Live updates:** Backend pushes `element_changed` events with hydrated `Element`
4. **Geometry:** When drawing, call `get_bounds(id)` for fresh positions (pull)
5. **Cleanup:** `element_destroyed` event → remove port

```typescript
// Ports overlay pseudocode
const ports = new Map<string, Port>();

// PULL: Hit test on hover
const element = await axio.getElementAt(x, y);
// element = { id: "abc", role: "textbox", bounds: {...} }
showPreview(element);

// PULL + subscribe: Pin an element
onClick(async () => {
  const port = createPort(element);
  ports.set(element.id, port);
  await axio.watch(element.id); // Start receiving push events
});

// PUSH: Handle events
axio.onEvent((event) => {
  switch (event.type) {
    case "element_changed":
      const port = ports.get(event.element_id);
      if (port) {
        // event.element has the changed fields hydrated
        port.update(event.element);
      }
      break;

    case "element_destroyed":
      ports.delete(event.element_id);
      break;
  }
});

// PULL: Refresh bounds on demand (e.g., before drawing)
async function refreshPortBounds(portId: string) {
  const bounds = await axio.getBounds(portId);
  ports.get(portId)?.updatePosition(bounds);
}
```

### Design Summary

| Aspect         | Decision                                                 |
| -------------- | -------------------------------------------------------- |
| Element type   | Single `Element` type with `Option<T>` fields (hydrated) |
| Identity       | UUID-based (stable as long as element exists)            |
| Data freshness | Push events we get free, pull on demand                  |
| Discovery      | Both tree traversal and hit-test supported               |
| Memory         | Elements with only requested fields populated            |

### Remaining Questions (for implementation)

- How do we efficiently batch `get_bounds` calls for many elements?
- Multi-monitor coordinate handling (frontend concern mostly)

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

## Phase 0 Decisions (RESOLVED)

### Decision 1: Live Element Data → Push + Pull Hybrid

**Approach:** Keep it simple. Push events when we get info for free, pull when needed.

```
┌─────────────────────────────────────────────────────────────────┐
│  PUSH (events we receive for free)                              │
│  ─────────────────────────────────────────────────────────────  │
│  • Window moved/resized (kAXMovedNotification)                  │
│  • Element value changed (kAXValueChangedNotification)          │
│  • Element destroyed (kAXUIElementDestroyedNotification)        │
│  • Focus changed (kAXFocusedUIElementChangedNotification)       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼ Broadcast to subscribers
┌─────────────────────────────────────────────────────────────────┐
│  PULL (on-demand queries)                                       │
│  ─────────────────────────────────────────────────────────────  │
│  • Get element at position (hit test)                           │
│  • Get current bounds of element                                │
│  • Get children / tree                                          │
│  • Get current value/label/properties                           │
└─────────────────────────────────────────────────────────────────┘
```

**Key principles:**

- Don't poll. Only query when explicitly asked.
- Push all events we subscribe to (watchers get notifications for free)
- Frontend decides when it needs fresh data
- Bounds may be stale - caller can request refresh if needed

### Decision 2: Single Element Type with Hydration

**Approach:** One `Element` type where fields are `Option<T>` and get filled in based on what was queried.

```rust
/// A UI element. Fields are populated based on how it was obtained.
pub struct Element {
    // Always present (required for identity)
    pub id: ElementId,
    pub role: AXRole,

    // Populated when queried or from tree traversal
    pub label: Option<String>,
    pub value: Option<AXValue>,
    pub description: Option<String>,

    // Populated when bounds were requested or from tree
    pub bounds: Option<Bounds>,

    // Populated when children were requested
    pub children: Option<Vec<Element>>,
    pub children_count: Option<usize>,  // May know count without loading children

    // Populated from watch events
    pub focused: Option<bool>,
    pub enabled: Option<bool>,
}
```

**Semantics:**

- `None` = "not fetched" (unknown)
- `Some(value)` = "was this value when queried" (may be stale)
- To get fresh data, call `axio.refresh(&element.id, fields)` or individual getters

**This pairs well with "elements are primary, trees are views":**

- Tree query returns Elements with children populated
- Hit test returns Element with just id/role (minimal)
- Watch returns Element with changed fields populated

---

## Remaining Work

### Quick Wins (Completed ✓)

- ✓ Removed debug `println!` statements
- ✓ Deleted deprecated protocol types in `protocol.ts`
- ✓ Fixed silent failures that should crash (added proper expects/asserts)

### Quick Wins (Remaining)

- [ ] Add `thiserror` for proper error types (do during crate extraction)
- [ ] Remove redundant `clone()` calls (do during crate extraction)

---

## Implementation Phases

### Phase 1: Core Crate Extraction

1. **Split into crates** - `axio` (core) + `axio-ws` (WebSocket)
2. **Instance-based state** - remove global statics, `Axio::new()` returns owned instance
3. **Clear threading model** - start with `!Send` (single-threaded), document constraints
4. **Implement Element type** - single type with Option fields as decided above

### Phase 2: Fix Window Tracking

5. **Event-driven window tracking** - use NSWorkspace notifications instead of polling
6. **Stable window identity** - use CGWindowID or other stable identifier, not bounds-matching

### Phase 3: Push + Pull Implementation

7. **Implement push events** - subscribe to AX notifications, broadcast to watchers
8. **Implement pull queries** - on-demand getters for bounds, properties, children
9. **Protocol updates** - add request IDs for correlation, update message types

### Phase 4: TypeScript Client Improvements

10. **Match new Element model** - single type with optional fields
11. **Proper state management** - immutable updates, no mutation
12. **Request correlation** - handle concurrent requests correctly

### Future Goals

- **Query API:** `find(root, predicate)` and `find_by_role(root, role)` for searching
- **Z-order occlusion:** Use window `z_index` for CSS-driven occlusion in overlays (e.g., hide ports behind other windows using z-index or clip-path)
