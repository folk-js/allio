/*!
Query methods for Axio.

- `get_*` = registry/state lookups (fast, no OS calls)
- `fetch_*` = platform/OS calls (may be slow)
*/

use super::Axio;
use crate::platform::{CurrentPlatform, Handle, Platform};
use crate::types::{
  Element, Window, AxioError, AxioResult, ElementId, ProcessId, TextSelection, WindowId,
};

// ============================================================================
// Registry Lookups (get_*)
// ============================================================================

impl Axio {
  /// Get all windows from registry.
  pub fn get_windows(&self) -> Vec<Window> {
    self.read(|s| s.get_all_windows().cloned().collect())
  }

  /// Get a specific window from registry.
  pub fn get_window(&self, window_id: WindowId) -> Option<Window> {
    self.read(|s| s.get_window(window_id).cloned())
  }

  /// Get the focused window ID from registry.
  pub fn get_focused_window(&self) -> Option<WindowId> {
    self.read(|s| s.get_focused_window())
  }

  /// Get window z-order (front to back) from registry.
  pub fn get_z_order(&self) -> Vec<WindowId> {
    self.read(|s| s.get_z_order().to_vec())
  }

  /// Get element by ID from registry (with derived relationships).
  pub fn get_element(&self, element_id: ElementId) -> Option<Element> {
    self.read(|s| s.get_element(element_id))
  }

  /// Get element by hash from registry.
  pub(crate) fn get_element_by_hash(&self, hash: u64) -> Option<Element> {
    self.read(|s| {
      s.find_element_by_hash(hash)
        .and_then(|id| s.get_element(id))
    })
  }

  /// Get multiple elements by ID from registry.
  pub fn get_elements(&self, element_ids: &[ElementId]) -> Vec<Element> {
    self.read(|s| {
      element_ids
        .iter()
        .filter_map(|id| s.get_element(*id))
        .collect()
    })
  }

  /// Get all elements from registry.
  pub fn get_all_elements(&self) -> Vec<Element> {
    self.read(|s| s.get_all_elements())
  }

  /// Get a snapshot of the current state for sync.
  pub fn get_snapshot(&self) -> crate::types::Snapshot {
    self.read(|s| s.snapshot())
  }

  /// Get window at a point from registry.
  pub(crate) fn get_window_at_point(&self, x: f64, y: f64) -> Option<Window> {
    self.read(|s| s.get_window_at_point(x, y).cloned())
  }

  /// Get window info with handle from registry.
  pub(crate) fn get_window_with_handle(
    &self,
    window_id: WindowId,
  ) -> Option<(Window, Option<Handle>)> {
    self.read(|s| {
      let window = s.get_window(window_id)?;
      let handle = s.get_window_handle(window_id).cloned();
      Some((window.clone(), handle))
    })
  }

  /// Get the focused window for a specific PID from registry.
  pub(crate) fn get_focused_window_for_pid(&self, pid: u32) -> Option<WindowId> {
    self.read(|s| s.get_focused_window_for_pid(pid))
  }

  /// Get the app element handle for a process from registry.
  pub(crate) fn get_app_handle(&self, pid: u32) -> Option<Handle> {
    self.read(|s| s.get_process(ProcessId(pid)).map(|p| p.app_handle.clone()))
  }

  /// Get element handle and metadata. Use this to extract data before platform calls.
  pub(crate) fn get_element_handle(
    &self,
    element_id: ElementId,
  ) -> AxioResult<(Handle, WindowId, u32)> {
    self.read(|s| {
      let e = s
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.data.window_id, e.pid()))
    })
  }

  /// Get element state data needed for refresh operations.
  /// Returns (handle, window_id, pid, is_root).
  pub(crate) fn get_element_for_refresh(
    &self,
    element_id: ElementId,
  ) -> AxioResult<(Handle, WindowId, u32, bool)> {
    self.read(|s| {
      let e = s
        .get_element_state(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.data.window_id, e.pid(), e.data.is_root))
    })
  }
}

// ============================================================================
// Platform Fetches (fetch_*)
// ============================================================================

impl Axio {
  /// Check if accessibility permissions are granted.
  pub fn has_permissions() -> bool {
    CurrentPlatform::has_permissions()
  }

  /// Fetch screen dimensions (width, height) from OS.
  pub fn fetch_screen_size(&self) -> (f64, f64) {
    CurrentPlatform::fetch_screen_size()
  }

  /// Fetch element at screen coordinates from OS.
  ///
  /// Returns `Ok(None)` if no tracked window exists at the position.
  /// This is not an error - it's valid to query positions outside windows.
  pub fn fetch_element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>> {
    crate::platform::element_ops::fetch_element_at_position(self, x, y)
  }

  /// Fetch and register children of element from OS.
  pub fn fetch_children(
    &self,
    element_id: ElementId,
    max_children: usize,
  ) -> AxioResult<Vec<Element>> {
    crate::platform::element_ops::fetch_children(self, element_id, max_children)
  }

  /// Fetch and register parent of element from OS (None if element is root).
  pub fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<Element>> {
    crate::platform::element_ops::fetch_parent(self, element_id)
  }

  /// Fetch fresh element attributes from OS.
  pub fn fetch_element(&self, element_id: ElementId) -> AxioResult<Element> {
    crate::platform::element_ops::fetch_element(self, element_id)
  }

  /// Fetch root element for a window from OS.
  pub fn fetch_window_root(&self, window_id: WindowId) -> AxioResult<Element> {
    crate::platform::element_ops::fetch_window_root(self, window_id)
  }

  /// Fetch currently focused element and text selection for a window from OS.
  pub fn fetch_window_focus(
    &self,
    window_id: WindowId,
  ) -> AxioResult<(Option<Element>, Option<TextSelection>)> {
    let window = self
      .get_window(window_id)
      .ok_or(AxioError::WindowNotFound(window_id))?;
    crate::platform::element_ops::fetch_focus(self, window.process_id.0)
  }
}
