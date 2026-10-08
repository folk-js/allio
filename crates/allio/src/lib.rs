/*!
Allio - Accessibility (A11y) I/O Layer

```ignore
use allio::{Allio, Recency};

// Create instance (polling starts automatically)
let allio = Allio::new()?;

// Query state with explicit recency
let windows = allio.all_windows();
let element = allio.get(element_id, Recency::Any)?;      // From cache (fast)
let element = allio.get(element_id, Recency::Current)?;       // From OS (slow)
let element = allio.get(element_id, Recency::max_age_ms(100))?; // Refresh if stale

// Traversal with recency
let children = allio.children(element.id, Recency::Current)?;
let parent = allio.parent(element.id, Recency::Any)?;

// Subscribe to events
let mut events = allio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}

// Polling stops when allio is dropped
drop(allio);
```
*/

mod core;
mod observation;
mod platform;
mod polling;

pub mod a11y;

mod types;
pub use types::*;

pub use crate::core::{Allio, AllioBuilder, SetOptions};
pub use crate::observation::{ObservationHandle, ObserveConfig};

/// Puts one of this process's own windows on exactly one Space (by its window number). Call it
/// once `AppKit` has ordered the window in: ordering in puts a window on the current Space,
/// undoing an earlier placement. Returns whether it is now there.
#[cfg(target_os = "macos")]
pub fn place_window_on_space(window_number: u32, space: SpaceId) -> bool {
  platform::macos::skylight::place_window_on_space(window_number, space)
}
