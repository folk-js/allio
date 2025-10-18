- Automatically toggle cursor transparency on mouse move. If cursor is over an element which contains a 'data-solid' attribute in its ancestry, make non-transparent, else it should be transparent.
- setup the macOS window as a panel, with NSNonactivatingPanelMask, so you can interact with it without changing it to focused. https://forum.juce.com/t/making-a-floating-window-that-doesnt-bring-the-application-to-the-front/36963
- figure out why there are 'ghosts' in the frontend, like semitransparent imprints of DIVs or the edges of the sand sim particles.
- fix panic: thread '<unnamed>' panicked at /Users/orion/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/objc2-app-kit-0.3.1/src/generated/NSRunningApplication.rs:93:5:
  messsaging isActive to nil

- filter out screenshot app
