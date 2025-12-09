/*!
Query methods for Axio.

- `get_*` = registry/state lookups (fast, no OS calls)
- `fetch_*` = platform/OS calls (may be slow)
*/

use super::state::ElementState;
use super::Axio;
use crate::platform::{self, Handle};
use crate::types::{
  AXElement, AXWindow, AxioError, AxioResult, ElementId, ProcessId, TextSelection, WindowId,
};

// ============================================================================
// Registry Lookups (get_*)
// ============================================================================

impl Axio {
  /// Get all windows from registry.
  pub fn get_windows(&self) -> Vec<AXWindow> {
    self.state.read().get_all_windows().cloned().collect()
  }

  /// Get a specific window from registry.
  pub fn get_window(&self, window_id: WindowId) -> Option<AXWindow> {
    self.state.read().get_window(window_id).cloned()
  }

  /// Get the focused window ID from registry.
  pub fn get_focused_window(&self) -> Option<WindowId> {
    self.state.read().get_focused_window()
  }

  /// Get window depth order (front to back) from registry.
  pub fn get_depth_order(&self) -> Vec<WindowId> {
    self.state.read().get_depth_order().to_vec()
  }

  /// Get element by ID from registry.
  pub fn get_element(&self, element_id: ElementId) -> Option<AXElement> {
    self.state.read().get_element(element_id).cloned()
  }

  /// Get element by hash from registry (for checking if element is already registered).
  pub(crate) fn get_element_by_hash(&self, hash: u64) -> Option<AXElement> {
    let state = self.state.read();
    state
      .find_element_by_hash(hash)
      .and_then(|id| state.get_element(id).cloned())
  }

  /// Get multiple elements by ID from registry.
  pub fn get_elements(&self, element_ids: &[ElementId]) -> Vec<AXElement> {
    let state = self.state.read();
    element_ids
      .iter()
      .filter_map(|id| state.get_element(*id).cloned())
      .collect()
  }

  /// Get all elements from registry.
  pub fn get_all_elements(&self) -> Vec<AXElement> {
    self.state.read().get_all_elements().cloned().collect()
  }

  /// Get a snapshot of the current state for sync.
  pub fn get_snapshot(&self) -> crate::types::Snapshot {
    self.state.read().snapshot()
  }

  /// Get window at a point from registry.
  pub(crate) fn get_window_at_point(&self, x: f64, y: f64) -> Option<AXWindow> {
    self.state.read().get_window_at_point(x, y).cloned()
  }

  /// Get window info with handle from registry.
  pub(crate) fn get_window_with_handle(&self, window_id: WindowId) -> Option<(AXWindow, Option<Handle>)> {
    let state = self.state.read();
    let window = state.get_window(window_id)?;
    let handle = state.get_window_handle(window_id).cloned();
    Some((window.clone(), handle))
  }

  /// Get the focused window for a specific PID from registry.
  pub(crate) fn get_focused_window_for_pid(&self, pid: u32) -> Option<WindowId> {
    self.state.read().get_focused_window_for_pid(pid)
  }

  /// Get the app element handle for a process from registry.
  pub(crate) fn get_app_handle(&self, pid: u32) -> Option<Handle> {
    self.state.read().get_process(ProcessId(pid)).map(|p| p.app_handle.clone())
  }

  /// Access element state via closure.
  pub(crate) fn with_element<F, R>(&self, element_id: ElementId, f: F) -> AxioResult<R>
  where
    F: FnOnce(&ElementState) -> R,
  {
    let state = self.state.read();
    let elem_state = state
      .get_element_state(element_id)
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
  pub fn fetch_children(&self, element_id: ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
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
  pub fn fetch_window_focus(&self, window_id: WindowId) -> AxioResult<(Option<AXElement>, Option<TextSelection>)> {
    let window = self
      .get_window(window_id)
      .ok_or(AxioError::WindowNotFound(window_id))?;
    Ok(crate::platform::element_ops::fetch_focus(self, window.process_id.0))
  }
}
