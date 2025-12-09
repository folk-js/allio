/*!
AXIO - Accessibility I/O Layer

```ignore
use axio::Axio;

// Create instance (polling starts automatically)
let axio = Axio::new()?;

// Query state (get_ = registry lookup)
let windows = axio.get_windows();
let element = axio.get_element(element_id)?;

// Fetch from platform (fetch_ = OS call)
let element = axio.fetch_element_at(100.0, 200.0)?;
let children = axio.fetch_children(element.id, 100)?;

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
