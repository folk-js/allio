//! Window operations
//!
//! All functions for querying windows and their state.

use crate::platform;
use crate::types::{AXElement, AXWindow, AxioResult, WindowId};
use crate::window_registry;

/// Get all current windows.
pub fn all() -> Vec<AXWindow> {
  window_registry::get_windows()
}

/// Get a specific window by ID.
pub fn get(window_id: &WindowId) -> Option<AXWindow> {
  window_registry::get_window(window_id)
}

/// Get the active window ID (preserved when desktop is focused).
pub fn active() -> Option<WindowId> {
  window_registry::get_active()
}

/// Get window IDs in depth order (front to back).
pub fn depth_order() -> Vec<WindowId> {
  window_registry::get_depth_order()
}

/// Get the root element for a window.
pub fn root(window_id: &WindowId) -> AxioResult<AXElement> {
  platform::get_window_root(window_id)
}

