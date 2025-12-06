//! AXIO - Accessibility I/O Layer
//!
//! Window tracking and accessibility operations for macOS (future: Windows, Linux).

mod types;
pub use types::{
  AXAction, AXElement, AXRole, AXValue, AXWindow, AxioError, AxioResult, Bounds, ElementId, Point,
  ProcessId, Selection, ServerEvent, SyncInit, TextRange, WindowId,
};

pub mod events;
pub use events::{set_event_sink, EventSink, NoopEventSink};

pub mod api;
pub mod element_registry;
pub mod platform;
pub mod window_registry;
pub mod windows;

pub use api::{
  children, click, element_at, get, get_current_focus, get_many, refresh, unwatch, watch,
  window_root, write,
};
pub use window_registry::{get_active, get_depth_order, get_window, get_windows};
pub use windows::{get_main_screen_dimensions, start_polling};
