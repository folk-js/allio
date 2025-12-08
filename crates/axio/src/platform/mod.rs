/*! Platform Abstraction Layer */

// Opaque handle types (platform-specific implementations)
mod handles;
pub use handles::{ElementHandle, ObserverHandle};

// Platform-specific implementations
#[cfg(target_os = "macos")]
mod macos_cf;
#[cfg(target_os = "macos")]
mod macos_windows;

// macOS platform modules
#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
pub use macos_windows::enumerate_windows;

// Cross-platform modules (with platform-specific implementations inside)
mod display;
mod mouse;

pub use display::get_main_screen_dimensions;
pub use mouse::get_mouse_position;

// Display-synced callback support (macOS only for now)
#[cfg(target_os = "macos")]
mod display_link;
#[cfg(target_os = "macos")]
pub use display_link::{start_display_link, DisplayLinkHandle};
