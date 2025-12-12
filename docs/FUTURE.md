## Observation API

### The Problem

Clients want reactive updates without managing notification/polling details.

### Core Idea

```typescript
const obs = axio.observe(elementId, ["value", "bounds"], { maxAge: 100 });

obs.on("change", (element, changedAttrs) => {
  // Fires regardless of whether change came from notification or polling
});

obs.dispose(); // Stop observing
```

### How It Works Internally

1. **Strategy lookup**: For each (role, attribute) pair, determine if macOS notifications work reliably or if polling is needed (need more research here, this matrix may be insufficient, some way to reliably fallback from notifications to polling would be the ideal)
2. **Set up notifications** where they work
3. **Set up polling** where they don't (at interval meeting recency requirement)
4. **Emit unified change events** - client doesn't care about the source

### Recency to Polling Interval

```rust
match recency {
    Recency::Current => poll_every_frame(), // Maybe this is actually INSTANT and we need a separate Recency::MaxFrameCount(n) option? Separate from age driven by polling loop?
    Recency::Any => no_polling(),
    Recency::MaxAge(d) => poll_at_interval(d),
}
```

### Tree Observation Helper

Observing a hierarchy (e.g., TODO list) requires manual subscription management.

```typescript
const tree = axio.observeTree(todoListId, {
  depth: 2,
  attrs: ["value", "label"],
  recency: { maxAge: 100 },
});

// tree.children is automatically managed
tree.on("change", () => {
  // Any change in tree (structure or content)
});
```

Open question: Is there a simpler building block that `observeTree` could be built from?

### Research Needed

- Systematic testing of which (Role, Attribute) notifications are reliable on macOS
- IPC cost model (when does batching win?), other ways to batch / do less work?
- Polling budget (how many elements per frame before performance degrades?)

---

## Registry Tree Views

### The Problem

Raw accessibility trees are verbose. Clients usually want a pruned/collapsed view.

### Core Ideas

**Pruning**: Remove generic leaf elements with no semantic value.

- `GenericElement` is pruned

**Contraction**: Collapse single-child generic containers:

- `Group → Group → Button` becomes `Button`

AX trees follow DOM-like semantics:

- **No move operation** - moving = delete + recreate
- **Delete cascades** - removing a parent removes all children
  // Future note: there is a trivial perf improvement we can do here by batching a subtree removal into an array of ids to remove.
- **Structure is simpler than content** - child count changes less than values

### Efficient Structure Observation

When observing a simplified view:

1. **Observe child counts** only - detect structural changes cheaply
2. **Observe contracted nodes shallowly** - only need to know if contraction rules change
3. **Lazy content observation** - only observe value/label when needed

### Open Questions

1. Should views emit their own events or augment raw events?
2. Eager vs lazy view computation?
3. Should clients request "simplified" vs "full", or get both?

---

## Pattern Matching / Query API

... TBD ...

Thinking about queries/pattern matching and structured data. The overarching goal of this project is to hijack accessibility and other OS APIs to break through app walled gardens and make new kinds of interoperability possible and desirable. What if for example you could plug out of your apple reminders app in @src-web/src/ports.ts and 'pattern match' on todo items and propagate that list directly into a markdown todo list or similar, or pipe the computed values of a spreadsheet into a visualisation tool, etc.
