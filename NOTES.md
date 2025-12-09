important TODOs:

- currently we have a hardcoded websocket connection. We need to look at this wiring and see how we might do better discovery here. this isn't immediately obvious, partly because the websocket client is running in a web view...
- we should drop the AX prefix from the crate. For typegen we should either rename to include the prefix or find a different strategy (like importing as namespaced to avoid collisions with browser types)
- find oppertunities to use more compile-time pure functions. find places where indirection is unnecesary or hurting us (wrapper functions, etc)
- a naming principle we could apply for both Rust and TS: `get_` hits the registry. `fetch_` hits the platform. `get_or_fetch_` hits the registry and if not found, fetches from the platform.
- our watch logic is a bit weird, we special case destruction. can we simplify this? Same should be investigated for other notifications and state changes. Should element_watch and destruction_watch be merged? Would it make sense for watch+unwatch to just take a list of notifications, which are added/removed from the watch set (which we'd use to not double-subscribe to notifications)? Would this approach work for macOS, are there any platform-specific idiosyncrasies to consider here?
- for watching, actions, getting/setting values: can we make these both more flexible (watch+unwatch takes a list of notifications, actions takes a list of actions, etc) while also improving type safety? these seem perhaps at odds. E.g. we use specifically the destruction notification for cleaning up our state, but this feels special, and its a pattern that extends beyond just destruction.
- We do a bit of stuff like `let app_handle = ElementHandle::new(app_element(pid));` which feels off. app element = process lifetime right? should this be part of ProcessState or something?
- wanna know more about "Used through concrete type (trait bound satisfied)"
- add mock platform + fuzz for testing

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
- i wonder about splitting 'ws' into its own crate again... We want 'axio' to be the core thing, and easy to integrate into CLI tools, run on a websocket without tauri, etc, etc. Wonder what the best strategy is here.
