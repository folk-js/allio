- Automatically toggle cursor transparency on mouse move. If cursor is over an element which contains a 'data-solid' in its ancestry, make non-transparent, else it should be transparent.
- Split system into 'overlay' which just renders stuff onscreen, and 'axio' as in 'accessability in/out' as the core of the system.

## AXIO

The high level idea here is that we unify around accessability trees as the core data structure. Roots have application metadata and contain windows, which have geometry data (position, size, etc). Then the tree can be queried.

Nodes should be based on a small subset of ARIA. With extensions for things like geometry, and 'functions' to write to the nodes.

The state should be maintained in Rust, with a corresponding AXIO setup in Typescript which uses a set of primitive calls over the websocket (Typescript should be a mostly-dumb wrapper here and not maintain its own state). We can probably use a 'handles' type approach.

The parsing of accessibility stuff is currently VERY crude, and often parses incorrectly. We need a much better solution here, I suspect we can interact with the macOS APIs differently.

In the mid-future we also need to think about structural pattern matching and reactivity. E.g. "give me X when X changes at this location" but this should be entirely absent from the first version.

I also want to explore the possibility of pulling window dimensions directly from accessibility APIs instead of the polling approach we have right now.

The current setup is really a one-off for a demo, like a v0.0.1, we want the redesign to be a principled, minimal first step towards a real system. Also note that the Rust code is very non-idomatic and confused (as well as confusing)
