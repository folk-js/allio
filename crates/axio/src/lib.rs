/*!
AXIO - Accessibility I/O Layer

```ignore
use axio::{Axio, Freshness};

// Create instance (polling starts automatically)
let axio = Axio::new()?;

// Query state with explicit freshness
let windows = axio.all_windows();
let element = axio.get(element_id, Freshness::Cached)?;      // From cache (fast)
let element = axio.get(element_id, Freshness::Fresh)?;       // From OS (slow)
let element = axio.get(element_id, Freshness::max_age_ms(100))?; // Refresh if stale

// Traversal with freshness
let children = axio.children(element.id, Freshness::Fresh)?;
let parent = axio.parent(element.id, Freshness::Cached)?;

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

pub use crate::core::Axio;
pub use crate::polling::AxioOptions;
