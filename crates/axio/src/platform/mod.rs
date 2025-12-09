/*! Platform Abstraction Layer.

This module provides platform-agnostic interfaces to OS-specific functionality.
The actual implementations live in platform-specific submodules (e.g., `macos/`).
*/

use crate::accessibility::{Action, Value};
use crate::types::Bounds;

/// All commonly-needed element attributes, fetched in a batch for performance.
/// This is the cross-platform interface between platform code and core.
#[derive(Debug, Default)]
pub(crate) struct ElementAttributes {
  pub(crate) role: Option<String>,
  pub(crate) subrole: Option<String>,
  pub(crate) title: Option<String>,
  pub(crate) value: Option<Value>,
  pub(crate) description: Option<String>,
  pub(crate) placeholder: Option<String>,
  pub(crate) url: Option<String>,
  pub(crate) bounds: Option<Bounds>,
  pub(crate) focused: Option<bool>,
  pub(crate) disabled: bool,
  pub(crate) selected: Option<bool>,
  pub(crate) expanded: Option<bool>,
  pub(crate) row_index: Option<usize>,
  pub(crate) column_index: Option<usize>,
  pub(crate) row_count: Option<usize>,
  pub(crate) column_count: Option<usize>,
  pub(crate) actions: Vec<Action>,
}

// Platform implementations
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
  // Accessibility
  check_accessibility_permissions,
  children,
  click_element,
  create_observer_for_pid,
  element_hash,
  enable_accessibility_for_pid,
  // Window enumeration
  enumerate_windows,
  fetch_window_handle,
  get_current_focus,
  get_element_at_position,
  // Display and input
  get_main_screen_dimensions,
  get_mouse_position,
  get_window_root,
  parent,
  refresh_element,
  start_display_link,
  subscribe_app_notifications,
  subscribe_destruction_notification,
  subscribe_notifications,
  unsubscribe_destruction_notification,
  unsubscribe_notifications,
  write_element_value,
  DisplayLinkHandle,
  // Handles
  ElementHandle,
  ObserverContextHandle,
  ObserverHandle,
};

#[cfg(target_os = "windows")]
compile_error!("Windows support is not yet implemented");

#[cfg(target_os = "linux")]
compile_error!("Linux support is not yet implemented");

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("Unsupported platform - AXIO only supports macOS currently");
