/*!
Core Axio instance - owns all accessibility state and event broadcasting.

This is the main entry point for the axio library. Create an `Axio` instance
and use its methods to interact with the accessibility tree.

# Example

```ignore
let axio = Axio::new()?;

let element = axio.element_at(100.0, 200.0)?;
let children = axio.children(element.id, 100)?;

let mut events = axio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}
```
*/

pub(crate) mod element_ops;
mod mutations;
mod queries;
mod state;
mod subscriptions;

pub(crate) use state::State;

use crate::platform;
use crate::polling::{self, AxioOptions, PollingHandle};
use crate::types::{AxioError, AxioResult, Event};
use async_broadcast::{InactiveReceiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

const EVENT_CHANNEL_CAPACITY: usize = 5000;

/// Main axio instance - owns state, event broadcasting, and polling.
///
/// Polling starts automatically when created and stops when dropped.
/// Clone is cheap (Arc bumps) - share freely across threads.
pub struct Axio {
  pub(crate) state: Arc<RwLock<State>>,
  events_tx: Sender<Event>,
  events_keepalive: InactiveReceiver<Event>,
  polling: Arc<Mutex<Option<PollingHandle>>>,
}

impl Clone for Axio {
  fn clone(&self) -> Self {
    Self {
      state: Arc::clone(&self.state),
      events_tx: self.events_tx.clone(),
      events_keepalive: self.events_keepalive.clone(),
      polling: Arc::clone(&self.polling),
    }
  }
}

impl std::fmt::Debug for Axio {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Axio").finish_non_exhaustive()
  }
}

impl Axio {
  /// Create a new Axio instance with default options.
  ///
  /// Polling starts automatically and stops when the instance is dropped.
  pub fn new() -> AxioResult<Self> {
    Self::with_options(AxioOptions::default())
  }

  /// Create a new Axio instance with custom options.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let axio = Axio::with_options(AxioOptions {
  ///     exclude_pid: Some(ProcessId::from(std::process::id())),
  ///     ..Default::default()
  /// })?;
  /// ```
  pub fn with_options(options: AxioOptions) -> AxioResult<Self> {
    if !platform::check_accessibility_permissions() {
      return Err(AxioError::PermissionDenied);
    }

    let (mut tx, rx) = async_broadcast::broadcast(EVENT_CHANNEL_CAPACITY);
    tx.set_overflow(true); // Drop oldest messages when full

    let axio = Axio {
      state: Arc::new(RwLock::new(State::new())),
      events_tx: tx,
      events_keepalive: rx.deactivate(),
      polling: Arc::new(Mutex::new(None)),
    };

    // Start polling with a clone (shares state via Arc)
    let polling_handle = polling::start_polling(axio.clone(), options);
    *axio.polling.lock() = Some(polling_handle);

    Ok(axio)
  }

  /// Subscribe to events from this instance.
  pub fn subscribe(&self) -> async_broadcast::Receiver<Event> {
    self.events_keepalive.activate_cloned()
  }

  /// Emit an event to all subscribers.
  pub(crate) fn emit(&self, event: Event) {
    drop(self.events_tx.try_broadcast(event));
  }

  /// Emit multiple events.
  pub(crate) fn emit_all(&self, events: impl IntoIterator<Item = Event>) {
    for event in events {
      self.emit(event);
    }
  }
}
