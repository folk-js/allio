/*!
macOS platform implementation.

All macOS-specific code lives here:
- `handles`: `ElementHandle`, `ObserverHandle` implementations
- `mapping`: Bidirectional mappings between our types and macOS AX* strings
- `observer`: Observer management and unified callback
- `element`: Element building, discovery, and operations
- `focus`: Focus and selection handling
- `notifications`: Notification subscription management
- `window`: Window-related operations
- `window_list`: `CGWindowList` enumeration
- `display`: Screen dimensions
- `display_link`: `CVDisplayLink` for vsync callbacks
- `mouse`: Mouse position tracking
- `cf_utils`: Core Foundation helpers
- `util`: Shared utilities
*/

// Core handles
mod handles;
pub(crate) use handles::{ElementHandle, ObserverHandle};

// Accessibility
mod element;
mod focus;
pub(crate) mod mapping;
mod notifications;
mod observer;
mod window;

pub(crate) use element::{
  children, click_element, element_hash, parent, refresh_element, write_element_value,
};
pub(crate) use focus::get_current_focus;
pub(crate) use notifications::{
  subscribe_app_notifications, subscribe_destruction_notification, subscribe_notifications,
  unsubscribe_destruction_notification, unsubscribe_notifications,
};
pub(crate) use observer::{create_observer_for_pid, ObserverContextHandle};
pub(crate) use window::{
  enable_accessibility_for_pid, fetch_window_handle, get_element_at_position, get_window_root,
};

// Window enumeration
mod cf_utils;
mod window_list;
pub(crate) use window_list::enumerate_windows;

// Display and input
mod display;
mod display_link;
mod mouse;

pub(crate) use display::get_main_screen_dimensions;
pub(crate) use display_link::{start_display_link, DisplayLinkHandle};
pub(crate) use mouse::get_mouse_position;

// Utilities
mod util;
pub(crate) use util::check_accessibility_permissions;
