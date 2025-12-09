TODOs:

- figure out why we cant see table data in the accessibility tree
- figure out future ways to handle table (or other structured) data
- figure out how to get filepaths from macos finder accessibility elements
- warn on fallthrough for platform mappings, we want to be comprehensive and not just blindly map everything to a generic container
- work towards a better watching mechanism, in ts something like .observe(id, [things to watch], callback) would be nice, but before that we need a much more thorough understanding of what can actually be watched, what must be polled, etc.

misc other bits:

- [ ] **CLI tool?** - Use `axio` crate directly for scripting/automation without Tauri
- [ ] **Query API?** - `axio.query()` for searching elements by predicate
- [ ] **Select API?** - `axio.select(element_id)` for selecting items in lists/tables

Thinking about 2 new explorations for @axio : queries/pattern matching and structured data. The overarching goal of this project is to hijack accessibility and other OS APIs to break through app walled gardens and make new kinds of interoperability possible and desirable. What if for example you could plug out of your apple reminders app in @src-web/src/ports.ts and 'pattern match' on todo items and propagate that list directly into a markdown todo list or similar, or pipe the computed values of a spreadsheet into a visualisation tool, etc.

notes from crabviz callgraph:

- we only use our config object in one place (WebSocketState) I think we should delete it.
- the only users of events.rs outside of the registry are start_server in server.rs and poll_iteration in polling.rs I wonder if we can remove this coupling, so the only one emitting events is the registry...
- Our types are quite spread out. We have our per-file types in accessibility, which is good (vlue, action, notification and role) as this is our cross-platorm abstraction, then we also have many types in a types.rs file, then we have misc types across observer.rs, polling.rs, observer.rs, files in platform, registry.rs...Some in platform are macos specific, some are part of our more generic abstraction... We need to survey our types and use a coherent strategy here. Might also involve changing/removing some types.
- the macos call graph is messy we can simplify it
