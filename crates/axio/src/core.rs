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

#![allow(unsafe_code)]

use crate::accessibility::Notification;
use crate::platform::{self, ElementHandle, ObserverHandle};
use crate::polling::{self, AxioOptions, PollingHandle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, Event, ProcessId, TextSelection, WindowId,
};
use async_broadcast::{InactiveReceiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::Arc;

const EVENT_CHANNEL_CAPACITY: usize = 5000;

/// Shared inner state that polling and callbacks access.
struct AxioInner {
  state: Arc<RwLock<State>>,
  events_tx: Sender<Event>,
  events_keepalive: InactiveReceiver<Event>,
  /// Polling handle - set after construction, wrapped in Mutex for interior mutability.
  polling: Mutex<Option<PollingHandle>>,
}

/// Main axio instance - owns state, event broadcasting, and polling.
///
/// Polling starts automatically when created and stops when dropped.
/// Clone is cheap (Arc bump) - share freely across threads.
pub struct Axio {
  inner: Arc<AxioInner>,
}

impl Clone for Axio {
  fn clone(&self) -> Self {
    Self {
      inner: Arc::clone(&self.inner),
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

    let inner = Arc::new(AxioInner {
      state: Arc::new(RwLock::new(State::new())),
      events_tx: tx,
      events_keepalive: rx.deactivate(),
      polling: Mutex::new(None),
    });

    let axio = Axio { inner };

    // Start polling with a clone (shares the same inner via Arc)
    let polling_handle = polling::start_polling(axio.clone(), options);
    *axio.inner.polling.lock() = Some(polling_handle);

    Ok(axio)
  }

  /// Subscribe to events from this instance.
  pub fn subscribe(&self) -> async_broadcast::Receiver<Event> {
    self.inner.events_keepalive.activate_cloned()
  }

  /// Emit an event to all subscribers.
  pub(crate) fn emit(&self, event: Event) {
    drop(self.inner.events_tx.try_broadcast(event));
  }

  /// Emit multiple events.
  pub(crate) fn emit_all(&self, events: impl IntoIterator<Item = Event>) {
    for event in events {
      self.emit(event);
    }
  }
}

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

// === State Query Methods ===

impl Axio {
  /// Get all windows.
  pub fn get_windows(&self) -> Vec<AXWindow> {
    self
      .inner
      .state
      .read()
      .windows
      .values()
      .map(|w| w.info.clone())
      .collect()
  }

  /// Get a specific window.
  pub fn get_window(&self, window_id: WindowId) -> Option<AXWindow> {
    self
      .inner
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| w.info.clone())
  }

  /// Get the focused window ID.
  pub fn get_focused_window(&self) -> Option<WindowId> {
    self.inner.state.read().focused_window
  }

  /// Get window depth order (front to back).
  pub fn get_depth_order(&self) -> Vec<WindowId> {
    self.inner.state.read().depth_order.clone()
  }

  /// Get element by ID.
  pub fn get_element(&self, element_id: ElementId) -> Option<AXElement> {
    self
      .inner
      .state
      .read()
      .elements
      .get(&element_id)
      .map(|e| e.element.clone())
  }

  /// Get element by hash (for checking if element is already registered).
  pub(crate) fn get_element_by_hash(&self, hash: u64) -> Option<AXElement> {
    let state = self.inner.state.read();
    state
      .hash_to_element
      .get(&hash)
      .and_then(|id| state.elements.get(id))
      .map(|e| e.element.clone())
  }

  /// Get multiple elements by ID.
  pub fn get_elements(&self, element_ids: &[ElementId]) -> Vec<AXElement> {
    let state = self.inner.state.read();
    element_ids
      .iter()
      .filter_map(|id| state.elements.get(id).map(|e| e.element.clone()))
      .collect()
  }

  /// Get all elements.
  pub fn get_all_elements(&self) -> Vec<AXElement> {
    self
      .inner
      .state
      .read()
      .elements
      .values()
      .map(|e| e.element.clone())
      .collect()
  }

  /// Get a snapshot of the current state for sync.
  pub fn snapshot(&self) -> crate::types::Snapshot {
    let state = self.inner.state.read();
    let (focused_element, selection) = state
      .focused_window
      .and_then(|window_id| {
        let window = state.windows.get(&window_id)?;
        let process = state.processes.get(&window.process_id)?;

        let focused_elem = process
          .focused_element
          .and_then(|id| state.elements.get(&id).map(|s| s.element.clone()));

        Some((focused_elem, process.last_selection.clone()))
      })
      .unwrap_or((None, None));

    crate::types::Snapshot {
      windows: state.windows.values().map(|w| w.info.clone()).collect(),
      elements: state.elements.values().map(|s| s.element.clone()).collect(),
      focused_window: state.focused_window,
      focused_element,
      selection,
      depth_order: state.depth_order.clone(),
      mouse_position: state.mouse_position,
    }
  }

  /// Find window at a point.
  pub(crate) fn find_window_at_point(&self, x: f64, y: f64) -> Option<AXWindow> {
    let state = self.inner.state.read();
    let point = crate::Point::new(x, y);
    let mut candidates: Vec<_> = state
      .windows
      .values()
      .filter(|w| w.info.bounds.contains(point))
      .collect();
    candidates.sort_by_key(|w| w.info.z_index);
    candidates.first().map(|w| w.info.clone())
  }

  /// Get window info with handle.
  pub(crate) fn get_window_with_handle(
    &self,
    window_id: WindowId,
  ) -> Option<(AXWindow, Option<ElementHandle>)> {
    self
      .inner
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| (w.info.clone(), w.handle.clone()))
  }

  /// Get the focused window for a specific PID.
  pub(crate) fn get_focused_window_for_pid(&self, pid: u32) -> Option<WindowId> {
    let state = self.inner.state.read();
    let window_id = state.focused_window?;
    let window_state = state.windows.get(&window_id)?;
    if window_state.process_id.0 == pid {
      Some(window_id)
    } else {
      None
    }
  }

  /// Get stored element info for operations that need it.
  pub(crate) fn get_stored_element_info(
    &self,
    element_id: ElementId,
  ) -> AxioResult<StoredElementInfo> {
    let state = self.inner.state.read();
    let elem_state = state
      .elements
      .get(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;
    Ok(StoredElementInfo {
      handle: elem_state.handle.clone(),
      window_id: elem_state.element.window_id,
      pid: elem_state.pid,
      platform_role: elem_state.platform_role.clone(),
      is_root: elem_state.element.is_root,
      parent_id: elem_state.element.parent_id,
      children: elem_state.element.children.clone(),
    })
  }

  /// Access stored element handle for operations (click, write).
  pub(crate) fn with_element_handle<F, R>(&self, element_id: ElementId, f: F) -> AxioResult<R>
  where
    F: FnOnce(&ElementHandle, &str) -> R,
  {
    let state = self.inner.state.read();
    let elem_state = state
      .elements
      .get(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;
    Ok(f(&elem_state.handle, &elem_state.platform_role))
  }
}

// === State Mutation Methods ===

impl Axio {
  /// Get or create process state for a PID.
  /// Creates the `AXObserver` if this is a new process.
  pub(crate) fn get_or_create_process(&self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    // Fast path: check if already exists
    if self.inner.state.read().processes.contains_key(&process_id) {
      return Ok(process_id);
    }

    // Slow path: create observer and insert
    let observer = platform::create_observer_for_pid(pid, self.clone())?;

    // Subscribe to app-level notifications (focus, selection)
    if let Err(e) = platform::subscribe_app_notifications(pid, &observer, self.clone()) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
    }

    self.inner.state.write().processes.insert(
      process_id,
      ProcessState {
        observer,
        focused_element: None,
        last_selection: None,
      },
    );

    Ok(process_id)
  }

  /// Update windows from polling. Returns PIDs of newly added windows.
  pub(crate) fn update_windows(&self, new_windows: Vec<AXWindow>) -> Vec<ProcessId> {
    let mut events = Vec::new();
    let mut added_window_ids = Vec::new();
    let mut new_process_ids = Vec::new();
    let mut changed_window_ids = Vec::new();

    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    {
      let mut state = self.inner.state.write();

      // Find removed windows
      let removed: Vec<WindowId> = state
        .windows
        .keys()
        .filter(|id| !new_ids.contains(id))
        .copied()
        .collect();

      for window_id in removed {
        events.extend(Self::remove_window_internal(&mut state, window_id));
      }

      // Process new/existing windows
      for window_info in new_windows {
        let window_id = window_info.id;
        let process_id = window_info.process_id;

        if let Some(existing) = state.windows.get_mut(&window_id) {
          if existing.info != window_info {
            changed_window_ids.push(window_id);
          }
          existing.info = window_info;

          if existing.handle.is_none() {
            existing.handle = platform::fetch_window_handle(&existing.info);
          }
        } else {
          // New window
          let handle = platform::fetch_window_handle(&window_info);

          state.windows.insert(
            window_id,
            WindowState {
              process_id,
              info: window_info.clone(),
              handle,
            },
          );
          added_window_ids.push(window_id);
          new_process_ids.push(process_id);
        }
      }

      // Update depth order
      let mut windows: Vec<_> = state.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      state.depth_order = windows.into_iter().map(|w| w.id).collect();

      // Generate events for added windows
      for window_id in &added_window_ids {
        if let Some(window) = state.windows.get(window_id) {
          events.push(Event::WindowAdded {
            window: window.info.clone(),
          });
        }
      }

      // Generate events for changed windows
      for window_id in changed_window_ids {
        if let Some(window) = state.windows.get(&window_id) {
          events.push(Event::WindowChanged {
            window: window.info.clone(),
          });
        }
      }
    }

    // Ensure processes exist for new windows (outside the state lock)
    for process_id in &new_process_ids {
      if let Err(e) = self.get_or_create_process(process_id.0) {
        log::warn!("Failed to create process for window: {e:?}");
      }
    }

    // Emit events
    self.emit_all(events);

    new_process_ids
  }

  /// Set currently focused window. Emits `FocusWindow` if value changed.
  pub(crate) fn set_focused_window(&self, window_id: Option<WindowId>) {
    let changed = {
      let mut state = self.inner.state.write();
      if state.focused_window == window_id {
        false
      } else {
        state.focused_window = window_id;
        true
      }
    };
    if changed {
      self.emit(Event::FocusWindow { window_id });
    }
  }

  /// Update mouse position and emit event if changed.
  pub(crate) fn update_mouse_position(&self, pos: crate::types::Point) {
    let changed = {
      let mut state = self.inner.state.write();
      let changed = state
        .mouse_position
        .is_none_or(|last| pos.moved_from(last, 1.0));
      if changed {
        state.mouse_position = Some(pos);
      }
      changed
    };
    if changed {
      self.emit(Event::MousePosition(pos));
    }
  }

  /// Register an element. Returns existing if hash matches.
  pub(crate) fn register_element(&self, data: ElementData) -> Option<AXElement> {
    let mut events = Vec::new();
    let result = {
      let mut state = self.inner.state.write();
      Self::register_internal(&mut state, data, self, &mut events)
    };
    self.emit_all(events);
    result
  }

  /// Update element data. Emits `ElementChanged` if actually changed.
  pub(crate) fn update_element(&self, element_id: ElementId, updated: AXElement) -> AxioResult<()> {
    let maybe_event = {
      let mut state = self.inner.state.write();
      let elem_state = state
        .elements
        .get_mut(&element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;

      if elem_state.element == updated {
        None
      } else {
        elem_state.element = updated.clone();
        Some(Event::ElementChanged { element: updated })
      }
    };

    if let Some(event) = maybe_event {
      self.emit(event);
    }
    Ok(())
  }

  /// Set children for an element. Emits `ElementChanged` if children changed.
  pub(crate) fn set_element_children(
    &self,
    element_id: ElementId,
    children: Vec<ElementId>,
  ) -> AxioResult<()> {
    let maybe_event = {
      let mut state = self.inner.state.write();
      let elem_state = state
        .elements
        .get_mut(&element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;

      let new_children = Some(children);
      if elem_state.element.children == new_children {
        None
      } else {
        elem_state.element.children = new_children;
        Some(Event::ElementChanged {
          element: elem_state.element.clone(),
        })
      }
    };

    if let Some(event) = maybe_event {
      self.emit(event);
    }
    Ok(())
  }

  /// Remove an element (cascades to children).
  pub(crate) fn remove_element(&self, element_id: ElementId) {
    let events = {
      let mut state = self.inner.state.write();
      Self::remove_element_internal(&mut state, element_id)
    };
    self.emit_all(events);
  }

  /// Update focused element for a process. Emits `FocusElement` event.
  pub(crate) fn update_focus(&self, pid: u32, element: AXElement) -> Option<ElementId> {
    let (previous_id, should_emit) = {
      let mut state = self.inner.state.write();
      let process_id = ProcessId(pid);
      let process = state.processes.get_mut(&process_id)?;

      let previous = process.focused_element;
      let same_element = previous == Some(element.id);

      if same_element {
        return previous;
      }

      process.focused_element = Some(element.id);
      (previous, true)
    };

    if !should_emit {
      return previous_id;
    }

    // Auto-unwatch previous element
    if let Some(prev_id) = previous_id {
      if let Some(prev_elem) = self.get_element(prev_id) {
        if prev_elem.role.auto_watch_on_focus() || prev_elem.role.is_writable() {
          drop(self.unwatch_element(prev_id));
        }
      }
    }

    // Auto-watch new element
    if element.role.auto_watch_on_focus() || element.role.is_writable() {
      drop(self.watch_element(element.id));
    }

    // Emit focus event
    self.emit(Event::FocusElement {
      element,
      previous_element_id: previous_id,
    });

    previous_id
  }

  /// Update selection for a process. Emits `SelectionChanged` if changed.
  pub(crate) fn update_selection(
    &self,
    pid: u32,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    let new_selection = TextSelection {
      element_id,
      text,
      range,
    };

    let should_emit = {
      let mut state = self.inner.state.write();
      let process_id = ProcessId(pid);
      let Some(process) = state.processes.get_mut(&process_id) else {
        return;
      };

      let changed = process.last_selection.as_ref() != Some(&new_selection);
      process.last_selection = Some(new_selection.clone());
      changed
    };

    if should_emit {
      self.emit(Event::SelectionChanged {
        window_id,
        element_id: new_selection.element_id,
        text: new_selection.text,
        range: new_selection.range,
      });
    }
  }

  /// Watch an element for notifications.
  pub(crate) fn watch_element(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.inner.state.write();
    Self::watch_internal(&mut state, &element_id, self.clone())
  }

  /// Stop watching an element.
  pub(crate) fn unwatch_element(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.inner.state.write();
    Self::unwatch_internal(&mut state, &element_id)
  }

  /// Write typed value to element.
  pub(crate) fn write_element_value(
    &self,
    element_id: ElementId,
    value: &crate::accessibility::Value,
  ) -> AxioResult<()> {
    self.with_element_handle(element_id, |handle, platform_role| {
      platform::write_element_value(handle, value, platform_role)
    })?
  }

  /// Click element.
  pub(crate) fn click_element(&self, element_id: ElementId) -> AxioResult<()> {
    self.with_element_handle(element_id, |handle, _| platform::click_element(handle))?
  }
}

// === Internal State Operations (take &mut State) ===

impl Axio {
  /// Remove a window and cascade to all its elements.
  fn remove_window_internal(state: &mut State, window_id: WindowId) -> Vec<Event> {
    let mut events = Vec::new();

    let element_ids: Vec<ElementId> = state
      .elements
      .iter()
      .filter(|(_, e)| e.element.window_id == window_id)
      .map(|(id, _)| *id)
      .collect();

    for element_id in element_ids {
      events.extend(Self::remove_element_internal(state, element_id));
    }

    if let Some(window_state) = state.windows.remove(&window_id) {
      let mut windows: Vec<_> = state.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      state.depth_order = windows.into_iter().map(|w| w.id).collect();

      events.push(Event::WindowRemoved { window_id });

      let process_id = window_state.process_id;
      let has_windows = state.windows.values().any(|w| w.process_id == process_id);
      if !has_windows {
        state.processes.remove(&process_id);
      }
    }

    events
  }

  /// Register a new element. Returns existing if hash matches.
  fn register_internal(
    state: &mut State,
    data: ElementData,
    axio: &Axio,
    events: &mut Vec<Event>,
  ) -> Option<AXElement> {
    let ElementData {
      mut element,
      handle,
      hash,
      parent_hash,
      raw_role,
    } = data;

    let window_id = element.window_id;

    // Check for existing element with same hash
    if let Some(existing_id) = state.hash_to_element.get(&hash) {
      if let Some(existing) = state.elements.get(existing_id) {
        return Some(existing.element.clone());
      }
    }

    // Try to link orphan to parent if parent exists in registry
    if !element.is_root && element.parent_id.is_none() {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = state.hash_to_element.get(ph) {
          element.parent_id = Some(parent_id);
        }
      }
    }

    let element_id = element.id;
    let pid = element.pid.0;
    let process_id = element.pid;

    let mut elem_state = ElementState {
      element: element.clone(),
      handle,
      hash,
      parent_hash,
      pid,
      platform_role: raw_role,
      subscriptions: HashSet::new(),
      destruction_context: None,
      watch_context: None,
    };

    // Subscribe to destruction notification
    if let Some(process) = state.processes.get(&process_id) {
      Self::subscribe_destruction(&mut elem_state, &process.observer, axio);
    }

    state.elements.insert(element_id, elem_state);
    state.element_to_window.insert(element_id, window_id);
    state.hash_to_element.insert(hash, element_id);

    // Link to parent
    if let Some(parent_id) = element.parent_id {
      Self::add_child_to_parent(state, parent_id, element_id, events);
    } else if !element.is_root {
      // Orphan: has parent in OS but not loaded yet
      if let Some(ref ph) = parent_hash {
        state
          .waiting_for_parent
          .entry(*ph)
          .or_default()
          .push(element_id);
      }
    }

    // Link waiting orphans to this element
    if let Some(orphans) = state.waiting_for_parent.remove(&hash) {
      for orphan_id in orphans {
        Self::link_orphan_to_parent(state, orphan_id, element_id, events);
      }
    }

    events.push(Event::ElementAdded {
      element: element.clone(),
    });

    Some(element)
  }

  /// Link an orphan element to its newly-discovered parent.
  fn link_orphan_to_parent(
    state: &mut State,
    orphan_id: ElementId,
    parent_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(orphan_state) = state.elements.get_mut(&orphan_id) {
      orphan_state.element.parent_id = Some(parent_id);
      events.push(Event::ElementChanged {
        element: orphan_state.element.clone(),
      });
    }
    Self::add_child_to_parent(state, parent_id, orphan_id, events);
  }

  /// Add a child to a parent's children list.
  fn add_child_to_parent(
    state: &mut State,
    parent_id: ElementId,
    child_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(parent_state) = state.elements.get_mut(&parent_id) {
      let children = parent_state.element.children.get_or_insert_with(Vec::new);
      if !children.contains(&child_id) {
        children.push(child_id);
        events.push(Event::ElementChanged {
          element: parent_state.element.clone(),
        });
      }
    }
  }

  /// Subscribe to destruction notification for an element.
  fn subscribe_destruction(elem_state: &mut ElementState, observer: &ObserverHandle, axio: &Axio) {
    if elem_state.destruction_context.is_some() {
      return;
    }

    match platform::subscribe_destruction_notification(
      &elem_state.element.id,
      &elem_state.handle,
      observer,
      axio.clone(),
    ) {
      Ok(context) => {
        elem_state.destruction_context = Some(context.cast::<c_void>());
        elem_state.subscriptions.insert(Notification::Destroyed);
      }
      Err(e) => {
        log::debug!(
          "Failed to register destruction for element {} (role: {}): {:?}",
          elem_state.element.id,
          elem_state.platform_role,
          e
        );
      }
    }
  }

  /// Remove an element.
  fn remove_element_internal(state: &mut State, element_id: ElementId) -> Vec<Event> {
    let mut events = Vec::new();

    let Some(_window_id) = state.element_to_window.remove(&element_id) else {
      return events;
    };

    let Some(mut elem_state) = state.elements.remove(&element_id) else {
      return events;
    };

    // Remove from parent's children
    if let Some(parent_id) = elem_state.element.parent_id {
      Self::remove_child_from_parent(state, parent_id, element_id, &mut events);
    }

    // Remove from waiting_for_parent
    if let Some(ref ph) = elem_state.parent_hash {
      if let Some(waiting) = state.waiting_for_parent.get_mut(ph) {
        waiting.retain(|&id| id != element_id);
        if waiting.is_empty() {
          state.waiting_for_parent.remove(ph);
        }
      }
    }

    state.waiting_for_parent.remove(&elem_state.hash);

    // Recursively remove children
    if let Some(children) = &elem_state.element.children {
      for child_id in children.clone() {
        events.extend(Self::remove_element_internal(state, child_id));
      }
    }

    state.hash_to_element.remove(&elem_state.hash);

    // Unsubscribe from notifications
    let process_id = ProcessId(elem_state.pid);
    if let Some(process) = state.processes.get(&process_id) {
      Self::unsubscribe_all(&mut elem_state, &process.observer);
    }

    events.push(Event::ElementRemoved { element_id });

    events
  }

  /// Remove a child from a parent's children list.
  fn remove_child_from_parent(
    state: &mut State,
    parent_id: ElementId,
    child_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(parent_state) = state.elements.get_mut(&parent_id) {
      if let Some(children) = &mut parent_state.element.children {
        let old_len = children.len();
        children.retain(|&id| id != child_id);
        if children.len() != old_len {
          events.push(Event::ElementChanged {
            element: parent_state.element.clone(),
          });
        }
      }
    }
  }

  /// Unsubscribe from all notifications for an element.
  fn unsubscribe_all(elem_state: &mut ElementState, observer: &ObserverHandle) {
    // Unsubscribe destruction tracking
    if let Some(context) = elem_state.destruction_context.take() {
      platform::unsubscribe_destruction_notification(
        &elem_state.handle,
        observer,
        context.cast::<platform::ObserverContextHandle>(),
      );
    }

    // Unsubscribe watch notifications
    if let Some(context) = elem_state.watch_context.take() {
      let notifs: Vec<_> = elem_state
        .subscriptions
        .iter()
        .filter(|n| **n != Notification::Destroyed)
        .copied()
        .collect();

      platform::unsubscribe_notifications(
        &elem_state.handle,
        observer,
        context.cast::<platform::ObserverContextHandle>(),
        &notifs,
      );
    }

    elem_state.subscriptions.clear();
  }

  /// Subscribe to watch notifications for an element.
  fn watch_internal(state: &mut State, element_id: &ElementId, axio: Axio) -> AxioResult<()> {
    let elem_state = state
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    if elem_state.watch_context.is_some() {
      return Ok(()); // Already watching
    }

    let process_id = ProcessId(elem_state.pid);
    let observer = state
      .processes
      .get(&process_id)
      .map(|p| &p.observer)
      .ok_or(AxioError::NotSupported("Process not found".into()))?;

    let notifs = Notification::for_watching(elem_state.element.role);
    if notifs.is_empty() {
      return Ok(()); // Nothing to watch
    }

    let context = platform::subscribe_notifications(
      &elem_state.element.id,
      &elem_state.handle,
      observer,
      &elem_state.platform_role,
      &notifs,
      axio,
    )?;

    elem_state.watch_context = Some(context.cast::<c_void>());
    for n in notifs {
      elem_state.subscriptions.insert(n);
    }

    Ok(())
  }

  /// Unsubscribe from watch notifications.
  fn unwatch_internal(state: &mut State, element_id: &ElementId) -> AxioResult<()> {
    let elem_state = state
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    let Some(context) = elem_state.watch_context.take() else {
      return Ok(());
    };

    let process_id = ProcessId(elem_state.pid);
    let process = state
      .processes
      .get(&process_id)
      .ok_or_else(|| AxioError::Internal("Process not found during unwatch".into()))?;

    let notifs: Vec<_> = elem_state
      .subscriptions
      .iter()
      .filter(|n| **n != Notification::Destroyed)
      .copied()
      .collect();

    platform::unsubscribe_notifications(
      &elem_state.handle,
      &process.observer,
      context.cast::<platform::ObserverContextHandle>(),
      &notifs,
    );

    elem_state
      .subscriptions
      .retain(|n| *n == Notification::Destroyed);

    Ok(())
  }
}
