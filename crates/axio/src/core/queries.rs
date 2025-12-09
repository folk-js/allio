/*!
Read-only state queries.
*/

use super::state::ElementState;
use super::Axio;
use crate::platform::ElementHandle;
use crate::types::{AXElement, AXWindow, AxioError, AxioResult, ElementId, WindowId};

impl Axio {
  /// Get all windows.
  pub fn get_windows(&self) -> Vec<AXWindow> {
    self
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
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| w.info.clone())
  }

  /// Get the focused window ID.
  pub fn get_focused_window(&self) -> Option<WindowId> {
    self.state.read().focused_window
  }

  /// Get window depth order (front to back).
  pub fn get_depth_order(&self) -> Vec<WindowId> {
    self.state.read().depth_order.clone()
  }

  /// Get element by ID.
  pub fn get_element(&self, element_id: ElementId) -> Option<AXElement> {
    self
      .state
      .read()
      .elements
      .get(&element_id)
      .map(|e| e.element.clone())
  }

  /// Get element by hash (for checking if element is already registered).
  pub(crate) fn get_element_by_hash(&self, hash: u64) -> Option<AXElement> {
    let state = self.state.read();
    state
      .hash_to_element
      .get(&hash)
      .and_then(|id| state.elements.get(id))
      .map(|e| e.element.clone())
  }

  /// Get multiple elements by ID.
  pub fn get_elements(&self, element_ids: &[ElementId]) -> Vec<AXElement> {
    let state = self.state.read();
    element_ids
      .iter()
      .filter_map(|id| state.elements.get(id).map(|e| e.element.clone()))
      .collect()
  }

  /// Get all elements.
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

  /// Find window at a point.
  pub(crate) fn find_window_at_point(&self, x: f64, y: f64) -> Option<AXWindow> {
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

  /// Get window info with handle.
  pub(crate) fn get_window_with_handle(
    &self,
    window_id: WindowId,
  ) -> Option<(AXWindow, Option<ElementHandle>)> {
    self
      .state
      .read()
      .windows
      .get(&window_id)
      .map(|w| (w.info.clone(), w.handle.clone()))
  }

  /// Get the focused window for a specific PID.
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
