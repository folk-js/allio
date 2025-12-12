- currently we have a hardcoded websocket connection. We need to look at this wiring and see how we might do better discovery here. this isn't immediately obvious, partly because the websocket client is running in a web view...

- our watch logic is a bit weird, we special case destruction. can we simplify this? Same should be investigated for other notifications and state changes. Should element_watch and destruction_watch be merged? Would it make sense for watch+unwatch to just take a list of notifications, which are added/removed from the watch set (which we'd use to not double-subscribe to notifications)? Would this approach work for macOS, are there any platform-specific idiosyncrasies to consider here?

- for watching, actions, getting/setting values: can we make these both more flexible (watch+unwatch takes a list of notifications, actions takes a list of actions, etc) while also improving type safety? these seem perhaps at odds. E.g. we use specifically the destruction notification for cleaning up our state, but this feels special, and its a pattern that extends beyond just destruction.
- add mock platform + fuzz for testing

MISC:

- fix AXColorWell mapping to a ColorPicker role, then wire up reactivity with color values so we can FINALLY do that demo where we use our 'favorite color' picker to input reactively to e.g. a hex field
- figure out why we cant see table data in the accessibility tree and what to change there
- figure out future ways to handle table (or other structured) data
- figure out how to get filepaths from macos finder accessibility elements

- [x] rename AXIO to ALLIO? As in a11y i/o layer? ALL I/O! **DONE - Renamed to Allio!**

misc other bits:

- [ ] **CLI tool?** - Use `allio` crate directly for scripting/automation without Tauri
- [ ] **Query API?** - `allio.query()` for searching elements by predicate
- [ ] **Select API?** - `allio.select(element_id)` for selecting items in lists/tables
