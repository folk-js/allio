mod types;
pub use types::{
  AXAction, AXElement, AXRole, AXValue, AXWindow, AxioError, AxioResult, Bounds, ElementId, Point,
  ProcessId, Selection, ServerEvent, SyncInit, TextRange, WindowId,
};

pub mod events;
pub use events::{set_event_sink, EventSink, NoopEventSink};

pub mod api;
pub mod element_registry;
pub mod window_registry;
pub mod windows;

// Platform module is internal - only expose cross-platform utilities
pub(crate) mod platform;
pub use platform::{
  check_accessibility_permissions, get_main_screen_dimensions, get_mouse_position,
};

pub use api::{
  children, click, element_at, get, get_current_focus, get_many, refresh, unwatch, watch,
  window_root, write,
};
pub use window_registry::{get_active, get_depth_order, get_window, get_windows};
pub use windows::start_polling;
