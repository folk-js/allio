/*!
Platform Abstraction Layer.

This module defines the contract between core code and platform implementations.
Core code should only import from this module - never from platform-specific submodules.

# Architecture

- `Platform` trait: static methods for OS-level operations
- `PlatformHandle` trait: per-element operations
- `PlatformObserver` trait: notification subscriptions
- `CurrentPlatform` type alias: the platform for the current OS
- `Handle`/`Observer` type aliases: opaque handles for core code

Core code uses `CurrentPlatform::method()` for platform operations.
All platform-specific details (CFType, AXUIElement, etc.) stay hidden.

# Adding a New Platform

1. Create `platform/newos/mod.rs`
2. Implement `Platform`, `PlatformHandle`, `PlatformObserver` traits
3. Add conditional compilation in this file
*/

pub(crate) mod element_ops;
mod traits;

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
