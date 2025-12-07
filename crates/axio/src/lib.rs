//! AXIO - Accessibility I/O Layer
//!
//! Cross-platform accessibility API for discovering and interacting with UI elements.
//!
//! # Usage
//!
//! ```ignore
//! use axio::{elements, windows};
//!
//! // Get all windows
//! let all_windows = windows::all();
//!
//! // Get element at coordinates
//! let element = elements::at(100.0, 200.0)?;
//!
//! // Get element children
//! let children = elements::children(&element.id, 100)?;
//! ```

// === Internal modules (not exposed) ===
pub(crate) mod element_registry;
pub(crate) mod platform;
pub(crate) mod polling;
pub(crate) mod window_registry;

// === Types ===
mod types;
pub use types::{
  // Core data
  AXAction,
  AXElement,
  AXRole,
  AXValue,
  AXWindow,
  // Errors
  AxioError,
  AxioResult,
  // Geometry
  Bounds,
  // IDs
  ElementId,
  // Events
  Event,
  Point,
  ProcessId,
  Selection,
  SyncInit,
  TextRange,
  WindowId,
};

// === Public API modules ===
mod api;
pub use api::elements;
pub use api::screen;
pub use api::windows;

// === Events ===
pub(crate) mod events;
pub use events::{set_event_sink, EventSink, NoopEventSink};

// === Polling configuration ===
pub use polling::{PollingConfig, WindowEnumOptions};

// === Lifecycle ===

/// Check if accessibility permissions are granted.
/// Returns true if trusted, false otherwise.
pub fn verify_permissions() -> bool {
  platform::check_accessibility_permissions()
}

/// Start background polling for windows and mouse position.
pub fn start_polling(config: PollingConfig) {
  polling::start_polling(config);
}
