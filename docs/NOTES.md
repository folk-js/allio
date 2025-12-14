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

now:

- Expose current API in TS and update RPC (get, recency)
- remove click, write, writeValue, refresh
- rename action to perform

from reminders:

- Ports demo piping more than just text… colors and booleans
- Sketch out elements API with status: get, set, perform, observe, select, query, views
- Need to ensure we don’t double subscribe, always clean up, and handle failures correctly (new error types?) failures could inform polling fallbacks???
- Multiselect api?
- MERGE!
- future speculative structured data bridge! Any way to do this without community-maintained mappings?? If not, how do we make that easy and effective? Map identifier to data? Hmmmm this seems like the hard end goal…. Can prototype…
- Can we get multiline strings?? YES
- Table data (spreadsheets!)
- Observe API, add+remove observations, use notifs + polling
- Tree views: pruning + contraction
- Deprecate watch+unwatch (or just internal as add/remove trait via observers?)
- Remove destruction special case? What about structural changes as a category?
- Can we get filepaths? What about refs to entries in e.g. sqlite for Zotero and others? How do we bridge to structured data?
- What does a query api look like? ARIA CSS selectors? observe selection? How in the world do we make this GLOBAL and efficient? That’s THE goal right?
- Need a more robust WS discovery mechanism… what do other systems do?
- Need non-overlay tests/demos: CLI tool, non-overlay web page…
- Need to handle offscreen stuff correctly, currently we just stop polling but this is hacky, we still getElementAt… what’s the approach here? Can our approach double as cleanup/preparation for multiscreen and multi window?
