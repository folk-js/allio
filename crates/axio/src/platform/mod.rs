/*! Platform Abstraction Layer */

// Opaque handle types (platform-specific implementations)
mod handles;
pub use handles::{ElementHandle, ObserverHandle};

// Platform-specific implementations
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_cf;
#[cfg(target_os = "macos")]
mod macos_windows;

// New organized macOS modules (migration in progress)
// Will eventually replace macos.rs, macos_cf.rs, macos_windows.rs
#[cfg(target_os = "macos")]
pub mod macos_platform;

#[cfg(target_os = "macos")]
pub use macos_platform::mapping as macos_mapping;

#[cfg(target_os = "macos")]
pub use macos::{
  check_accessibility_permissions, cleanup_dead_observers, click_element, create_observer_for_pid,
  discover_children, element_hash, enable_accessibility_for_pid, fetch_window_handle,
  get_current_focus, get_element_at_position, get_window_root, refresh_element,
  subscribe_destruction_notification, subscribe_element_notifications,
  unsubscribe_destruction_notification, unsubscribe_element_notifications, write_element_value,
  AXNotification, ObserverContextHandle,
};

#[cfg(target_os = "macos")]
pub use macos_windows::enumerate_windows;

// Cross-platform modules (with platform-specific implementations inside)
mod display;
mod mouse;

pub use display::get_main_screen_dimensions;
pub use mouse::get_mouse_position;
