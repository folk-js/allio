/*! Platform Abstraction Layer */

mod handles;
pub use handles::{ElementHandle, ObserverHandle};

// macOS-specific implementations
pub mod macos;
mod macos_cf;
mod macos_windows;

pub use macos::{
  // Core functionality
  check_accessibility_permissions,
  children,
  click_element,
  create_observer_for_pid,
  element_hash,
  enable_accessibility_for_pid,
  fetch_window_handle,
  get_current_focus,
  get_element_at_position,
  get_window_root,
  parent,
  refresh_element,
  // Notification subscriptions
  subscribe_app_notifications,
  subscribe_destruction_notification,
  subscribe_notifications,
  unsubscribe_destruction_notification,
  unsubscribe_notifications,
  write_element_value,
  // Context management (re-exported for registry use)
  ObserverContextHandle,
};

pub use macos_windows::enumerate_windows;

mod display;
mod mouse;

pub use display::get_main_screen_dimensions;
pub use mouse::get_mouse_position;

mod display_link;
pub use display_link::{start_display_link, DisplayLinkHandle};
