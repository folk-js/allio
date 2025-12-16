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

pub use crate::core::{Allio, AllioBuilder};
pub use crate::observation::{ObservationHandle, ObserveConfig};
