/*!
State types for the Axio registry.
*/

use crate::platform::{Handle, Observer, WatchHandle};
use crate::types::{AXElement, AXWindow, ElementId, ProcessId, TextSelection, WindowId};
use std::collections::HashMap;

/// Per-process state: owns the `AXObserver` for this application.
pub(crate) struct ProcessState {
  /// The observer for this process (one per PID).
  pub(crate) observer: Observer,
  /// The app element handle (lives for process lifetime).
  pub(crate) app_handle: Handle,
  /// Currently focused element in this app.
  pub(crate) focused_element: Option<ElementId>,
  /// Last selection state for deduplication.
  pub(crate) last_selection: Option<TextSelection>,
}

/// Per-window state.
pub(crate) struct WindowState {
  pub(crate) process_id: ProcessId,
  pub(crate) info: AXWindow,
  /// Platform handle for window-level operations.
  pub(crate) handle: Option<Handle>,
}

/// Per-element state: element data + platform handle + watch.
pub(crate) struct ElementState {
  /// The element data (what we return to callers).
  pub(crate) element: AXElement,
  /// Platform handle for operations.
  pub(crate) handle: Handle,
  /// `CFHash` of the element (for duplicate detection).
  pub(crate) hash: u64,
  /// `CFHash` of this element's OS parent (for lazy linking).
  pub(crate) parent_hash: Option<u64>,
  /// Raw platform role string, e.g. `AXButton` (for role mapping).
  pub(crate) raw_role: String,
  /// Watch handle managing all notification subscriptions.
  /// Created at registration (with Destroyed), additional notifications added via `watch`.
  pub(crate) watch: Option<WatchHandle>,
}

impl ElementState {
  /// Create a new element state (no watch yet - added during registration).
  pub(crate) fn new(
    element: AXElement,
    handle: Handle,
    hash: u64,
    parent_hash: Option<u64>,
    raw_role: String,
  ) -> Self {
    Self {
      element,
      handle,
      hash,
      parent_hash,
      raw_role,
      watch: None,
    }
  }

  /// Get process ID from the element.
  pub(crate) fn pid(&self) -> u32 {
    self.element.pid.0
  }
}

/// Internal state storage.
pub(crate) struct State {
  /// Process state keyed by `ProcessId`.
  pub(crate) processes: HashMap<ProcessId, ProcessState>,
  /// Window state keyed by `WindowId`.
  pub(crate) windows: HashMap<WindowId, WindowState>,
  /// Element state keyed by `ElementId`.
  pub(crate) elements: HashMap<ElementId, ElementState>,

  // === Reverse Indexes ===
  /// `ElementId` → `WindowId` (for cascade lookups).
  pub(crate) element_to_window: HashMap<ElementId, WindowId>,
  /// `CFHash` → `ElementId` (for O(1) duplicate detection).
  pub(crate) hash_to_element: HashMap<u64, ElementId>,
  /// Parent hash → children waiting for that parent (lazy linking).
  pub(crate) waiting_for_parent: HashMap<u64, Vec<ElementId>>,

  // === Focus/Input ===
  /// Currently focused window (can be None when desktop is focused).
  pub(crate) focused_window: Option<WindowId>,
  /// Window depth order (front to back, by `z_index`).
  pub(crate) depth_order: Vec<WindowId>,
  /// Current mouse position.
  pub(crate) mouse_position: Option<crate::types::Point>,
}

impl State {
  pub(crate) fn new() -> Self {
    Self {
      processes: HashMap::new(),
      windows: HashMap::new(),
      elements: HashMap::new(),
      element_to_window: HashMap::new(),
      hash_to_element: HashMap::new(),
      waiting_for_parent: HashMap::new(),
      focused_window: None,
      depth_order: Vec::new(),
      mouse_position: None,
    }
  }
}
