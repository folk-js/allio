/*!
Core Axio instance - owns all accessibility state and event broadcasting.

# Module Structure

- `mod.rs` - Axio struct, construction, events, PlatformCallbacks
- `registry/` - Registry (cache) with private fields + operations + event emission
- `queries.rs` - get() with freshness, lookups, discovery
- `mutations.rs` - set_*, perform_*, sync_*, notification handlers
- `subscriptions.rs` - watch/unwatch

# Example

```ignore
use axio::Recency;

let axio = Axio::new()?;

// Get element with explicit freshness
let element = axio.get(element_id, Recency::Any)?;  // From cache
let element = axio.get(element_id, Recency::Current)?;   // From OS

// Traversal with freshness
let children = axio.children(element.id, Recency::Current)?;

let mut events = axio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}
```
*/

mod builders;
mod mutations;
mod queries;
mod registry;
mod subscriptions;

pub(crate) use builders::{build_element, build_snapshot};
pub(crate) use registry::{ElementData, Registry};

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
  screen_size: Arc<std::sync::OnceLock<(f64, f64)>>,
}

impl Clone for Axio {
  fn clone(&self) -> Self {
    Self {
      state: Arc::clone(&self.state),
      events_tx: self.events_tx.clone(),
      events_keepalive: self.events_keepalive.clone(),
      polling: Arc::clone(&self.polling),
      screen_size: Arc::clone(&self.screen_size),
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
      screen_size: Arc::new(std::sync::OnceLock::new()),
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

// ============================================================================
// PlatformCallbacks Implementation
// ============================================================================

use crate::platform::{ElementEvent, Handle, PlatformCallbacks, PlatformHandle};

impl PlatformCallbacks for Axio {
  type Handle = Handle;

  fn on_element_event(&self, event: ElementEvent<Handle>) {
    use crate::types::ProcessId;

    match event {
      ElementEvent::Destroyed(element_id) => {
        self.handle_element_destroyed(element_id);
      }

      ElementEvent::Changed(element_id, notification) => {
        self.handle_element_changed(element_id, notification);
      }

      ElementEvent::ChildrenChanged(element_id) => {
        // Re-fetch children
        drop(self.fetch_children(element_id, 1000));
      }

      ElementEvent::FocusChanged(focused_handle) => {
        let pid = ProcessId(focused_handle.pid());

        // Find window for this element
        let Some(window_id) = self.window_for_handle(&focused_handle) else {
          log::debug!("FocusChanged: no window_id found for PID {pid:?}, skipping");
          return;
        };

        // Cache element from handle and delegate to handler
        let element_id = self.cache_from_handle(focused_handle, window_id, pid);
        self.handle_focus_changed(pid.0, element_id);
      }

      ElementEvent::SelectionChanged {
        handle,
        text,
        range,
      } => {
        let pid = ProcessId(handle.pid());

        // Find window for this element
        let Some(window_id) = self.window_for_handle(&handle) else {
          log::debug!("SelectionChanged: no window_id found for PID {pid:?}, skipping");
          return;
        };

        // Cache element from handle
        let element_id = self.cache_from_handle(handle, window_id, pid);

        // Delegate to existing handler
        self.handle_selection_changed(pid.0, window_id, element_id, text, range);
      }
    }
  }
}
