/*!
Platform Abstraction Layer.

This module defines the contract between core code and platform implementations.
Core code should only import from this module - never from platform-specific submodules.

# Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                    Core (uses traits)                        │
│  - Only sees: Handle, Observer, Platform functions           │
│  - Never sees: CFType, AXUIElement, etc.                     │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│              platform/mod.rs (THE BOUNDARY)                  │
│  - Trait definitions                                         │
│  - Type aliases for current platform                         │
│  - Re-exported platform functions                            │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│              platform/macos/ (implementation)                │
│  - impl Platform for MacOS                                   │
│  - impl PlatformHandle for ElementHandle                     │
│  - All macOS-specific code                                   │
└─────────────────────────────────────────────────────────────┘
```

# Adding a New Platform

1. Create `platform/newos/mod.rs`
2. Implement `Platform`, `PlatformHandle`, `PlatformObserver` traits
3. Add conditional compilation in this file
*/

mod traits;
pub(crate) mod element_ops;

pub(crate) use traits::{
  DisplayLinkHandle, ElementAttributes, Platform, PlatformHandle, PlatformObserver, WatchHandle,
};

// === Platform Implementations ===

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(target_os = "windows")]
compile_error!("Windows support is not yet implemented");

#[cfg(target_os = "linux")]
compile_error!("Linux support is not yet implemented");

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("Unsupported platform - AXIO only supports macOS currently");

// === Type Aliases for Current Platform ===

/// The platform implementation for the current OS.
#[cfg(target_os = "macos")]
pub(crate) type CurrentPlatform = macos::MacOS;

/// Opaque handle to a UI element.
/// Core code can hold and clone this, but cannot inspect its contents.
pub(crate) type Handle = <CurrentPlatform as Platform>::Handle;

/// Opaque handle to a notification observer.
pub(crate) type Observer = <CurrentPlatform as Platform>::Observer;

// === Convenience Functions ===
// These delegate to CurrentPlatform methods for ergonomic use.
// Naming convention: get_ = registry, fetch_ = OS call, set_ = value, perform_ = action

use crate::core::Axio;
use crate::types::{AXWindow, AxioResult, Point};

/// Check if accessibility permissions are granted.
pub(crate) fn check_accessibility_permissions() -> bool {
  CurrentPlatform::check_permissions()
}

/// Create an observer for a process.
pub(crate) fn create_observer(pid: u32, axio: Axio) -> AxioResult<Observer> {
  CurrentPlatform::create_observer(pid, axio)
}

/// Get the root element handle for a window (from window info, not OS call).
pub(crate) fn get_window_handle(window: &AXWindow) -> Option<Handle> {
  CurrentPlatform::window_handle(window)
}

/// Start a display-linked callback.
pub(crate) fn start_display_link<F: Fn() + Send + Sync + 'static>(
  callback: F,
) -> Option<DisplayLinkHandle> {
  CurrentPlatform::start_display_link(callback)
}

/// Fetch all visible windows from OS.
pub(crate) fn fetch_windows() -> Vec<AXWindow> {
  CurrentPlatform::fetch_windows(None)
}

/// Fetch main screen dimensions (width, height) from OS.
pub(crate) fn fetch_screen_size() -> (f64, f64) {
  CurrentPlatform::fetch_screen_size()
}

/// Fetch current mouse position from OS.
pub(crate) fn fetch_mouse_position() -> Option<Point> {
  Some(CurrentPlatform::fetch_mouse_position())
}

/// Enable accessibility for a process (mostly for Chromium/Electron apps).
pub(crate) fn enable_accessibility_for_pid(pid: u32) {
  CurrentPlatform::enable_accessibility_for_pid(pid);
}

/// Convert raw platform role string to our Role enum.
#[cfg(target_os = "macos")]
pub(crate) fn role_from_raw(raw: &str) -> crate::accessibility::Role {
  macos::mapping::role_from_macos(raw)
}
