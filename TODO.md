- [ ] Separate AXIO from Overlay concerns

  - AXIO: window polling, accessibility trees, WebSocket server
  - Overlay: HTML loading, transparent window rendering, Tauri window management
  - Keep them in separate modules/files despite coupling

- [ ] Consolidate duplicate ID generation functions

  - `generate_stable_element_id()` exists in both platform/macos.rs and node_watcher.rs
  - Move to single location in platform/macos.rs?

- [ ] Use match statement or dispatch table?
- [ ] setup the macOS window as a panel, with NSNonactivatingPanelMask, so you can interact with it without changing it to focused. https://forum.juce.com/t/making-a-floating-window-that-doesnt-bring-the-application-to-the-front/36963
- [ ] figure out why there are 'ghosts' in the frontend, like semitransparent imprints of DIVs or the edges of the sand sim particles.
