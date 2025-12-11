# Observation System Ideas

This document captures ideas from a design conversation about how Axio should handle caching, freshness, and reactivity. Ideas are presented separately for evaluation - they are not yet a unified design.

---

## Context: The Problem

Axio wants to be:
1. **Fast** - avoid IPC, work in-memory
2. **Reactive** - respond to changes in real-time
3. **Universal** - works for everything, not just things that emit notifications
4. **Cross-platform** - not macOS-specific patterns

Reality constraints:
- **Fast requires caching**, but caches can be stale
- **Reactivity requires notifications**, but not everything emits them (e.g., StaticText value changes)
- **Universal reactivity is impossible** at the OS level
- **Watching everything is expensive** - notifications have overhead, polling has IPC cost

The current architecture is confused because it tries to provide *implicit universal reactivity* when that's physically impossible.

---

## Idea 1: The Honest API

**Core insight:** Stop pretending the cache is always fresh. Make staleness explicit.

### Three Freshness Modes

```rust
enum Freshness {
    /// Use whatever's in cache. Fast but might be stale.
    Cached,
    /// Refresh if older than this duration.
    MaxAge(Duration),
    /// Always fetch from OS. Slow but guaranteed fresh.
    Fresh,
}

impl Axio {
    fn get(&self, id: ElementId, freshness: Freshness) -> Option<Element>;
}
```

### Explicit Polling Opt-In

```rust
impl Axio {
    /// Start polling this element. Changes emit events.
    fn poll(&self, id: ElementId, interval: Duration);
    
    /// Stop polling.
    fn unpoll(&self, id: ElementId);
}
```

### Clear Subscription Semantics

```rust
impl Axio {
    /// Subscribe to changes. 
    /// - Uses OS notifications where available
    /// - For polled elements, fires on poll-detected changes
    /// - NOT guaranteed to catch all changes
    fn subscribe(&self, id: ElementId) -> Receiver<ElementChange>;
}
```

**Trade-off:** Honest but verbose. Client must think about freshness.

---

## Idea 2: The `ensure_element` Pattern

**Core insight:** There's only ONE orchestration we need: "Given a handle, ensure it's in the cache."

```rust
impl Axio {
    /// Ensure element is cached. Fast if already exists.
    fn ensure_element(&self, handle: Handle, window_id: WindowId, pid: u32) -> Option<Element> {
        let hash = handle.element_hash();
        
        // Fast path: already cached
        if let Some(elem) = self.get_by_hash(hash, window_id) {
            return Some(elem);  // No OS calls
        }
        
        // Slow path: build from OS
        let attrs = handle.fetch_attributes();  // OS call
        let entry = self.build_entry(handle, attrs, window_id, pid);
        let id = self.insert(entry);
        self.setup_watch(id);
        
        self.get(id)
    }
}
```

Everything else composes from this:
- `fetch_children` = get handles from OS, `ensure_element` each, update tree
- `fetch_parent` = get handle from OS, `ensure_element`
- Notification handlers = `ensure_element`, then update state

**Trade-off:** Simplifies internals, but doesn't address the reactivity problem.

---

## Idea 3: Three Tiers of Liveness

**Core insight:** Different things need different liveness levels.

### Tier 0: Snapshot (one-shot, no tracking)
```typescript
const elem = await axio.elementAt(x, y);
// elem.value is value AT QUERY TIME
// If it changes, we don't know
```

### Tier 1: Global State (always observed, implicitly)
```typescript
axio.focusedElement  // Always live, no setup needed
axio.selection       // Always live
axio.focusedWindow   // Always live
axio.windows         // Always live (polled)
```

### Tier 2: Explicit Observation (opt-in)
```typescript
const obs = axio.observe(elemId, ['value']);
obs.element.value  // Always live while obs exists
obs.on('change', () => { ... });
obs.dispose();
```

**Trade-off:** Clear mental model, but requires client to manage observations.

---

## Idea 4: Automatic Strategy Selection

**Core insight:** The system should know HOW to keep things fresh, not the client.

### Strategy Matrix

```rust
struct ObservationStrategy {
    /// Use OS notification if available
    notification: Option<Notification>,
    /// Poll as fallback/backup
    poll_interval: Option<Duration>,
}

fn strategy_for(role: Role, attr: Attribute, platform: Platform) -> ObservationStrategy {
    match (platform, role, attr) {
        // TextField value - notification works
        (MacOS, TextField, Value) => ObservationStrategy {
            notification: Some(ValueChanged),
            poll_interval: None,
        },
        
        // StaticText value - no notification, must poll
        (MacOS, StaticText, Value) => ObservationStrategy {
            notification: None,
            poll_interval: Some(100.ms()),
        },
        
        // Bounds - notification exists but unreliable
        (MacOS, _, Bounds) => ObservationStrategy {
            notification: Some(BoundsChanged),
            poll_interval: Some(16.ms()), // Poll as backup
        },
        
        // ... platform-specific knowledge
    }
}
```

### The Unified Observe API

```rust
impl Axio {
    /// Observe attributes. System handles HOW.
    pub fn observe(&self, id: ElementId, attrs: &[Attribute]) -> Observation;
}
```

Client just says "observe this" and the system:
1. Looks up strategy for each attribute
2. Sets up notifications where they work
3. Sets up polling where they don't
4. Emits unified change events

**Trade-off:** Requires building/maintaining the strategy matrix. Platform-specific knowledge.

---

## Idea 5: Staleness as a Parameter

**Core insight:** Let clients specify HOW fresh they need things.

```rust
impl Axio {
    pub fn observe(
        &self, 
        id: ElementId, 
        attrs: &[Attribute],
        max_staleness: Duration,  // How fresh?
    ) -> Observation;
}
```

- `max_staleness: 0ms` = must poll every frame = expensive
- `max_staleness: 100ms` = can poll less often = cheap
- `max_staleness: Duration::MAX` = notification only = free (but may miss changes)

The system budgets polling to meet staleness targets.

**Trade-off:** Gives clients control, but adds API complexity.

---

## Idea 6: Tree Observation

**Core insight:** Observing hierarchies is a common pattern (e.g., TODO list).

### Basic Tree Observation

```typescript
const tree = axio.observeTree(parentId, {
    depth: 2,
    attrs: ['value', 'label'],
});

// tree.root - the parent element
// tree.children - child observations (recursively)
tree.on('change', () => { ... });  // Fires for any change in tree
```

### Automatic Child Tracking

When children change:
1. System detects via `children` attribute observation
2. New children are automatically observed
3. Removed children are automatically unobserved

**Trade-off:** Convenient but can get expensive with deep trees.

---

## Idea 7: Simplified Views

**Core insight:** Clients usually want a pruned/collapsed view, not the raw OS tree.

From TREE_REFACTOR.md:
- Prune generic leaf elements
- Collapse single-child generic containers
- Stay synchronized via events

### Observing Simplified Elements

```rust
impl SimplifiedView {
    /// Get all raw elements that map to this simplified element
    fn raw_elements(&self, simplified_id: SimplifiedId) -> Vec<ElementId>;
}

impl ObservationManager {
    fn observe_simplified(&mut self, id: SimplifiedId, attrs: &[Attribute]) {
        // Observe ALL raw elements that map to this simplified element
        let raw_ids = self.simplified_view.raw_elements(id);
        for raw_id in raw_ids {
            self.observe_raw(raw_id, attrs);
        }
    }
}
```

When any raw element changes, emit a simplified element change event.

**Trade-off:** Adds a layer of indirection. Need to maintain simplified ↔ raw mapping.

---

## Idea 8: Budget Management

**Core insight:** We can't poll everything every frame. Need to budget.

### Polling Budget

```rust
struct ObservationBudget {
    /// Max elements to poll per frame
    polls_per_frame: usize,
    
    /// Max time to spend polling
    time_per_frame: Duration,
}
```

### Priority Tiers

```rust
enum ObservationTier {
    Critical,     // Focused element - always fresh
    Interactive,  // Elements with overlays - high priority
    Observed,     // Explicit observe() calls
    Derived,      // In observed tree but not directly requested
    Background,   // Off-screen or inactive
}
```

### Urgency-Based Polling

Each observation tracks:
- `target_staleness` - what client requested
- `actual_staleness` - how stale it currently is
- `urgency` = `actual / target` - higher = poll sooner

Poll in urgency order until budget exhausted.

**Trade-off:** Complexity. Need to track staleness per observation.

---

## Idea 9: Graceful Degradation

**Core insight:** When over budget, degrade gracefully rather than drop events.

### Degradation Cascade

1. **First**: Increase actual staleness (poll less frequently)
2. **Second**: Deprioritize Background tier
3. **Third**: Notify client of degradation
4. **Fourth**: Suggest reducing observations

### Client Visibility

```typescript
interface Observation {
    readonly staleness: number;  // Current actual staleness
    readonly isFresh: boolean;   // Are we meeting target?
    
    on(event: 'stale', fn: (staleness: number) => void): void;
}

// System-level events
axio.on('degraded', (info) => { ... });
axio.on('recovered', () => { ... });
```

**Trade-off:** Client needs to handle degradation. More API surface.

---

## Idea 10: Frame-Based Event Batching

**Core insight:** Spread work across frames, but deliver events predictably.

### Frame Contract

```rust
struct FrameEvents {
    frame_id: u64,           // Monotonically increasing
    events: Vec<Event>,      // All events detected this frame
    budget_status: Status,   // healthy/degraded/overloaded
}
```

### Event Ordering Guarantees

Within a frame:
1. **Tree order** - parent changes before child changes
2. **Causal order** - if A caused B, A comes first
3. **Sequence numbers** - global ordering

```rust
struct Event {
    seq: u64,              // Global sequence
    frame_id: u64,         // Which frame
    source: EventSource,   // Notification vs polling
    kind: EventKind,
    data: EventData,
}
```

**Trade-off:** Adds complexity to event system. Clients may not need this level of ordering.

---

## Idea 11: The DOM Mental Model

**Core insight:** Make it feel like the browser DOM.

```typescript
// Get element - always current for observed
const elem = axio.get(id);

// Observe - like addEventListener
const obs = axio.observe(id, ['value', 'bounds']);

// Changes fire events
obs.on('change', (elem, changes) => { ... });
```

The system hides:
- Notification vs polling decisions
- Staleness management
- Budget management

Client just declares what they want to be reactive.

**Trade-off:** Requires significant infrastructure to make this "just work."

---

## Idea 12: Framework Integration

**Core insight:** Provide adapters for reactive frameworks.

### React Hook

```typescript
function useLiveElement(id: ElementId, attrs?: Attribute[]): Element | null {
    const [element, setElement] = useState<Element | null>(null);
    
    useEffect(() => {
        const obs = axio.observe(id, attrs);
        setElement(obs.element);
        obs.on('change', (e) => setElement(e));
        return () => obs.dispose();
    }, [id, attrs]);
    
    return element;
}
```

### Solid.js Signal

```typescript
function createLiveElement(id: ElementId, attrs?: Attribute[]) {
    const [element, setElement] = createSignal<Element | null>(null);
    
    const obs = axio.observe(id, attrs);
    setElement(obs.element);
    obs.on('change', setElement);
    onCleanup(() => obs.dispose());
    
    return element;
}
```

**Trade-off:** Additional packages to maintain. Framework-specific.

---

## Client API Summary (Various Options)

### Option A: Minimal Honest API

```typescript
class AXIO {
    // One-shot
    elementAt(x, y): Promise<Element | null>;
    fetchChildren(id, max?): Promise<Element[]>;
    
    // Always-live
    readonly focusedElement: Element | null;
    readonly selection: TextSelection | null;
    readonly windows: Map<WindowId, Window>;
    
    // Explicit control
    poll(id, interval): void;
    unpoll(id): void;
    
    // Events
    on(event, callback): void;
}
```

### Option B: Observation-Based API

```typescript
class AXIO {
    // One-shot
    elementAt(x, y): Promise<Element | null>;
    fetchChildren(id, max?): Promise<Element[]>;
    
    // Always-live
    readonly focusedElement: Element | null;
    readonly selection: TextSelection | null;
    readonly windows: Map<WindowId, Window>;
    
    // Observation
    observe(id, attrs?): Observation;
    observeTree(id, options?): TreeObservation;
    
    // Mutations
    setValue(id, value): Promise<void>;
    click(id): Promise<void>;
}

interface Observation {
    readonly element: Element;
    on(event: 'change', fn): void;
    dispose(): void;
}
```

### Option C: Full Featured API

```typescript
class AXIO {
    // Tier 0: One-shot
    elementAt(x, y): Promise<Element | null>;
    fetchChildren(id, max?): Promise<Element[]>;
    
    // Tier 1: Always-live
    readonly focusedElement: Element | null;
    readonly selection: TextSelection | null;
    readonly windows: Map<WindowId, Window>;
    
    // Tier 2: Observation with staleness control
    observe(id, options?: {
        attrs?: Attribute[];
        maxStaleness?: number;
        priority?: Priority;
    }): Observation;
    
    observeTree(id, options?: {
        depth?: number;
        simplified?: boolean;
        // ... same as observe
    }): TreeObservation;
    
    // Simplified view
    readonly simplified: SimplifiedTree;
    
    // System events
    on('degraded', fn): void;
    on('recovered', fn): void;
    on('frame', fn): void;
    
    // Mutations
    setValue(id, value): Promise<void>;
    click(id): Promise<void>;
}

interface Observation {
    readonly element: Element;
    readonly staleness: number;
    readonly isFresh: boolean;
    
    on('change', fn): void;
    on('stale', fn): void;
    on('removed', fn): void;
    
    dispose(): void;
}
```

---

## Research Needed

1. **Notification Reliability Matrix**
   - Test every (Role, Attribute) combination on macOS
   - Document what works, what's flaky, what's broken
   - Repeat for Windows

2. **IPC Cost Model**
   - How expensive is fetching N attributes?
   - Batch fetching options (`AXUIElementCopyMultipleAttributeValues`)
   - Where's the performance cliff?

3. **Polling Budget Math**
   - At what point does polling break frame rate?
   - How many elements can we poll per frame?

4. **Working Set Estimation**
   - Typical observation count for overlay use case
   - Typical observation count for automation use case

---

## Open Questions

1. Should simplified views be the default or opt-in?
2. Should staleness be configurable per-observation or global?
3. How do we handle tree observation when children change frequently?
4. Should we expose frame-based events or hide them?
5. What's the right granularity for priority tiers?
6. How do we test notification reliability systematically?

