//! Unified registry for accessibility state.
//!
//! This module provides a single source of truth for all accessibility entities:
//! - Processes: One AXObserver per running application
//! - Windows: Tracked windows belonging to processes  
//! - Elements: UI elements with handles and subscriptions
//!
//! # Lifecycle
//!
//! ```text
//! Process (ProcessId / PID)
//! ├─ created: first window seen for this app
//! ├─ destroyed: no windows remain
//! └─ owns: ONE AXObserver for all notifications
//!
//! Window (WindowId)
//! ├─ created: window enumeration sees it
//! ├─ destroyed: window enumeration stops seeing it
//! ├─ belongs to: one Process
//! └─ owns: set of ElementIds
//!
//! Element (ElementId)
//! ├─ created: discovered via API (children, elementAt, focus)
//! ├─ destroyed: notification or window removal
//! ├─ belongs to: one Window
//! └─ owns: handle, subscriptions
//! ```
//!
//! # Cascade Behavior
//!
//! - Individual elements can be removed (e.g., destroyed notification)
//! - Window removal cascades to all elements in that window
//! - Process removal cascades to all windows → all elements

use crate::accessibility::Notification;
use crate::events;
use crate::platform::{self, ElementHandle, ObserverHandle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, Event, ProcessId, WindowId,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::LazyLock;

// =============================================================================
// Internal State Types
// =============================================================================

/// Per-process state: owns the AXObserver for this application.
struct ProcessState {
  /// The observer for this process (one per PID).
  /// All elements in all windows of this process share this observer.
  observer: ObserverHandle,
  /// Currently focused element in this app.
  focused_element: Option<ElementId>,
  /// Last selection state for deduplication (element_id, text).
  last_selection: Option<(ElementId, String)>,
}

/// Per-window state.
struct WindowState {
  process_id: ProcessId,
  info: AXWindow,
  /// Platform handle for window-level operations.
  handle: Option<ElementHandle>,
}

/// Per-element state: element data + platform handle + subscriptions.
struct ElementState {
  /// The element data (what we return to callers).
  element: AXElement,
  /// Platform handle for operations.
  handle: ElementHandle,
  /// CFHash of the element (for duplicate detection).
  hash: u64,
  /// Process ID (needed for observer operations).
  pid: u32,
  /// Platform role string (for notification decisions).
  platform_role: String,
  /// Active notification subscriptions.
  subscriptions: HashSet<Notification>,
  /// Context handle for destruction tracking (always set).
  destruction_context: Option<*mut c_void>,
  /// Context handle for watch notifications (when watched).
  watch_context: Option<*mut c_void>,
}

// SAFETY: Registry is protected by a Mutex, and raw pointers (context handles)
// are only accessed while holding the lock.
unsafe impl Send for ProcessState {}
unsafe impl Sync for ProcessState {}
unsafe impl Send for ElementState {}
unsafe impl Sync for ElementState {}

// =============================================================================
// Registry
// =============================================================================

/// Unified registry for all accessibility state.
pub struct Registry {
  /// Process state keyed by ProcessId.
  processes: HashMap<ProcessId, ProcessState>,
  /// Window state keyed by WindowId.
  windows: HashMap<WindowId, WindowState>,
  /// Element state keyed by ElementId.
  elements: HashMap<ElementId, ElementState>,

  // === Reverse Indexes ===
  /// WindowId → ProcessId (for cascade lookups).
  window_to_process: HashMap<WindowId, ProcessId>,
  /// ElementId → WindowId (for cascade lookups).
  element_to_window: HashMap<ElementId, WindowId>,
  /// CFHash → ElementId (for O(1) duplicate detection).
  hash_to_element: HashMap<u64, ElementId>,

  // === Dead Tracking ===
  /// Hashes of destroyed elements (prevents re-registration).
  /// Pruned when window is removed.
  dead_hashes: HashSet<u64>,
  /// Map from hash to window (for pruning dead_hashes on window removal).
  hash_to_window: HashMap<u64, WindowId>,

  // === Focus ===
  /// Currently active window (sticky - last valid focused window).
  active_window: Option<WindowId>,
  /// Currently focused window (can be None when desktop is focused).
  focused_window: Option<WindowId>,
  /// Window depth order (front to back, by z_index).
  depth_order: Vec<WindowId>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::new()));

impl Registry {
  fn new() -> Self {
    Self {
      processes: HashMap::new(),
      windows: HashMap::new(),
      elements: HashMap::new(),
      window_to_process: HashMap::new(),
      element_to_window: HashMap::new(),
      hash_to_element: HashMap::new(),
      dead_hashes: HashSet::new(),
      hash_to_window: HashMap::new(),
      active_window: None,
      focused_window: None,
      depth_order: Vec::new(),
    }
  }

  /// Run a function with mutable access to the registry.
  fn with<F, R>(f: F) -> R
  where
    F: FnOnce(&mut Registry) -> R,
  {
    let mut guard = REGISTRY.lock();
    f(&mut guard)
  }

  // ===========================================================================
  // Process Management
  // ===========================================================================

  /// Get or create process state for a PID.
  /// Creates the AXObserver if this is a new process.
  fn get_or_create_process(&mut self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    if self.processes.contains_key(&process_id) {
      return Ok(process_id);
    }

    // Create observer for this process
    let observer = platform::create_observer_for_pid(pid)?;

    // Subscribe to app-level notifications (focus, selection)
    if let Err(e) = platform::subscribe_app_notifications(pid, &observer) {
      log::warn!(
        "Failed to subscribe app notifications for PID {}: {:?}",
        pid,
        e
      );
    }

    self.processes.insert(
      process_id,
      ProcessState {
        observer,
        focused_element: None,
        last_selection: None,
      },
    );

    Ok(process_id)
  }

  /// Remove a process and cascade to all its windows.
  fn remove_process_internal(&mut self, process_id: &ProcessId) -> Vec<Event> {
    let mut events = Vec::new();

    // Find all windows for this process
    let window_ids: Vec<WindowId> = self
      .windows
      .iter()
      .filter(|(_, w)| w.process_id == *process_id)
      .map(|(id, _)| *id)
      .collect();

    // Cascade to windows (which cascade to elements)
    for window_id in window_ids {
      events.extend(self.remove_window_internal(&window_id));
    }

    // Remove process state
    self.processes.remove(process_id);

    events
  }

  // ===========================================================================
  // Window Management
  // ===========================================================================

  /// Update windows from polling. Returns events for changes.
  fn update_windows_internal(&mut self, new_windows: Vec<AXWindow>) -> Vec<Event> {
    let mut events = Vec::new();

    // Build set of new window IDs
    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    // Find removed windows
    let removed: Vec<WindowId> = self
      .windows
      .keys()
      .filter(|id| !new_ids.contains(id))
      .copied()
      .collect();

    // Remove them (cascades to elements)
    for window_id in removed {
      events.extend(self.remove_window_internal(&window_id));
    }

    // Process new/existing windows
    for window_info in new_windows {
      let window_id = window_info.id;
      let process_id = window_info.process_id;
      let pid = process_id.0;

      if let Some(existing) = self.windows.get_mut(&window_id) {
        // Check if changed (mark for later, need depth_order)
        if existing.info != window_info {
          // Will emit after depth_order is computed
        }
        existing.info = window_info;

        // Retry fetching handle if missing
        if existing.handle.is_none() {
          existing.handle = platform::fetch_window_handle(&existing.info);
        }
      } else {
        // New window - ensure process exists
        if let Err(e) = self.get_or_create_process(pid) {
          log::warn!("Failed to create process for window {}: {:?}", window_id, e);
          continue;
        }

        let handle = platform::fetch_window_handle(&window_info);

        self.windows.insert(
          window_id,
          WindowState {
            process_id,
            info: window_info.clone(),
            handle,
          },
        );
        self.window_to_process.insert(window_id, process_id);
        // Will emit WindowAdded after depth_order is computed
      }
    }

    // Update depth order
    let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
    windows.sort_by_key(|w| w.z_index);
    self.depth_order = windows.into_iter().map(|w| w.id).collect();

    // Note: For simplicity, we don't emit granular window events here.
    // The polling code will handle event emission with proper depth_order.
    // This internal method just updates state.

    events
  }

  /// Remove a window and cascade to all its elements.
  fn remove_window_internal(&mut self, window_id: &WindowId) -> Vec<Event> {
    let mut events = Vec::new();

    // Find all elements in this window
    let element_ids: Vec<ElementId> = self
      .elements
      .iter()
      .filter(|(_, e)| e.element.window_id == *window_id)
      .map(|(id, _)| *id)
      .collect();

    // Collect hashes to prune from dead_hashes
    let hashes_to_prune: Vec<u64> = element_ids
      .iter()
      .filter_map(|id| self.elements.get(id).map(|e| e.hash))
      .collect();

    // Remove each element
    for element_id in &element_ids {
      if let Some(event) = self.remove_element_internal(element_id) {
        events.push(event);
      }
    }

    // Prune dead_hashes for this window's elements
    for hash in hashes_to_prune {
      self.dead_hashes.remove(&hash);
      self.hash_to_window.remove(&hash);
    }

    // Remove window state
    if let Some(window_state) = self.windows.remove(window_id) {
      self.window_to_process.remove(window_id);

      // Update depth order after removal
      let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      self.depth_order = windows.into_iter().map(|w| w.id).collect();

      events.push(Event::WindowRemoved {
        window_id: *window_id,
        depth_order: self.depth_order.clone(),
      });

      // Check if process has no more windows
      let process_id = window_state.process_id;
      let has_windows = self.windows.values().any(|w| w.process_id == process_id);
      if !has_windows {
        self.processes.remove(&process_id);
      }
    }

    events
  }

  // ===========================================================================
  // Element Management
  // ===========================================================================

  /// Register a new element. Returns existing if hash matches.
  /// Emits ElementAdded for newly registered elements.
  /// Returns None if the element's hash is in the dead set.
  fn register_internal(
    &mut self,
    element: AXElement,
    handle: ElementHandle,
    pid: u32,
    platform_role: &str,
  ) -> Option<AXElement> {
    let window_id = element.window_id;
    let hash = platform::element_hash(&handle);

    // Reject if this element was previously destroyed
    if self.dead_hashes.contains(&hash) {
      return None;
    }

    // Check for existing element with same hash
    if let Some(existing_id) = self.hash_to_element.get(&hash) {
      if let Some(existing) = self.elements.get(existing_id) {
        return Some(existing.element.clone());
      }
    }

    // Ensure process exists (creates observer if needed)
    let process_id = ProcessId(pid);
    if self.get_or_create_process(pid).is_err() {
      log::warn!("Failed to create process for element registration");
      return None;
    }

    // Store element
    let element_id = element.id;
    let mut state = ElementState {
      element: element.clone(),
      handle,
      hash,
      pid,
      platform_role: platform_role.to_string(),
      subscriptions: HashSet::new(),
      destruction_context: None,
      watch_context: None,
    };

    // Register destruction tracking immediately
    if let Some(process) = self.processes.get(&process_id) {
      self.subscribe_destruction(&mut state, &process.observer);
    }

    // Update indexes
    self.elements.insert(element_id, state);
    self.element_to_window.insert(element_id, window_id);
    self.hash_to_element.insert(hash, element_id);
    self.hash_to_window.insert(hash, window_id);

    // Emit event for newly registered element
    events::emit(Event::ElementAdded {
      element: element.clone(),
    });

    Some(element)
  }

  /// Subscribe to destruction notification for an element.
  fn subscribe_destruction(&self, state: &mut ElementState, observer: &ObserverHandle) {
    if state.destruction_context.is_some() {
      return;
    }

    match platform::subscribe_destruction_notification(
      &state.element.id,
      &state.handle,
      observer.clone(),
    ) {
      Ok(context) => {
        state.destruction_context = Some(context as *mut c_void);
        state.subscriptions.insert(Notification::Destroyed);
      }
      Err(e) => {
        log::debug!(
          "Failed to register destruction for element {} (role: {}): {:?}",
          state.element.id,
          state.platform_role,
          e
        );
      }
    }
  }

  /// Remove an element.
  fn remove_element_internal(&mut self, element_id: &ElementId) -> Option<Event> {
    let Some(window_id) = self.element_to_window.remove(element_id) else {
      return None;
    };

    let Some(mut state) = self.elements.remove(element_id) else {
      return None;
    };

    // Add to dead set
    self.hash_to_element.remove(&state.hash);
    self.dead_hashes.insert(state.hash);
    self.hash_to_window.insert(state.hash, window_id);

    // Unsubscribe from notifications
    let process_id = ProcessId(state.pid);
    if let Some(process) = self.processes.get(&process_id) {
      self.unsubscribe_all(&mut state, &process.observer);
    }

    Some(Event::ElementRemoved {
      element_id: *element_id,
    })
  }

  /// Unsubscribe from all notifications for an element.
  fn unsubscribe_all(&self, state: &mut ElementState, observer: &ObserverHandle) {
    // Unsubscribe destruction tracking
    if let Some(context) = state.destruction_context.take() {
      platform::unsubscribe_destruction_notification(
        &state.handle,
        observer.clone(),
        context as *mut platform::ObserverContextHandle,
      );
    }

    // Unsubscribe watch notifications
    if let Some(context) = state.watch_context.take() {
      let notifs: Vec<_> = state
        .subscriptions
        .iter()
        .filter(|n| **n != Notification::Destroyed)
        .copied()
        .collect();

      platform::unsubscribe_notifications(
        &state.handle,
        observer.clone(),
        context as *mut platform::ObserverContextHandle,
        &notifs,
      );
    }

    state.subscriptions.clear();
  }

  // ===========================================================================
  // Watch/Unwatch
  // ===========================================================================

  /// Subscribe to additional notifications for an element (beyond destruction).
  fn watch_internal(&mut self, element_id: &ElementId) -> AxioResult<()> {
    let state = self
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    if state.watch_context.is_some() {
      return Ok(()); // Already watching
    }

    let process_id = ProcessId(state.pid);
    let observer = self
      .processes
      .get(&process_id)
      .map(|p| p.observer.clone())
      .ok_or(AxioError::NotSupported("Process not found".into()))?;

    // Get notifications for this element's role
    let notifs = Notification::for_watching(state.element.role);
    if notifs.is_empty() {
      return Ok(()); // Nothing to watch
    }

    // Subscribe
    let context = platform::subscribe_notifications(
      &state.element.id,
      &state.handle,
      observer,
      &state.platform_role,
      &notifs,
    )?;

    state.watch_context = Some(context as *mut c_void);
    for n in notifs {
      state.subscriptions.insert(n);
    }

    Ok(())
  }

  /// Unsubscribe from watch notifications (keeps destruction tracking).
  fn unwatch_internal(&mut self, element_id: &ElementId) {
    let Some(state) = self.elements.get_mut(element_id) else {
      return;
    };

    let Some(context) = state.watch_context.take() else {
      return;
    };

    let process_id = ProcessId(state.pid);
    let Some(process) = self.processes.get(&process_id) else {
      return;
    };

    let notifs: Vec<_> = state
      .subscriptions
      .iter()
      .filter(|n| **n != Notification::Destroyed)
      .copied()
      .collect();

    platform::unsubscribe_notifications(
      &state.handle,
      process.observer.clone(),
      context as *mut platform::ObserverContextHandle,
      &notifs,
    );

    // Keep only Destroyed subscription
    state
      .subscriptions
      .retain(|n| *n == Notification::Destroyed);
  }
}

// =============================================================================
// Public API
// =============================================================================

/// Update windows from polling.
pub fn update_windows(new_windows: Vec<AXWindow>) {
  let events = Registry::with(|r| r.update_windows_internal(new_windows));
  for event in events {
    events::emit(event);
  }
}

/// Get all windows.
pub fn get_windows() -> Vec<AXWindow> {
  Registry::with(|r| r.windows.values().map(|w| w.info.clone()).collect())
}

/// Get a specific window.
pub fn get_window(window_id: &WindowId) -> Option<AXWindow> {
  Registry::with(|r| r.windows.get(window_id).map(|w| w.info.clone()))
}

/// Get active window ID (sticky - last valid focused window).
pub fn get_active_window() -> Option<WindowId> {
  Registry::with(|r| r.active_window)
}

/// Set active window.
pub fn set_active_window(window_id: Option<WindowId>) {
  Registry::with(|r| r.active_window = window_id);
}

/// Set currently focused window.
pub fn set_focused_window(window_id: Option<WindowId>) {
  Registry::with(|r| r.focused_window = window_id);
}

/// Get the focused window for a specific PID.
/// Returns the focused window ID if it belongs to the given process.
pub fn get_focused_window_for_pid(pid: u32) -> Option<WindowId> {
  Registry::with(|r| {
    let window_id = r.focused_window?;
    let window_state = r.windows.get(&window_id)?;
    if window_state.process_id.0 == pid {
      Some(window_id)
    } else {
      None
    }
  })
}

/// Get window depth order (front to back).
pub fn get_depth_order() -> Vec<WindowId> {
  Registry::with(|r| r.depth_order.clone())
}

/// Find window at a point.
pub fn find_window_at_point(x: f64, y: f64) -> Option<AXWindow> {
  Registry::with(|r| {
    let point = crate::Point::new(x, y);
    let mut candidates: Vec<_> = r
      .windows
      .values()
      .filter(|w| w.info.bounds.contains(point))
      .collect();
    candidates.sort_by_key(|w| w.info.z_index);
    candidates.first().map(|w| w.info.clone())
  })
}

/// Get window info with handle.
pub fn get_window_with_handle(window_id: &WindowId) -> Option<(AXWindow, Option<ElementHandle>)> {
  Registry::with(|r| {
    r.windows
      .get(window_id)
      .map(|w| (w.info.clone(), w.handle.clone()))
  })
}

// === Focus API ===

/// Set focused element for a process, returns the previous focused element.
pub fn set_process_focus(pid: u32, element_id: ElementId) -> Option<ElementId> {
  Registry::with(|r| {
    let process_id = ProcessId(pid);
    if let Some(process) = r.processes.get_mut(&process_id) {
      let previous = process.focused_element;
      process.focused_element = Some(element_id);
      previous
    } else {
      None
    }
  })
}

/// Set selection for a process, returns true if selection changed (for deduplication).
pub fn set_process_selection(pid: u32, element_id: ElementId, text: &str) -> bool {
  Registry::with(|r| {
    let process_id = ProcessId(pid);
    if let Some(process) = r.processes.get_mut(&process_id) {
      let new_selection = (element_id, text.to_string());
      let changed = process.last_selection.as_ref() != Some(&new_selection);
      process.last_selection = Some(new_selection);
      changed
    } else {
      false
    }
  })
}

// === Element API ===

/// Register a new element.
/// Register an element. Returns existing if hash matches.
/// Emits ElementAdded for newly registered elements.
pub fn register_element(
  element: AXElement,
  handle: ElementHandle,
  pid: u32,
  platform_role: &str,
) -> Option<AXElement> {
  Registry::with(|r| r.register_internal(element, handle, pid, platform_role))
}

/// Get element by ID.
pub fn get_element(element_id: &ElementId) -> AxioResult<AXElement> {
  Registry::with(|r| {
    r.elements
      .get(element_id)
      .map(|e| e.element.clone())
      .ok_or(AxioError::ElementNotFound(*element_id))
  })
}

/// Get element by hash (for checking if element is already registered).
pub fn get_element_by_hash(hash: u64) -> Option<AXElement> {
  Registry::with(|r| {
    r.hash_to_element
      .get(&hash)
      .and_then(|id| r.elements.get(id))
      .map(|e| e.element.clone())
  })
}

/// Get multiple elements by ID.
pub fn get_elements(element_ids: &[ElementId]) -> Vec<AXElement> {
  Registry::with(|r| {
    element_ids
      .iter()
      .filter_map(|id| r.elements.get(id).map(|e| e.element.clone()))
      .collect()
  })
}

/// Get all elements.
pub fn get_all_elements() -> Vec<AXElement> {
  Registry::with(|r| r.elements.values().map(|e| e.element.clone()).collect())
}

/// Update element data.
pub fn update_element(element_id: &ElementId, updated: AXElement) -> AxioResult<()> {
  Registry::with(|r| {
    let state = r
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;
    state.element = updated;
    Ok(())
  })
}

/// Set children for an element.
pub fn set_element_children(element_id: &ElementId, children: Vec<ElementId>) -> AxioResult<()> {
  Registry::with(|r| {
    let state = r
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;
    state.element.children = Some(children);
    Ok(())
  })
}

/// Remove an element.
pub fn remove_element(element_id: &ElementId) {
  let event = Registry::with(|r| r.remove_element_internal(element_id));
  if let Some(event) = event {
    events::emit(event);
  }
}

/// Watch an element for notifications.
pub fn watch_element(element_id: &ElementId) -> AxioResult<()> {
  Registry::with(|r| r.watch_internal(element_id))
}

/// Stop watching an element.
pub fn unwatch_element(element_id: &ElementId) {
  Registry::with(|r| r.unwatch_internal(element_id))
}

/// Access stored element for operations (click, write).
pub fn with_element_handle<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
where
  F: FnOnce(&ElementHandle, &str) -> R,
{
  Registry::with(|r| {
    let state = r
      .elements
      .get(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;
    Ok(f(&state.handle, &state.platform_role))
  })
}

/// Info about a stored element needed for child discovery.
pub struct StoredElementInfo {
  pub handle: ElementHandle,
  pub window_id: WindowId,
  pub pid: u32,
  pub platform_role: String,
  pub parent_id: Option<ElementId>,
  pub children: Option<Vec<ElementId>>,
}

/// Get full stored element info for operations that need it.
pub fn get_stored_element_info(element_id: &ElementId) -> AxioResult<StoredElementInfo> {
  Registry::with(|r| {
    let state = r
      .elements
      .get(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;
    Ok(StoredElementInfo {
      handle: state.handle.clone(),
      window_id: state.element.window_id,
      pid: state.pid,
      platform_role: state.platform_role.clone(),
      parent_id: state.element.parent_id,
      children: state.element.children.clone(),
    })
  })
}

/// Write value to element.
pub fn write_element_value(element_id: &ElementId, text: &str) -> AxioResult<()> {
  with_element_handle(element_id, |handle, platform_role| {
    platform::write_element_value(handle, text, platform_role)
  })?
}

/// Click element.
pub fn click_element(element_id: &ElementId) -> AxioResult<()> {
  with_element_handle(element_id, |handle, _| platform::click_element(handle))?
}

// === Cleanup ===

/// Clean up observers for dead processes.
pub fn cleanup_dead_processes(active_pids: &HashSet<ProcessId>) -> usize {
  Registry::with(|r| {
    let dead: Vec<ProcessId> = r
      .processes
      .keys()
      .filter(|pid| !active_pids.contains(pid))
      .copied()
      .collect();

    let count = dead.len();
    for pid in dead {
      let events = r.remove_process_internal(&pid);
      for event in events {
        // Emit outside the lock would be better, but for now this is fine
        events::emit(event);
      }
    }
    count
  })
}
