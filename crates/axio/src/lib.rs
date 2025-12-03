//! AXIO - Accessibility I/O Layer
//!
//! Provides window tracking and accessibility operations:
//! - Window enumeration and polling
//! - Accessibility element operations (read, write, watch)
//! - Type-safe element and window identifiers
//! - AXObserver-based change notifications
//!
//! # Example
//!
//! ```ignore
//! use axio::{api, windows, events, ElementId};
//!
//! // Set up event handling
//! events::set_event_sink(MyEventHandler);
//!
//! // Start window polling
//! windows::start_polling(windows::PollingConfig::default());
//!
//! // Get element at screen position
//! let element = api::element_at(100.0, 200.0)?;
//!
//! // Watch for changes
//! api::watch(&element.id)?;
//!
//! // Write to text field
//! api::write(&element.id, "Hello, world!")?;
//! ```

// Core types
mod types;
pub use types::*;

// Event system (trait-based, decoupled from transport)
pub mod events;
pub use events::{set_event_sink, EventSink, NoopEventSink};

// RPC dispatch (for WebSocket/HTTP servers)
pub mod rpc;

// Window enumeration and polling
pub mod windows;

// Internal modules
pub mod element_registry;
mod ui_element;
pub mod window_manager;

// Platform-specific implementations
pub mod platform;

// Public API
pub mod api;

// Re-export commonly used items at crate root
pub use api::{click, element_at, tree, unwatch, watch, write};
pub use windows::{get_main_screen_dimensions, get_windows, start_polling};
