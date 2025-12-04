//! AXIO - Accessibility I/O Layer
//!
//! Window tracking and accessibility operations for macOS (future: Windows, Linux).

mod types;
pub use types::{
    AXElement, AXRole, AXValue, AXWindow, AxioError, AxioResult, Bounds, ElementId, Position,
    ServerEvent, Size, WindowId,
};

pub mod events;
pub use events::{set_event_sink, EventSink, NoopEventSink};

pub mod api;
pub mod element_registry;
pub mod platform;
pub mod window_manager;
pub mod windows;

pub use api::{children, click, element_at, get, get_many, refresh, unwatch, watch, write};
pub use windows::{get_current_windows, get_main_screen_dimensions, get_windows, start_polling};
