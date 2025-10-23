## Architecture

- [ ] Separate AXIO from Overlay concerns

  - AXIO: window polling, accessibility trees, WebSocket server
  - Overlay: HTML loading, transparent window rendering, Tauri window management
  - Keep them in separate modules/files despite coupling

- [ ] Consolidate duplicate ID generation functions
  - `generate_stable_element_id()` exists in both platform/macos.rs and node_watcher.rs
  - Move to single location in platform/macos.rs

## Cleanup

- [x] Remove unused `push_tree_for_window()` in WebSocketState (marked dead_code)
- [ ] Consider removing `NodeUpdate.path` field (comment says "not used for identification")
- [x] Refactor WebSocket message handler chain (300+ lines of if-else)
  - Use match statement or dispatch table
- [x] Use string constants/enums for message type strings instead of literals
- [x] Create generic `Response<T>` type to reduce duplication across response structs
- [x] Consider `lazy_static` or `once_cell` for bundle ID cache initialization

## Current Work

- [ ] setup the macOS window as a panel, with NSNonactivatingPanelMask, so you can interact with it without changing it to focused. https://forum.juce.com/t/making-a-floating-window-that-doesnt-bring-the-application-to-the-front/36963
- [ ] figure out why there are 'ghosts' in the frontend, like semitransparent imprints of DIVs or the edges of the sand sim particles.
