/*!
macOS platform implementation.

Contains macOS-specific accessibility code:
- `mapping`: Bidirectional mappings between our types and macOS AX* strings
- `observer`: Observer management and unified callback
- `element`: Element building, discovery, and operations
- `focus`: Focus and selection handling
- `notifications`: Notification subscription management
- `window`: Window-related operations
- `util`: Shared utilities
*/

mod element;
mod focus;
pub(crate) mod mapping;
mod notifications;
mod observer;
mod util;
mod window;

// Re-export crate-internal API items
pub(crate) use element::{
  children, click_element, element_hash, parent, refresh_element, write_element_value,
};
pub(crate) use focus::get_current_focus;
pub(crate) use notifications::{
  subscribe_app_notifications, subscribe_destruction_notification, subscribe_notifications,
  unsubscribe_destruction_notification, unsubscribe_notifications,
};
pub(crate) use observer::{create_observer_for_pid, ObserverContextHandle};
pub(crate) use util::check_accessibility_permissions;
pub(crate) use window::{
  enable_accessibility_for_pid, fetch_window_handle, get_element_at_position, get_window_root,
};
