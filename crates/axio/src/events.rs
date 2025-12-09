/*!
Event broadcasting for AXIO.

Uses async-broadcast for multi-subscriber event streaming.
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
use async_broadcast::{broadcast, InactiveReceiver, Sender};
use std::sync::LazyLock;

const CHANNEL_CAPACITY: usize = 1000;

/// Holds both sender and an inactive receiver to keep the channel alive.
/// The inactive receiver prevents the channel from closing when no active receivers exist.
static EVENT_CHANNEL: LazyLock<(Sender<Event>, InactiveReceiver<Event>)> = LazyLock::new(|| {
  let (mut tx, rx) = broadcast(CHANNEL_CAPACITY);
  tx.set_overflow(true); // Drop oldest messages when full instead of blocking
  (tx, rx.deactivate())
});

/// Subscribe to events.
pub fn subscribe() -> async_broadcast::Receiver<Event> {
  EVENT_CHANNEL.1.activate_cloned()
}

/// Emit an event to all subscribers.
pub(crate) fn emit(event: Event) {
  // try_broadcast returns immediately - we ignore errors (no receivers or overflow)
  drop(EVENT_CHANNEL.0.try_broadcast(event));
}
