/*!
AXIO - Accessibility I/O Layer

```ignore
use axio::{Axio, Recency};

// Create instance (polling starts automatically)
let axio = Axio::new()?;

// Query state with explicit freshness
let windows = axio.all_windows();
let element = axio.get(element_id, Recency::Any)?;      // From cache (fast)
let element = axio.get(element_id, Recency::Current)?;       // From OS (slow)
let element = axio.get(element_id, Recency::max_age_ms(100))?; // Refresh if stale

// Traversal with freshness
let children = axio.children(element.id, Recency::Current)?;
let parent = axio.parent(element.id, Recency::Any)?;

// Subscribe to events
let mut events = axio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}

// Polling stops when axio is dropped
drop(axio);
```
*/

mod core;
mod platform;
mod polling;

pub mod accessibility;

mod types;
pub use types::*;

pub use crate::core::{Axio, AxioBuilder};
