//! macOS platform implementation.
//!
//! This module contains macOS-specific accessibility code, organized into:
//! - `mapping`: Bidirectional mappings between our types and macOS AX* strings
//! - `observer`: Observer management and unified callback
//! - `element`: Element building, discovery, and operations
//! - `focus`: Focus and selection handling
//! - `notifications`: Notification subscription management
//! - `window`: Window-related operations
//! - `util`: Shared utilities

pub mod element;
pub mod focus;
pub mod mapping;
pub mod notifications;
pub mod observer;
pub mod util;
pub mod window;

// Re-export public API items
pub use element::{click_element, discover_children, element_hash, refresh_element, write_element_value};
pub use focus::get_current_focus;
pub use notifications::{
  subscribe_app_notifications, subscribe_destruction_notification, subscribe_notifications,
  unsubscribe_destruction_notification, unsubscribe_notifications,
};
pub use observer::{create_observer_for_pid, ObserverContextHandle};
pub use util::check_accessibility_permissions;
pub use window::{
  enable_accessibility_for_pid, fetch_window_handle, get_element_at_position, get_window_root,
};
