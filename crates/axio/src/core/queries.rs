/*!
Query methods for Axio.

- `get_*` = registry/state lookups (fast, no OS calls)
- `fetch_*` = platform/OS calls (may be slow)
*/

use super::state::ElementState;
use super::Axio;
use crate::platform::{self, Handle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, TextSelection, WindowId,
};

// ============================================================================
// Registry Lookups (get_*)
// ============================================================================

impl Axio {
  /// Get all windows from registry.
  pub fn get_windows(&self) -> Vec<AXWindow> {
    self
      .state
      .read()
      .windows
      .values()
      .map(|w| w.info.clone())
      .collect()
  }

  /// Get a specific window from registry.
  pub fn get_window(&self, window_id: WindowId) -> Option<AXWindow> {
    self
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| w.info.clone())
  }

  /// Get the focused window ID from registry.
  pub fn get_focused_window(&self) -> Option<WindowId> {
    self.state.read().focused_window
  }

  /// Get window depth order (front to back) from registry.
  pub fn get_depth_order(&self) -> Vec<WindowId> {
    self.state.read().depth_order.clone()
  }

  /// Get element by ID from registry.
  pub fn get_element(&self, element_id: ElementId) -> Option<AXElement> {
    self
      .state
      .read()
      .elements
      .get(&element_id)
      .map(|e| e.element.clone())
  }

  /// Get element by hash from registry (for checking if element is already registered).
  pub(crate) fn get_element_by_hash(&self, hash: u64) -> Option<AXElement> {
    let state = self.state.read();
    state
      .hash_to_element
      .get(&hash)
      .and_then(|id| state.elements.get(id))
      .map(|e| e.element.clone())
  }

  /// Get multiple elements by ID from registry.
  pub fn get_elements(&self, element_ids: &[ElementId]) -> Vec<AXElement> {
    let state = self.state.read();
    element_ids
      .iter()
      .filter_map(|id| state.elements.get(id).map(|e| e.element.clone()))
      .collect()
  }

  /// Get all elements from registry.
  pub fn get_all_elements(&self) -> Vec<AXElement> {
    self
      .state
      .read()
      .elements
      .values()
      .map(|e| e.element.clone())
      .collect()
  }

  /// Get a snapshot of the current state for sync.
  pub fn snapshot(&self) -> crate::types::Snapshot {
    let state = self.state.read();
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

  /// Get window at a point from registry.
  pub(crate) fn get_window_at_point(&self, x: f64, y: f64) -> Option<AXWindow> {
    let state = self.state.read();
    let point = crate::Point::new(x, y);
    let mut candidates: Vec<_> = state
      .windows
      .values()
      .filter(|w| w.info.bounds.contains(point))
      .collect();
    candidates.sort_by_key(|w| w.info.z_index);
    candidates.first().map(|w| w.info.clone())
  }

  /// Get window info with handle from registry.
  pub(crate) fn get_window_with_handle(
    &self,
    window_id: WindowId,
  ) -> Option<(AXWindow, Option<Handle>)> {
    self
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| (w.info.clone(), w.handle.clone()))
  }

  /// Get the focused window for a specific PID from registry.
  pub(crate) fn get_focused_window_for_pid(&self, pid: u32) -> Option<WindowId> {
    let state = self.state.read();
    let window_id = state.focused_window?;
    let window_state = state.windows.get(&window_id)?;
    if window_state.process_id.0 == pid {
      Some(window_id)
    } else {
      None
    }
  }

  /// Get the app element handle for a process from registry.
  /// Returns None if the process hasn't been registered yet.
  pub(crate) fn get_app_handle(&self, pid: u32) -> Option<Handle> {
    self
      .state
      .read()
      .processes
      .get(&crate::types::ProcessId(pid))
      .map(|p| p.app_handle.clone())
  }

  /// Access element state via closure.
  ///
  /// Use this to extract what you need from element state without
  /// copying everything into a separate struct.
  pub(crate) fn with_element<F, R>(&self, element_id: ElementId, f: F) -> AxioResult<R>
  where
    F: FnOnce(&ElementState) -> R,
  {
    let state = self.state.read();
    let elem_state = state
      .elements
      .get(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;
    Ok(f(elem_state))
  }
}

// ============================================================================
// Platform Fetches (fetch_*)
// ============================================================================

impl Axio {
  /// Check if accessibility permissions are granted.
  pub fn verify_permissions() -> bool {
    platform::check_accessibility_permissions()
  }

  /// Fetch screen dimensions (width, height) from OS.
  pub fn fetch_screen_size(&self) -> (f64, f64) {
    platform::fetch_screen_size()
  }

  /// Fetch element at screen coordinates from OS.
  pub fn fetch_element_at(&self, x: f64, y: f64) -> AxioResult<AXElement> {
    crate::platform::element_ops::fetch_element_at_position(self, x, y)
  }

  /// Fetch and register children of element from OS.
  pub fn fetch_children(
    &self,
    element_id: ElementId,
    max_children: usize,
  ) -> AxioResult<Vec<AXElement>> {
    crate::platform::element_ops::fetch_children(self, element_id, max_children)
  }

  /// Fetch and register parent of element from OS (None if element is root).
  pub fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<AXElement>> {
    crate::platform::element_ops::fetch_parent(self, element_id)
  }

  /// Fetch fresh element attributes from OS.
  pub fn fetch_element(&self, element_id: ElementId) -> AxioResult<AXElement> {
    crate::platform::element_ops::fetch_element(self, element_id)
  }

  /// Fetch root element for a window from OS.
  pub fn fetch_window_root(&self, window_id: WindowId) -> AxioResult<AXElement> {
    crate::platform::element_ops::fetch_window_root(self, window_id)
  }

  /// Fetch currently focused element and text selection for a window from OS.
  pub fn fetch_window_focus(
    &self,
    window_id: WindowId,
  ) -> AxioResult<(Option<AXElement>, Option<TextSelection>)> {
    let window = self
      .get_window(window_id)
      .ok_or(AxioError::WindowNotFound(window_id))?;
    Ok(crate::platform::element_ops::fetch_focus(self, window.process_id.0))
  }
}

