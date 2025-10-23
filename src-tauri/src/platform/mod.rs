/**
 * Platform Abstraction Layer
 *
 * This module provides a platform-agnostic interface for accessibility APIs.
 * Each platform (macOS, Windows, Linux) implements the conversion from its
 * native accessibility API to the common AXIO format.
 */

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub mod ax_private;

#[cfg(target_os = "macos")]
pub use macos::*;

// Future platform support
// #[cfg(target_os = "windows")]
// pub mod windows;
//
// #[cfg(target_os = "linux")]
// pub mod linux;
