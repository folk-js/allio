/*!
Event broadcasting for AXIO.

Uses a tokio broadcast channel for multi-subscriber event streaming.
Events are emitted when Registry state changes.

# Usage

```ignore
// Subscribe to events (channel is lazily initialized)
let mut rx = axio::subscribe();

// Receive events
while let Ok(event) = rx.recv().await {
    // handle event
}
```
*/

use crate::types::Event;
use std::sync::LazyLock;
use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 1000;

static EVENT_CHANNEL: LazyLock<broadcast::Sender<Event>> =
  LazyLock::new(|| broadcast::channel(CHANNEL_CAPACITY).0);

/// Subscribe to events.
pub fn subscribe() -> broadcast::Receiver<Event> {
  EVENT_CHANNEL.subscribe()
}

/// Emit an event to all subscribers.
pub(crate) fn emit(event: Event) {
  // Ignore send errors - just means no receivers
  drop(EVENT_CHANNEL.send(event));
}
