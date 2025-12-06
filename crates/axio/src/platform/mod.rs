/**
 * Platform Abstraction Layer
 *
 * This module provides a platform-agnostic interface for accessibility APIs.
 * Each platform (macOS, Windows, Linux) implements the same function signatures.
 *
 * Usage: `crate::platform::function_name()` automatically resolves to the
 * correct platform implementation at compile time.
 */

// Platform-specific implementations
#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::*;

// Cross-platform modules (with platform-specific implementations inside)
mod display;
mod mouse;

pub use display::get_main_screen_dimensions;
pub use mouse::get_mouse_position;

// Future platform support
// #[cfg(target_os = "windows")]
// pub mod windows;
// #[cfg(target_os = "windows")]
// pub use windows::*;
//
// #[cfg(target_os = "linux")]
// pub mod linux;
// #[cfg(target_os = "linux")]
// pub use linux::*;
