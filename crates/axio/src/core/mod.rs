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
use axio::Freshness;

let axio = Axio::new()?;

// Get element with explicit freshness
let element = axio.get(element_id, Freshness::Cached)?;  // From cache
let element = axio.get(element_id, Freshness::Fresh)?;   // From OS

// Traversal with freshness
let children = axio.children(element.id, Freshness::Fresh)?;

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
pub(crate) use registry::{ElementData, ElementEntry, Registry};

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

// ============================================================================
// PlatformCallbacks Implementation
// ============================================================================

use crate::accessibility::Notification;
use crate::platform::{Handle, PlatformCallbacks};

impl PlatformCallbacks for Axio {
  type Handle = Handle;

  fn on_element_destroyed(&self, element_id: crate::types::ElementId) {
    // Delegate to existing handler
    self.handle_element_destroyed(element_id);
  }

  fn on_element_changed(&self, element_id: crate::types::ElementId, notification: Notification) {
    // Delegate to existing handler
    self.handle_element_changed(element_id, notification);
  }

  fn on_children_changed(&self, element_id: crate::types::ElementId) {
    // Re-fetch children
    drop(self.fetch_children(element_id, 1000));
  }

  fn on_focus_changed(&self, pid: u32, focused_handle: Handle) {
    use crate::platform::PlatformHandle;
    use crate::types::ProcessId;

    // Find window for this element: check if element exists, else use focused window
    let window_id = self.read(|r| {
      let hash = focused_handle.element_hash();
      r.find_by_hash(hash, Some(ProcessId(pid)))
        .and_then(|id| r.element(id))
        .map(|e| e.data.window_id)
        .or_else(|| r.focused_window_for_pid(pid))
    });

    let Some(window_id) = window_id else {
      log::debug!("FocusChanged: no window_id found for PID {pid}, skipping");
      return;
    };

    // Cache element from handle
    let element_id = self.cache_from_handle(focused_handle, window_id, ProcessId(pid));

    // Build element
    let Some(element) = self.read(|r| build_element(r, element_id)) else {
      log::warn!("FocusChanged: element build failed for PID {pid}");
      return;
    };

    // Only process focus for elements that self-identify as focused
    if element.focused != Some(true) {
      return;
    }

    // Delegate to existing handler (includes auto-watch logic)
    self.handle_focus_changed(pid, element);
  }

  fn on_selection_changed(
    &self,
    pid: u32,
    element_handle: Handle,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    use crate::platform::PlatformHandle;
    use crate::types::ProcessId;

    // Find window for this element: check if element exists, else use focused window
    let window_id = self.read(|r| {
      let hash = element_handle.element_hash();
      r.find_by_hash(hash, Some(ProcessId(pid)))
        .and_then(|id| r.element(id))
        .map(|e| e.data.window_id)
        .or_else(|| r.focused_window_for_pid(pid))
    });

    let Some(window_id) = window_id else {
      log::debug!("SelectionChanged: no window_id found for PID {pid}, skipping");
      return;
    };

    // Cache element from handle
    let element_id = self.cache_from_handle(element_handle, window_id, ProcessId(pid));

    // Delegate to existing handler
    self.handle_selection_changed(pid, window_id, element_id, text, range);
  }
}
