# Observation System Design

A pruned and refined set of ideas for Axio's observation/freshness model.

---

## Core Problem

Axio wants to be fast (in-memory), reactive (respond to changes), and universal (work for everything). Reality constraints:

- **Caching creates staleness** - fast reads may return old data
- **Not everything emits notifications** - StaticText value changes, bounds changes are unreliable
- **Watching everything is expensive** - notifications have overhead, polling has IPC cost

The current architecture is confused because it tries to provide _implicit universal freshness_ when that's physically impossible. We need to be honest about what's fresh and what might be stale.

---

## Idea 1: Freshness as the Core Concept

### The Freshness Enum

Freshness describes how up-to-date a value should be:

```rust
enum Freshness {
    /// Always get the current value from OS. Every read is an IPC call.
    /// Use for: elementAt(), one-shot queries where you need current truth.
    Fresh,

    /// Use the last known value. No IPC. Might be arbitrarily stale.
    /// Use for: bulk reads, non-critical data.
    Cached,

    /// Value must be at most this old. Poll if needed.
    /// Use for: observed elements where you can tolerate some staleness.
    MaxAge(Duration),
}
```

### Sensible Defaults

Different operations have different default freshness:

| Operation            | Default Freshness | Rationale                            |
| -------------------- | ----------------- | ------------------------------------ |
| `elementAt(x, y)`    | `Fresh`           | Hit testing needs current truth      |
| `fetchChildren(id)`  | `Fresh`           | Discovery needs current truth        |
| `get(id)`            | `Cached`          | Fast lookup, might be stale          |
| `get(id, freshness)` | Specified         | Explicit control                     |
| Observed elements    | `MaxAge(...)`     | Balanced based on observation config |

### Rust API Sketch

```rust
impl Axio {
    /// Get element from cache. Might be stale.
    fn get(&self, id: ElementId) -> Option<Element>;

    /// Get element with explicit freshness control.
    fn get_with(&self, id: ElementId, freshness: Freshness) -> Option<Element>;

    /// One-shot queries - always fresh.
    fn element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>>;
    fn fetch_children(&self, id: ElementId, max: usize) -> AxioResult<Vec<Element>>;
}
```

---

## Idea 2: Observation with Freshness

"Observe element X with freshness Y" means: keep this element's attributes fresh according to the freshness spec.

### The Observation API

```rust
impl Axio {
    /// Observe an element. System keeps it fresh according to `freshness`.
    fn observe(
        &self,
        id: ElementId,
        attrs: &[Attribute],
        freshness: Freshness,
    ) -> Observation;
}

struct Observation {
    /// Current element state (fresh according to the observation's freshness)
    element: Element,

    /// Actual staleness (for monitoring/debugging)
    staleness: Duration,
}
```

### How Observation Works Internally

When you observe an element with a freshness requirement:

1. **Look up strategy** for each attribute: can we use notifications, or must we poll?
2. **Set up notifications** where they work reliably
3. **Set up polling** where they don't, at an interval that meets the freshness requirement
4. **Emit change events** regardless of source (notification or poll)

The client doesn't care whether changes come from notifications or polling - they just get change events when things change.

### Freshness to Polling Interval

```rust
fn polling_interval_for(freshness: Freshness) -> Option<Duration> {
    match freshness {
        Freshness::Fresh => Some(Duration::ZERO), // Every frame
        Freshness::Cached => None,                 // Don't poll
        Freshness::MaxAge(d) => Some(d),          // Poll at this interval
    }
}
```

---

## Idea 3: Automatic Strategy Selection

The system knows which (role, attribute, platform) combinations support notifications and which need polling.

### Strategy Matrix (Research Needed)

```rust
struct ObservationStrategy {
    /// Can we use OS notifications?
    notification: Option<Notification>,
    /// Is the notification reliable, or do we need polling backup?
    needs_polling_backup: bool,
}

fn strategy_for(role: Role, attr: Attribute, platform: Platform) -> ObservationStrategy {
    // This is the knowledge base we need to build through research
    match (platform, role, attr) {
        // TextField value - notification works reliably
        (MacOS, TextField, Value) => ObservationStrategy {
            notification: Some(ValueChanged),
            needs_polling_backup: false,
        },

        // StaticText value - no notification exists
        (MacOS, StaticText, Value) => ObservationStrategy {
            notification: None,
            needs_polling_backup: true, // Must poll
        },

        // Bounds - notification exists but is unreliable
        (MacOS, _, Bounds) => ObservationStrategy {
            notification: Some(BoundsChanged),
            needs_polling_backup: true, // Poll as backup
        },

        // ... to be determined through research
    }
}
```

### The Client Doesn't Care

From the client's perspective:

```typescript
// "Keep this element's value and bounds fresh, poll at most every 50ms"
const obs = axio.observe(elementId, ["value", "bounds"], { maxAge: 50 });

// Changes fire regardless of whether they came from notification or polling
obs.on("change", (element, changedAttrs) => {
  console.log("Changed:", changedAttrs);
});
```

The complexity of notification vs polling is hidden.

---

## Idea 4: Tree Observation (Tentative)

Observing a tree (e.g., a TODO list) is a common pattern, but the current `observeTree` idea feels bespoke.

### The Problem

```typescript
// I want to observe a TODO list and all its items
const list = await axio.fetchChildren(todoListId);
// Now I need to observe each child...
// And when children change, manage subscriptions...
```

### Possible Simpler Primitive: Observe Children

Instead of `observeTree`, maybe just:

```typescript
// Observe the children list itself (structure, not content)
const obs = axio.observe(todoListId, ["children"], { maxAge: 100 });

obs.on("change", async (element, changed) => {
  if (changed.includes("children")) {
    // Children list changed, re-fetch and re-observe
    const children = await axio.fetchChildren(element.id);
    // ...manage child observations manually
  }
});
```

### Or: Higher-Level Tree Observation

```typescript
const tree = axio.observeTree(todoListId, {
  depth: 1,
  attrs: ["value", "label"],
  freshness: { maxAge: 100 },
});

// tree.children is automatically managed
tree.on("change", () => {
  // Any change in tree (structure or content)
});
```

**Open question:** Is there a simpler, more generic primitive that `observeTree` could be built from?

---

## Idea 5: Simplified Views (Structural Observation)

### The Insight

AX trees follow DOM-like semantics:

- **No move operation** - moving a node means delete + recreate
- **Delete cascades** - deleting a node removes all children
- **Structure is simpler than content** - child count changes less frequently than values

### Optimized Simplified View Observation

When observing a simplified view, we don't need to fully observe all raw elements:

1. **Observe child counts** - detect structural changes efficiently
2. **Only observe contracted nodes shallowly** - if a chain of single-child containers collapses, we only need to know if it stops being a single-child chain
3. **Lazy content observation** - only observe value/label when actually needed

```rust
impl SimplifiedView {
    /// Observe structure changes (efficient)
    fn observe_structure(&self, id: SimplifiedId) {
        // For each raw element that maps to this simplified element:
        // - Observe child COUNT only
        // - Detect if contraction rules change
    }

    /// Observe content (more expensive)
    fn observe_content(&self, id: SimplifiedId, attrs: &[Attribute]) {
        // Actually observe the attributes
    }
}
```

### Structural Change Detection

Since delete cascades children, we can detect many changes just by watching for:

- Child count changes
- Element destruction notifications

This is much cheaper than observing all attributes of all elements.

---

## Client API Sketch

This is derived from the ideas above, not a separate specification.

```typescript
class AXIO {
  // ===== Always-Live (implicitly observed) =====
  readonly focusedElement: Element | null;
  readonly focusedWindow: Window | null;
  readonly selection: TextSelection | null;
  readonly windows: Map<WindowId, Window>;

  // ===== One-Shot Queries (Fresh by default) =====
  elementAt(x: number, y: number): Promise<Element | null>;
  fetchChildren(id: ElementId, max?: number): Promise<Element[]>;
  fetchParent(id: ElementId): Promise<Element | null>;

  // ===== Cache Access (Cached by default) =====
  get(id: ElementId): Element | null;
  get(id: ElementId, freshness: Freshness): Element | null;

  // ===== Observation =====
  observe(id: ElementId, options?: ObserveOptions): Observation;

  // ===== Raw vs Simplified =====
  // Raw is default for now (simplified is opt-in during development)
  readonly simplified?: SimplifiedView;

  // ===== Mutations =====
  setValue(id: ElementId, value: Value): Promise<void>;
  click(id: ElementId): Promise<void>;

  // ===== Events =====
  on(event: EventType, callback: (data: any) => void): void;
}

interface ObserveOptions {
  attrs?: Attribute[];
  freshness?: Freshness; // Default: MaxAge(100ms)?
}

type Freshness =
  | "fresh" // Every read is IPC
  | "cached" // Last known value
  | { maxAge: number }; // Milliseconds

interface Observation {
  readonly element: Element;
  readonly staleness: number; // Actual staleness in ms

  on(
    event: "change",
    fn: (element: Element, changed: Attribute[]) => void
  ): void;
  on(event: "removed", fn: () => void): void;

  dispose(): void;
}
```

---

## Research Needed

1. **Notification Reliability Matrix**

   - For each (Role, Attribute) on macOS: does the notification fire reliably?
   - Build a systematic test suite
   - Document platform quirks

2. **IPC Cost Model**

   - Cost of fetching 1 attribute vs N attributes
   - Is `AXUIElementCopyMultipleAttributeValues` faster?
   - At what N does batch fetching win?

3. **Polling Cost Budget**

   - How many elements can we poll per frame while staying under 2ms?
   - Where's the performance cliff?

4. **Child Count Observation**
   - Can we efficiently observe just child counts for structural change detection?
   - Or do we need to poll `children.length` manually?

---

## Open Questions

1. **Tree observation primitive** - Is there something simpler than `observeTree` that can serve as a building block?

2. **Freshness defaults** - What should the default `maxAge` be for observations? 50ms? 100ms?

3. **Simplified view structure observation** - Can we observe structural changes cheaply enough to make this practical?

4. **Notification reliability** - How do we systematically test and document which notifications work?

---

## Future Work (Not In Scope Now)

- **Budget management** - Automatic prioritization when over-observing
- **Graceful degradation** - Staleness increases before dropping events
- **Framework integration** - React hooks, Solid signals
- **Cross-platform** - Windows UI Automation strategy matrix
