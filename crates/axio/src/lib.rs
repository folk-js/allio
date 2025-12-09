/*!
AXIO - Accessibility I/O Layer
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

#[cfg(feature = "ws")]
pub mod ws;

pub mod accessibility;

mod types;
pub use types::*;

mod api;
pub use api::{elements, screen, windows};

pub use events::subscribe;

pub use polling::{PollingHandle, PollingOptions};

/// Check if accessibility permissions are granted.
pub fn verify_permissions() -> bool {
  platform::check_accessibility_permissions()
}

/// Get a snapshot of the current registry state for sync.
/// Note: `accessibility_enabled` field will be `false` - caller should set it.
pub fn snapshot() -> Snapshot {
  registry::snapshot()
}

/// Start background polling for windows and mouse position.
///
/// Returns a [`PollingHandle`] that controls the polling lifetime.
/// Polling will stop when the handle is dropped or [`PollingHandle::stop`] is called.
pub fn start_polling(config: PollingOptions) -> PollingHandle {
  polling::start_polling(config)
}
