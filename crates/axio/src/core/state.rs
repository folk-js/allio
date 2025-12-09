/*!
State types for the Axio registry.
*/

#![allow(unsafe_code)]

use crate::accessibility::Notification;
use crate::platform::{ElementHandle, ObserverHandle};
use crate::types::{AXElement, AXWindow, ElementId, ProcessId, TextSelection, WindowId};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

/// Per-process state: owns the `AXObserver` for this application.
pub(crate) struct ProcessState {
  /// The observer for this process (one per PID).
  pub(crate) observer: ObserverHandle,
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
  pub(crate) handle: Option<ElementHandle>,
}

/// Per-element state: element data + platform handle + subscriptions.
pub(crate) struct ElementState {
  /// The element data (what we return to callers).
  pub(crate) element: AXElement,
  /// Platform handle for operations.
  pub(crate) handle: ElementHandle,
  /// `CFHash` of the element (for duplicate detection).
  pub(crate) hash: u64,
  /// `CFHash` of this element's OS parent (for lazy linking).
  pub(crate) parent_hash: Option<u64>,
  /// Process ID (needed for observer operations).
  pub(crate) pid: u32,
  /// Platform role string (for notification decisions).
  pub(crate) platform_role: String,
  /// Active notification subscriptions.
  pub(crate) subscriptions: HashSet<Notification>,
  /// Context handle for destruction tracking (always set).
  pub(crate) destruction_context: Option<*mut c_void>,
  /// Context handle for watch notifications (when watched).
  pub(crate) watch_context: Option<*mut c_void>,
}

// SAFETY: State is protected by RwLock, and raw pointers (context handles)
// are only accessed while holding the lock.
unsafe impl Send for ProcessState {}
unsafe impl Sync for ProcessState {}
unsafe impl Send for ElementState {}
unsafe impl Sync for ElementState {}

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

/// Data returned by platform element building (before registration).
pub(crate) struct ElementData {
  pub(crate) element: AXElement,
  pub(crate) handle: ElementHandle,
  pub(crate) hash: u64,
  pub(crate) parent_hash: Option<u64>,
  pub(crate) raw_role: String,
}

/// Info about a stored element needed for child discovery and refresh.
pub(crate) struct StoredElementInfo {
  pub(crate) handle: ElementHandle,
  pub(crate) window_id: WindowId,
  pub(crate) pid: u32,
  pub(crate) platform_role: String,
  pub(crate) is_root: bool,
  pub(crate) parent_id: Option<ElementId>,
  pub(crate) children: Option<Vec<ElementId>>,
}
