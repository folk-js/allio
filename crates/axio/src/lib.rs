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
/// Returns a [`PollingHandle`] that controls the polling thread lifetime.
/// The polling will stop when the handle is dropped or [`PollingHandle::stop`] is called.
///
/// # Example
///
/// ```ignore
/// let handle = axio::start_polling(PollingOptions::default());
/// // Polling runs until handle is dropped or stop() is called
/// handle.stop();
/// ```
pub fn start_polling(config: PollingOptions) -> PollingHandle {
  polling::start_polling(config)
}
