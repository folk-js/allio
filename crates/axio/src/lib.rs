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
mod element_registry;
mod events;
mod platform;
mod polling;
mod window_registry;

// Types - re-export everything at crate root
mod types;
pub use types::*;

// Public API modules
mod api;
pub use api::{elements, screen, windows};

// Events - just the sink setup, not emit()
pub use events::{set_event_sink, EventSink, NoopEventSink};

// Polling config
pub use polling::PollingOptions;

/// Check if accessibility permissions are granted.
pub fn verify_permissions() -> bool {
  platform::check_accessibility_permissions()
}

/// Start background polling for windows and mouse position.
pub fn start_polling(config: PollingOptions) {
  polling::start_polling(config);
}
