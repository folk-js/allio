/*!
Unified registry for accessibility state.

Single source of truth for all accessibility entities:
- Processes: One `AXObserver` per running application
- Windows: Tracked windows belonging to processes
- Elements: UI elements with handles and subscriptions

# Lifecycle

```text
Process (ProcessId / PID)
├─ created: first window seen for this app
├─ destroyed: no windows remain
└─ owns: ONE AXObserver for all notifications

Window (WindowId)
├─ created: window enumeration sees it
├─ destroyed: window enumeration stops seeing it
├─ belongs to: one Process
└─ owns: set of ElementIds

Element (ElementId)
├─ created: discovered via API (children, elementAt, focus)
├─ destroyed: notification or window removal
├─ belongs to: one Window
└─ owns: handle, subscriptions
```

# Cascade Behavior

- Individual elements can be removed (e.g., destroyed notification)
- Window removal cascades to all elements in that window
- Process removal cascades to all windows → all elements
*/

#![allow(unsafe_code)]

use crate::accessibility::Notification;
use crate::events;
use crate::platform::{self, ElementHandle, ObserverHandle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, Event, ProcessId, Selection, WindowId,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::LazyLock;

/// Per-process state: owns the `AXObserver` for this application.
struct ProcessState {
  /// The observer for this process (one per PID).
  /// All elements in all windows of this process share this observer.
  observer: ObserverHandle,
  /// Currently focused element in this app.
  focused_element: Option<ElementId>,
  /// Last selection state for deduplication.
  last_selection: Option<Selection>,
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
  /// `CFHash` of the element (for duplicate detection).
  hash: u64,
  /// `CFHash` of this element's OS parent (for lazy linking).
  /// None means this is a root element (no parent in OS tree).
  parent_hash: Option<u64>,
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

/// Alternative designs considered:
/// - **Dependency injection**: Pass `Arc<AxioInstance>` everywhere. Cleaner but doesn't work well with C callbacks.
/// - **Thread-local storage**: Doesn't work across threads (observer callbacks).
/// - **Context parameter**: Similar issues with C callbacks needing raw pointers.
struct Registry {
  /// Process state keyed by `ProcessId`.
  processes: HashMap<ProcessId, ProcessState>,
  /// Window state keyed by `WindowId`.
  windows: HashMap<WindowId, WindowState>,
  /// Element state keyed by `ElementId`.
  elements: HashMap<ElementId, ElementState>,

  // === Reverse Indexes ===
  /// `ElementId` → `WindowId` (for cascade lookups).
  element_to_window: HashMap<ElementId, WindowId>,
  /// `CFHash` → `ElementId` (for O(1) duplicate detection).
  hash_to_element: HashMap<u64, ElementId>,
  /// Parent hash → children waiting for that parent (lazy linking).
  waiting_for_parent: HashMap<u64, Vec<ElementId>>,

  // === Focus/Input ===
  /// Currently focused window (can be None when desktop is focused).
  focused_window: Option<WindowId>,
  /// Window depth order (front to back, by `z_index`).
  depth_order: Vec<WindowId>,
  /// Current mouse position.
  mouse_position: Option<crate::types::Point>,
}

static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::new()));

impl Registry {
  fn new() -> Self {
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

  /// Run a function with read access to the registry.
  fn read<F, R>(f: F) -> R
  where
    F: FnOnce(&Registry) -> R,
  {
    let guard = REGISTRY.read();
    f(&guard)
  }

  /// Run a function with write access to the registry.
  fn write<F, R>(f: F) -> R
  where
    F: FnOnce(&mut Registry) -> R,
  {
    let mut guard = REGISTRY.write();
    f(&mut guard)
  }

  /// Get or create process state for a PID.
  /// Creates the `AXObserver` if this is a new process.
  fn get_or_create_process(&mut self, pid: u32) -> AxioResult<ProcessId> {
    let process_id = ProcessId(pid);

    if self.processes.contains_key(&process_id) {
      return Ok(process_id);
    }

    let observer = platform::create_observer_for_pid(pid)?;

    // Subscribe to app-level notifications (focus, selection)
    if let Err(e) = platform::subscribe_app_notifications(pid, &observer) {
      log::warn!("Failed to subscribe app notifications for PID {pid}: {e:?}");
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

  /// Update windows from polling. Returns (events, added PIDs).
  fn update_windows_internal(
    &mut self,
    new_windows: Vec<AXWindow>,
  ) -> (Vec<Event>, Vec<ProcessId>) {
    let mut events = Vec::new();
    let mut added_window_ids = Vec::new();
    let mut new_process_ids = Vec::new();
    let mut changed_window_ids = Vec::new();

    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    let removed: Vec<WindowId> = self
      .windows
      .keys()
      .filter(|id| !new_ids.contains(id))
      .copied()
      .collect();

    for window_id in removed {
      events.extend(self.remove_window_internal(window_id));
    }

    // Process new/existing windows
    for window_info in new_windows {
      let window_id = window_info.id;
      let process_id = window_info.process_id;
      let pid = process_id.0;

      if let Some(existing) = self.windows.get_mut(&window_id) {
        if existing.info != window_info {
          changed_window_ids.push(window_id);
        }
        existing.info = window_info;

        if existing.handle.is_none() {
          existing.handle = platform::fetch_window_handle(&existing.info);
        }
      } else {
        // New window - ensure process exists
        if let Err(e) = self.get_or_create_process(pid) {
          log::warn!("Failed to create process for window {window_id}: {e:?}");
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
        added_window_ids.push(window_id);
        new_process_ids.push(process_id);
      }
    }

    let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
    windows.sort_by_key(|w| w.z_index);
    self.depth_order = windows.into_iter().map(|w| w.id).collect();

    for window_id in added_window_ids {
      if let Some(window) = self.windows.get(&window_id) {
        events.push(Event::WindowAdded {
          window: window.info.clone(),
        });
      }
    }

    for window_id in changed_window_ids {
      if let Some(window) = self.windows.get(&window_id) {
        events.push(Event::WindowChanged {
          window: window.info.clone(),
        });
      }
    }

    (events, new_process_ids)
  }

  /// Remove a window and cascade to all its elements.
  fn remove_window_internal(&mut self, window_id: WindowId) -> Vec<Event> {
    let mut events = Vec::new();

    let element_ids: Vec<ElementId> = self
      .elements
      .iter()
      .filter(|(_, e)| e.element.window_id == window_id)
      .map(|(id, _)| *id)
      .collect();

    for element_id in element_ids {
      events.extend(self.remove_element_internal(element_id));
    }

    if let Some(window_state) = self.windows.remove(&window_id) {
      let mut windows: Vec<_> = self.windows.values().map(|w| &w.info).collect();
      windows.sort_by_key(|w| w.z_index);
      self.depth_order = windows.into_iter().map(|w| w.id).collect();

      events.push(Event::WindowRemoved { window_id });

      let process_id = window_state.process_id;
      let has_windows = self.windows.values().any(|w| w.process_id == process_id);
      if !has_windows {
        self.processes.remove(&process_id);
      }
    }

    events
  }

  /// Register a new element. Returns existing if hash matches.
  /// Emits `ElementAdded` for newly registered elements.
  fn register_internal(
    &mut self,
    mut element: AXElement,
    handle: ElementHandle,
    pid: u32,
    platform_role: &str,
  ) -> Option<AXElement> {
    let window_id = element.window_id;
    let hash = platform::element_hash(&handle);

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

    let parent_hash = if element.is_root {
      None // Root elements have no parent
    } else {
      handle
        .get_element("AXParent")
        .map(|parent_handle| platform::element_hash(&parent_handle))
    };

    // Try to link orphan to parent if parent exists in registry
    if !element.is_root && element.parent_id.is_none() {
      if let Some(ref ph) = parent_hash {
        if let Some(&parent_id) = self.hash_to_element.get(ph) {
          element.parent_id = Some(parent_id);
        }
      }
    }

    let element_id = element.id;
    let mut state = ElementState {
      element: element.clone(),
      handle,
      hash,
      parent_hash,
      pid,
      platform_role: platform_role.to_string(),
      subscriptions: HashSet::new(),
      destruction_context: None,
      watch_context: None,
    };

    if let Some(process) = self.processes.get(&process_id) {
      Self::subscribe_destruction(&mut state, &process.observer);
    }

    self.elements.insert(element_id, state);
    self.element_to_window.insert(element_id, window_id);
    self.hash_to_element.insert(hash, element_id);

    if let Some(parent_id) = element.parent_id {
      self.add_child_to_parent(parent_id, element_id);
    } else if !element.is_root {
      // Orphan: has parent in OS but not loaded yet
      if let Some(ref ph) = parent_hash {
        self
          .waiting_for_parent
          .entry(*ph)
          .or_default()
          .push(element_id);
      }
    }

    if let Some(orphans) = self.waiting_for_parent.remove(&hash) {
      for orphan_id in orphans {
        self.link_orphan_to_parent(orphan_id, element_id);
      }
    }

    events::emit(Event::ElementAdded {
      element: element.clone(),
    });

    Some(element)
  }

  /// Link an orphan element to its newly-discovered parent.
  fn link_orphan_to_parent(&mut self, orphan_id: ElementId, parent_id: ElementId) {
    if let Some(orphan_state) = self.elements.get_mut(&orphan_id) {
      orphan_state.element.parent_id = Some(parent_id);
      events::emit(Event::ElementChanged {
        element: orphan_state.element.clone(),
      });
    }
    self.add_child_to_parent(parent_id, orphan_id);
  }

  /// Add a child to a parent's children list (if not already there).
  fn add_child_to_parent(&mut self, parent_id: ElementId, child_id: ElementId) {
    if let Some(parent_state) = self.elements.get_mut(&parent_id) {
      let children = parent_state.element.children.get_or_insert_with(Vec::new);
      if !children.contains(&child_id) {
        children.push(child_id);
        events::emit(Event::ElementChanged {
          element: parent_state.element.clone(),
        });
      }
    }
  }

  /// Subscribe to destruction notification for an element.
  fn subscribe_destruction(state: &mut ElementState, observer: &ObserverHandle) {
    if state.destruction_context.is_some() {
      return;
    }

    match platform::subscribe_destruction_notification(&state.element.id, &state.handle, observer) {
      Ok(context) => {
        state.destruction_context = Some(context.cast::<c_void>());
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
  fn remove_element_internal(&mut self, element_id: ElementId) -> Vec<Event> {
    let mut events = Vec::new();

    let Some(_window_id) = self.element_to_window.remove(&element_id) else {
      return events;
    };

    let Some(mut state) = self.elements.remove(&element_id) else {
      return events;
    };

    if let Some(parent_id) = state.element.parent_id {
      self.remove_child_from_parent(parent_id, element_id, &mut events);
    }

    if let Some(ref ph) = state.parent_hash {
      if let Some(waiting) = self.waiting_for_parent.get_mut(ph) {
        waiting.retain(|&id| id != element_id);
        if waiting.is_empty() {
          self.waiting_for_parent.remove(ph);
        }
      }
    }

    self.waiting_for_parent.remove(&state.hash);

    if let Some(children) = &state.element.children {
      for child_id in children.clone() {
        events.extend(self.remove_element_internal(child_id));
      }
    }

    self.hash_to_element.remove(&state.hash);

    let process_id = ProcessId(state.pid);
    if let Some(process) = self.processes.get(&process_id) {
      Self::unsubscribe_all(&mut state, &process.observer);
    }

    events.push(Event::ElementRemoved { element_id });

    events
  }

  /// Remove a child from a parent's children list.
  fn remove_child_from_parent(
    &mut self,
    parent_id: ElementId,
    child_id: ElementId,
    events: &mut Vec<Event>,
  ) {
    if let Some(parent_state) = self.elements.get_mut(&parent_id) {
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
  fn unsubscribe_all(state: &mut ElementState, observer: &ObserverHandle) {
    // Unsubscribe destruction tracking
    if let Some(context) = state.destruction_context.take() {
      platform::unsubscribe_destruction_notification(
        &state.handle,
        observer,
        context.cast::<platform::ObserverContextHandle>(),
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
        observer,
        context.cast::<platform::ObserverContextHandle>(),
        &notifs,
      );
    }

    state.subscriptions.clear();
  }

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
      .map(|p| &p.observer)
      .ok_or(AxioError::NotSupported("Process not found".into()))?;

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

    state.watch_context = Some(context.cast::<c_void>());
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
      &process.observer,
      context.cast::<platform::ObserverContextHandle>(),
      &notifs,
    );

    // Keep only Destroyed subscription
    state
      .subscriptions
      .retain(|n| *n == Notification::Destroyed);
  }
}

/// Update windows from polling. Returns PIDs of newly added windows for accessibility setup.
pub(crate) fn update_windows(new_windows: Vec<AXWindow>) -> Vec<ProcessId> {
  let (events, added_pids) = Registry::write(|r| r.update_windows_internal(new_windows));
  for event in events {
    events::emit(event);
  }
  added_pids
}

/// Get all windows.
pub(crate) fn get_windows() -> Vec<AXWindow> {
  Registry::read(|r| r.windows.values().map(|w| w.info.clone()).collect())
}

/// Get a specific window.
pub(crate) fn get_window(window_id: WindowId) -> Option<AXWindow> {
  Registry::read(|r| r.windows.get(&window_id).map(|w| w.info.clone()))
}

/// Get the focused window ID.
pub(crate) fn get_focused_window() -> Option<WindowId> {
  Registry::read(|r| r.focused_window)
}

/// Set currently focused window. Emits `FocusWindow` if value changed.
pub(crate) fn set_focused_window(window_id: Option<WindowId>) {
  let changed = Registry::write(|r| {
    if r.focused_window == window_id {
      false
    } else {
      r.focused_window = window_id;
      true
    }
  });
  if changed {
    events::emit(Event::FocusWindow { window_id });
  }
}

/// Get the focused window for a specific PID.
/// Returns the focused window ID if it belongs to the given process.
pub(crate) fn get_focused_window_for_pid(pid: u32) -> Option<WindowId> {
  Registry::read(|r| {
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
pub(crate) fn get_depth_order() -> Vec<WindowId> {
  Registry::read(|r| r.depth_order.clone())
}

/// Get a snapshot of the current registry state for sync.
/// Note: `accessibility_enabled` must be set by caller (platform-specific check).
pub(crate) fn snapshot() -> crate::types::Snapshot {
  Registry::read(|r| {
    let (focused_element, selection) = r
      .focused_window
      .and_then(|window_id| {
        let window = r.windows.get(&window_id)?;
        let process = r.processes.get(&window.process_id)?;

        let focused_elem = process
          .focused_element
          .and_then(|id| r.elements.get(&id).map(|s| s.element.clone()));

        Some((focused_elem, process.last_selection.clone()))
      })
      .unwrap_or((None, None));

    crate::types::Snapshot {
      windows: r.windows.values().map(|w| w.info.clone()).collect(),
      elements: r.elements.values().map(|s| s.element.clone()).collect(),
      focused_window: r.focused_window,
      focused_element,
      selection,
      depth_order: r.depth_order.clone(),
      mouse_position: r.mouse_position,
      accessibility_enabled: false, // Caller must set this
    }
  })
}

/// Find window at a point.
pub(crate) fn find_window_at_point(x: f64, y: f64) -> Option<AXWindow> {
  Registry::read(|r| {
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
pub(crate) fn get_window_with_handle(
  window_id: WindowId,
) -> Option<(AXWindow, Option<ElementHandle>)> {
  Registry::read(|r| {
    r.windows
      .get(&window_id)
      .map(|w| (w.info.clone(), w.handle.clone()))
  })
}

/// Update focused element for a process and emit `FocusElement` event.
/// Handles auto-watch/unwatch based on element roles.
/// Returns the previous focused element ID if focus actually changed.
pub(crate) fn update_focus(pid: u32, element: AXElement) -> Option<ElementId> {
  let (previous_id, should_emit) = Registry::write(|r| {
    let process_id = ProcessId(pid);
    let process = r.processes.get_mut(&process_id)?;

    let previous = process.focused_element;
    let same_element = previous == Some(element.id);

    if same_element {
      return Some((previous, false));
    }

    process.focused_element = Some(element.id);
    Some((previous, true))
  })?;

  if !should_emit {
    return previous_id;
  }

  // Auto-unwatch previous element
  if let Some(prev_id) = previous_id {
    if let Ok(prev_elem) = get_element(prev_id) {
      if prev_elem.role.auto_watch_on_focus() || prev_elem.role.is_writable() {
        unwatch_element(prev_id);
      }
    }
  }

  // Auto-watch new element
  if element.role.auto_watch_on_focus() || element.role.is_writable() {
    drop(watch_element(element.id));
  }

  // Emit focus event
  events::emit(Event::FocusElement {
    element,
    previous_element_id: previous_id,
  });

  previous_id
}

/// Update selection for a process and emit `SelectionChanged` event if changed.
pub(crate) fn update_selection(
  pid: u32,
  window_id: WindowId,
  element_id: ElementId,
  text: String,
  range: Option<crate::types::TextRange>,
) {
  let new_selection = Selection {
    element_id,
    text,
    range,
  };

  let should_emit = Registry::write(|r| {
    let process_id = ProcessId(pid);
    let process = r.processes.get_mut(&process_id)?;

    let changed = process.last_selection.as_ref() != Some(&new_selection);
    process.last_selection = Some(new_selection.clone());
    Some(changed)
  })
  .unwrap_or(false);

  if should_emit {
    events::emit(Event::SelectionChanged {
      window_id,
      element_id: new_selection.element_id,
      text: new_selection.text,
      range: new_selection.range,
    });
  }
}

/// Register an element. Returns existing if hash matches.
/// Emits `ElementAdded` for newly registered elements.
pub(crate) fn register_element(
  element: AXElement,
  handle: ElementHandle,
  pid: u32,
  platform_role: &str,
) -> Option<AXElement> {
  Registry::write(|r| r.register_internal(element, handle, pid, platform_role))
}

/// Get element by ID.
pub(crate) fn get_element(element_id: ElementId) -> AxioResult<AXElement> {
  Registry::read(|r| {
    r.elements
      .get(&element_id)
      .map(|e| e.element.clone())
      .ok_or(AxioError::ElementNotFound(element_id))
  })
}

/// Get element by hash (for checking if element is already registered).
pub(crate) fn get_element_by_hash(hash: u64) -> Option<AXElement> {
  Registry::read(|r| {
    r.hash_to_element
      .get(&hash)
      .and_then(|id| r.elements.get(id))
      .map(|e| e.element.clone())
  })
}

/// Get multiple elements by ID.
pub(crate) fn get_elements(element_ids: &[ElementId]) -> Vec<AXElement> {
  Registry::read(|r| {
    element_ids
      .iter()
      .filter_map(|id| r.elements.get(id).map(|e| e.element.clone()))
      .collect()
  })
}

/// Get all elements.
pub(crate) fn get_all_elements() -> Vec<AXElement> {
  Registry::read(|r| r.elements.values().map(|e| e.element.clone()).collect())
}

/// Update element data. Emits `ElementChanged` if the element actually changed.
pub(crate) fn update_element(element_id: ElementId, updated: AXElement) -> AxioResult<()> {
  let maybe_event = Registry::write(|r| {
    let state = r
      .elements
      .get_mut(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;

    // Only emit if something changed
    if state.element == updated {
      Ok(None)
    } else {
      state.element = updated.clone();
      Ok(Some(Event::ElementChanged { element: updated }))
    }
  })?;

  if let Some(event) = maybe_event {
    events::emit(event);
  }
  Ok(())
}

/// Set children for an element. Emits `ElementChanged` if children changed.
pub(crate) fn set_element_children(
  element_id: ElementId,
  children: Vec<ElementId>,
) -> AxioResult<()> {
  let maybe_event = Registry::write(|r| {
    let state = r
      .elements
      .get_mut(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;

    let new_children = Some(children);
    // Only emit if children changed
    if state.element.children == new_children {
      Ok(None)
    } else {
      state.element.children = new_children;
      Ok(Some(Event::ElementChanged {
        element: state.element.clone(),
      }))
    }
  })?;

  if let Some(event) = maybe_event {
    events::emit(event);
  }
  Ok(())
}

/// Remove an element (cascades to children).
pub(crate) fn remove_element(element_id: ElementId) {
  let events = Registry::write(|r| r.remove_element_internal(element_id));
  for event in events {
    events::emit(event);
  }
}

/// Watch an element for notifications.
pub(crate) fn watch_element(element_id: ElementId) -> AxioResult<()> {
  Registry::write(|r| r.watch_internal(&element_id))
}

/// Stop watching an element.
pub(crate) fn unwatch_element(element_id: ElementId) {
  Registry::write(|r| r.unwatch_internal(&element_id));
}

/// Access stored element for operations (click, write).
pub(crate) fn with_element_handle<F, R>(element_id: ElementId, f: F) -> AxioResult<R>
where
  F: FnOnce(&ElementHandle, &str) -> R,
{
  Registry::read(|r| {
    let state = r
      .elements
      .get(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;
    Ok(f(&state.handle, &state.platform_role))
  })
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

/// Get full stored element info for operations that need it.
pub(crate) fn get_stored_element_info(element_id: ElementId) -> AxioResult<StoredElementInfo> {
  Registry::read(|r| {
    let state = r
      .elements
      .get(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;
    Ok(StoredElementInfo {
      handle: state.handle.clone(),
      window_id: state.element.window_id,
      pid: state.pid,
      platform_role: state.platform_role.clone(),
      is_root: state.element.is_root,
      parent_id: state.element.parent_id,
      children: state.element.children.clone(),
    })
  })
}

/// Write typed value to element.
pub(crate) fn write_element_value(
  element_id: ElementId,
  value: &crate::accessibility::Value,
) -> AxioResult<()> {
  with_element_handle(element_id, |handle, platform_role| {
    platform::write_element_value(handle, value, platform_role)
  })?
}

/// Click element.
pub(crate) fn click_element(element_id: ElementId) -> AxioResult<()> {
  with_element_handle(element_id, |handle, _| platform::click_element(handle))?
}

/// Update mouse position and emit event if changed.
pub(crate) fn update_mouse_position(pos: crate::types::Point) {
  let changed = Registry::write(|r| {
    let changed = r
      .mouse_position
      .is_none_or(|last| pos.moved_from(last, 1.0));
    if changed {
      r.mouse_position = Some(pos);
    }
    changed
  });
  if changed {
    events::emit(Event::MousePosition(pos));
  }
}
