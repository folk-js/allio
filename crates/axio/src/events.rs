/*!
Event broadcasting for AXIO.

Uses a tokio broadcast channel for multi-subscriber event streaming.
Events are emitted when Registry state changes.

# Usage

```ignore
// Initialize once at startup
let rx = axio::events::init();

// Subscribe additional receivers
let rx2 = axio::events::subscribe().unwrap();

// Receive events
while let Ok(event) = rx.recv().await {
    // handle event
}
```

# Why Global?

macOS accessibility callbacks fire on the main thread and need to emit events.
A global channel is simpler than threading senders through C callback contexts.
*/

use crate::types::Event;
use std::sync::OnceLock;
use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 1000;

static EVENT_CHANNEL: OnceLock<broadcast::Sender<Event>> = OnceLock::new();

/// Initialize the event broadcast channel. Returns a receiver.
///
/// # Panics
/// Panics if called more than once.
pub fn init() -> broadcast::Receiver<Event> {
    let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
    EVENT_CHANNEL
        .set(tx)
        .expect("axio::events::init() called more than once");
    rx
}

/// Subscribe to events. Returns None if init() hasn't been called.
pub fn subscribe() -> Option<broadcast::Receiver<Event>> {
    EVENT_CHANNEL.get().map(|tx| tx.subscribe())
}

/// Emit an event to all subscribers.
/// No-op if init() hasn't been called (events are silently dropped).
pub(crate) fn emit(event: Event) {
    if let Some(tx) = EVENT_CHANNEL.get() {
        // Ignore send errors - just means no receivers
        let _ = tx.send(event);
    }
}
