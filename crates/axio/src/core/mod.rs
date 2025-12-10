/*!
Core Axio instance - owns all accessibility state and event broadcasting.

# Module Structure

- `mod.rs` - Axio struct, construction, events
- `state.rs` - State with private fields + operations + event emission
- `queries.rs` - get_* (registry lookups) and fetch_* (platform calls)
- `mutations.rs` - set_*, perform_*, sync_*, on_* handlers
- `subscriptions.rs` - watch/unwatch

# Naming Convention

- `get_*` = registry/state lookup (fast, no OS calls)
- `fetch_*` = hits OS/platform (may be slow)
- `set_*` = value setting
- `perform_*` = actions
- `sync_*` = bulk updates from polling
- `on_*` = notification handlers

# Example

```ignore
let axio = Axio::new()?;

// Registry lookup
let windows = axio.get_windows();
let element = axio.get_element(element_id);

// Platform fetch
let element = axio.fetch_element_at(100.0, 200.0)?;
let children = axio.fetch_children(element.id, 100)?;

let mut events = axio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}
```
*/

mod mutations;
mod queries;
mod state;
mod subscriptions;
mod tree;

pub(crate) use state::{ElementData, ElementEntry, Registry};

use crate::platform::{CurrentPlatform, Platform};
use crate::polling::{self, PollingHandle};
use crate::types::{AxioError, AxioResult, Event};
use async_broadcast::{InactiveReceiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

pub(crate) use crate::polling::AxioOptions;

const EVENT_CHANNEL_CAPACITY: usize = 5000;

// ============================================================================
// Axio Struct Definition
// ============================================================================

/// Main axio instance - owns state, event broadcasting, and polling.
///
/// Polling starts automatically when created and stops when dropped.
/// Clone is cheap (Arc bumps) - share freely across threads.
pub struct Axio {
  pub(crate) state: Arc<RwLock<Registry>>,
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

// ============================================================================
// Construction & Events
// ============================================================================

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
    if !CurrentPlatform::has_permissions() {
      return Err(AxioError::PermissionDenied);
    }

    let (mut tx, rx) = async_broadcast::broadcast(EVENT_CHANNEL_CAPACITY);
    tx.set_overflow(true); // Drop oldest messages when full

    // State owns a clone of the sender for event emission
    let state = Registry::new(tx.clone());

    let axio = Axio {
      state: Arc::new(RwLock::new(state)),
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

  // ==========================================================================
  // State Access - NEVER do I/O inside these closures
  // ==========================================================================

  /// Read state. Lock released when closure returns.
  /// **Never call platform/OS functions inside the closure.**
  #[inline]
  pub(crate) fn read<R>(&self, f: impl FnOnce(&Registry) -> R) -> R {
    f(&self.state.read())
  }

  /// Write state. Lock released when closure returns.
  /// **Never call platform/OS functions inside the closure.**
  #[inline]
  pub(crate) fn write<R>(&self, f: impl FnOnce(&mut Registry) -> R) -> R {
    f(&mut self.state.write())
  }
}
