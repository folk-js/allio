# Axio Architecture

## Overview

Axio provides cross-platform accessibility tree access via a WebSocket API. The Rust server maintains a **Registry** of windows and elements, which TypeScript clients mirror via events.

## Core Principle

> **Elements are primary, trees are views.**

The Registry is a flat collection of elements, partitioned by window. Trees are derived by traversing `parent_id`/`children` relationships. This keeps the data model simple and queries flexible.

---

## Registry (Rust)

The Registry is the **source of truth** for all accessibility state.

```rust
pub struct Registry {
    windows: HashMap<WindowId, AXWindow>,
    elements: HashMap<ElementId, AXElement>,
    active_window: Option<WindowId>,
    focused_window: Option<WindowId>,  // Currently focused (can be null for desktop)
}
```

Note: `active_window` is the "last valid focused window" (persists when focus goes to desktop). `focused_window` is the currently focused window (null when desktop focused).

### AXWindow

```rust
pub struct AXWindow {
    id: WindowId,
    title: String,
    bounds: Bounds,  // { x, y, w, h }
    focused: bool,
    process_id: i32,
    process_name: String,
}
```

Windows are discovered via polling (platform-specific window enumeration). They do **not** contain children directly—elements reference their window via `window_id`.

### AXElement

```rust
pub struct AXElement {
    id: ElementId,
    window_id: WindowId,
    parent_id: Option<ElementId>,      // None = root element of window
    children: Option<Vec<ElementId>>,  // None = not yet fetched
    role: String,
    subrole: Option<String>,
    label: Option<String>,
    value: Option<Value>,
    bounds: Option<Bounds>,
    focused: Option<bool>,
    enabled: Option<bool>,
}
```

Elements are fetched lazily via RPC (`children()`, `elementAt()`). The `children` field is `None` until explicitly fetched, allowing incremental tree discovery.

---

## Events

Events notify clients when the Registry changes. **Any registry change emits an event**, regardless of the trigger (polling, RPC, watch, etc.).

### Initial Sync

On connection, the server sends a `sync:init` event with the full current state:

```
sync:init {
  windows: AXWindow[],
  elements: AXElement[],
  active_window: WindowId | null,
  focused_window: WindowId | null
}
```

This allows clients to initialize their local mirror immediately. After this, incremental events keep the client in sync.

### Window Events

```
window:added   { window: AXWindow }
window:changed { window: AXWindow }
window:removed { window: AXWindow }
```

- Fired from window polling
- `removed` includes full data (fired before removal)

### Element Events

```
element:added   { element: AXElement }
element:changed { element: AXElement }
element:removed { element: AXElement }
```

- `added`: First time this element enters the Registry
- `changed`: Element data differs from what was stored (from ANY source)
- `removed`: Element leaving the Registry (includes full data)

### Focus Events

```
focus:changed { window_id: WindowId | null }
active:changed { window_id: WindowId }
```

- `focus:changed`: Fires when `focused_window` changes (`null` when desktop focused)
- `active:changed`: Fires when `active_window` changes (always has a value)

---

## RPC

RPC methods let clients ask questions and perform actions.

### Questions (read from Registry, may fetch from OS)

| Method      | Args         | Returns       | Notes                       |
| ----------- | ------------ | ------------- | --------------------------- |
| `get`       | `element_id` | `AXElement`   | Get cached element          |
| `children`  | `element_id` | `AXElement[]` | Get/fetch children          |
| `elementAt` | `x, y`       | `AXElement`   | Get element at screen point |

If answering requires fetching from the OS:

1. Server fetches from accessibility API
2. Server updates Registry
3. Server emits `element:added` or `element:changed`
4. Server returns result

The client doesn't need to know whether data was cached or fetched.

> **Open question**: Should `get` also fetch if not in cache? For now, `get` is cache-only, while `children` and `elementAt` always fetch fresh data. This may be refined based on usage patterns.

### Actions (perform mutations)

| Method    | Args               | Returns     | Notes                  |
| --------- | ------------------ | ----------- | ---------------------- |
| `write`   | `element_id, text` | `boolean`   | Write text to element  |
| `click`   | `element_id`       | `boolean`   | Perform click action   |
| `refresh` | `element_id`       | `AXElement` | Force re-fetch from OS |

Actions return `boolean` for success/failure.

### Subscriptions

| Method    | Args         | Returns | Notes                        |
| --------- | ------------ | ------- | ---------------------------- |
| `watch`   | `element_id` | `void`  | Subscribe to element changes |
| `unwatch` | `element_id` | `void`  | Unsubscribe                  |

---

## Watch Mechanism

Watching an element creates a macOS `AXObserver` to receive change notifications. This is **expensive**—observers consume system resources.

### Backend Behavior

- `watch(id)` creates an observer (if not already watching)
- `unwatch(id)` removes the observer
- Observer fires → element is refreshed → `element:changed` emitted
- **Connection-scoped**: WebSocket close removes all watches for that client
- **Reference counted**: Multiple clients watching same element = one observer

### Client API

```typescript
// Watch with callback, returns cleanup function
const stop = axio.watch(elementId, (element) => {
  console.log("Element changed:", element);
});

// Later: cleanup
stop();
```

The callback receives the updated element whenever it changes. The returned function cleans up the subscription.

---

## Client (TypeScript)

The client maintains a mirror of the Registry, updated via events.

```typescript
class Axio extends EventEmitter {
  // State (mirrors Registry)
  windows: Map<WindowId, AXWindow>;
  elements: Map<ElementId, AXElement>;
  activeWindow: WindowId | null; // Last valid focused window
  focusedWindow: WindowId | null; // Currently focused (null = desktop)
  clickthrough: boolean; // Convenience for overlay apps

  // Questions (RPC)
  get(id: ElementId): Promise<AXElement>;
  children(id: ElementId): Promise<AXElement[]>;
  elementAt(x: number, y: number): Promise<AXElement>;

  // Actions (RPC)
  write(id: ElementId, text: string): Promise<boolean>;
  click(id: ElementId): Promise<boolean>;
  refresh(id: ElementId): Promise<AXElement>;

  // Subscriptions
  watch(id: ElementId, callback?: (el: AXElement) => void): () => void;

  // Derived queries (local, no RPC)
  // Note: "children" is RPC, "getChildren" is local lookup
  getWindowElements(windowId: WindowId): AXElement[];
  getRootElements(windowId: WindowId): AXElement[];
  getChildren(elementId: ElementId): AXElement[];
}
```

### Event Handling

All events update the local mirror:

```typescript
axio.on("element:added", ({ element }) => {
  this.elements.set(element.id, element);
});

axio.on("element:changed", ({ element }) => {
  this.elements.set(element.id, element);
});

axio.on("element:removed", ({ element }) => {
  this.elements.delete(element.id);
});
```

---

## Cleanup & Lifecycle

### Window-Scoped Elements

Elements are tied to their window:

- Window closes → all elements for that window are removed
- `element:removed` fires for each (with full data)

### Lazy Cleanup

If an element access fails (element no longer exists in OS):

- Element is removed from Registry
- `element:removed` fires

> **Implementation note**: Since we hold `AXUIElement` references, we can detect invalid/dead references when iterating the registry. This allows proactive cleanup without waiting for explicit access failures. The equality-check loop we use for UUID matching could also validate element liveness.

### Watch Cleanup

- Explicit: Call the cleanup function returned by `watch()`
- Implicit: WebSocket disconnection removes all watches

---

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│                    macOS / Platform                      │
│  ┌──────────────┐  ┌─────────────────────────────────┐  │
│  │ Window Enum  │  │ Accessibility API (AXUIElement) │  │
│  └──────┬───────┘  └────────────────┬────────────────┘  │
└─────────┼───────────────────────────┼───────────────────┘
          │                           │
          ▼                           ▼
┌─────────────────────────────────────────────────────────┐
│                   Registry (Rust)                        │
│  windows: HashMap<WindowId, AXWindow>                    │
│  elements: HashMap<ElementId, AXElement>                 │
│  active_window / focused_window                          │
│                                                          │
│  ┌─────────────┐         ┌────────────────────────────┐ │
│  │   Polling   │────────▶│ window:added/changed/removed│ │
│  └─────────────┘         │ focus:changed              │ │
│                          └────────────────────────────┘ │
│  ┌─────────────┐         ┌────────────────────────────┐ │
│  │     RPC     │────────▶│ element:added/changed      │ │
│  └─────────────┘         └────────────────────────────┘ │
│                                                          │
│  ┌─────────────┐         ┌────────────────────────────┐ │
│  │  AXObserver │────────▶│ element:changed            │ │
│  │  (watches)  │         └────────────────────────────┘ │
│  └─────────────┘                                        │
└────────────────────────────┬────────────────────────────┘
                             │ Events (WebSocket)
                             ▼
┌─────────────────────────────────────────────────────────┐
│                   Client (TypeScript)                    │
│  windows: Map<WindowId, AXWindow>                        │
│  elements: Map<ElementId, AXElement>                     │
│  activeWindow / focusedWindow / clickthrough             │
└─────────────────────────────────────────────────────────┘
```

---

## Design Principles

1. **Registry is source of truth** — Rust owns the state, clients mirror it
2. **Events are registry replication** — Any change emits an event
3. **RPC answers questions** — Clients ask, server figures out how to answer
4. **No "discover" concept** — Implementation detail, not API surface
5. **Watch is explicit subscription** — Expensive, requires cleanup
6. **Elements are primary** — Flat collection, trees are derived views
7. **Window-scoped lifecycle** — Elements live with their window

---

## Naming Conventions

- **Types**: `AXWindow`, `AXElement` (prefixed to avoid collision with browser types)
- **RPC `children()`**: Fetches from server (may hit OS)
- **Local `getChildren()`**: Looks up cached data only

---

## Future Considerations

- **Cache vs fetch semantics**: When should `get()` fetch if not cached?
- **Proactive cleanup**: Use AXUIElement validity to detect dead elements
- **Staleness/TTL**: Should non-watched elements expire?
- **LRU eviction**: Memory pressure handling
- **Cross-platform**: How do Windows/Linux accessibility APIs differ?
- **Batch operations**: Fetch multiple elements efficiently
