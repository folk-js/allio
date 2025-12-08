/*!
AXIO - Accessibility I/O Layer

Cross-platform accessibility API for discovering and interacting with UI elements.

```ignore
use axio::{elements, windows, AXElement};

let all_windows = windows::all();
let element = elements::at(100.0, 200.0)?;
let children = elements::children(&element.id, 100)?;
```
*/

// Internal modules - not accessible outside this crate
mod events;
mod platform;
mod polling;
mod registry;

// Cross-platform accessibility abstractions (new)
pub mod accessibility;

// Types - re-export everything at crate root
mod types;
pub use types::*;

// Public API modules
mod api;
pub use api::{elements, screen, windows};

// Events - just the sink setup, not emit()
pub use events::{set_event_sink, EventSink, NoopEventSink};

// Polling
pub use polling::{PollingHandle, PollingOptions};

/// Check if accessibility permissions are granted.
pub fn verify_permissions() -> bool {
  platform::check_accessibility_permissions()
}

/// Start background polling for windows and mouse position.
///
/// Returns a [`PollingHandle`] that controls the polling lifetime.
/// Polling will stop when the handle is dropped or [`PollingHandle::stop`] is called.
///
/// On macOS with `use_display_link: true` (the default), polling is synchronized
/// to the display's refresh rate (60Hz, 120Hz, etc.) for optimal frame alignment
/// and zero timing drift.
///
/// # Example
///
/// ```ignore
/// // Default: uses display-synced polling on macOS
/// let handle = axio::start_polling(PollingOptions::default());
///
/// // Or explicitly use thread-based polling
/// let handle = axio::start_polling(PollingOptions {
///     use_display_link: false,
///     interval_ms: 16, // ~60fps
///     ..Default::default()
/// });
///
/// // Polling runs until handle is dropped or stop() is called
/// handle.stop();
/// ```
pub fn start_polling(config: PollingOptions) -> PollingHandle {
  polling::start_polling(config)
}
