/*!
AXIO - Accessibility I/O Layer

```ignore
use axio::Axio;

// Create instance (polling starts automatically)
let axio = Axio::new()?;

// Query state
let windows = axio.get_windows();
let element = axio.element_at(100.0, 200.0)?;
let children = axio.children(element.id, 100)?;

// Subscribe to events
let mut events = axio.subscribe();
while let Ok(event) = events.recv().await {
    // handle event
}

// Polling stops when axio is dropped
drop(axio);
```
*/

mod core;
mod platform;
mod polling;

pub mod accessibility;

mod types;
pub use types::*;

pub use crate::core::Axio;
pub use polling::AxioOptions;

impl Axio {
  /// Check if accessibility permissions are granted.
  pub fn verify_permissions() -> bool {
    platform::check_accessibility_permissions()
  }

  /// Get screen dimensions (width, height).
  pub fn screen_size(&self) -> (f64, f64) {
    platform::get_main_screen_dimensions()
  }

  /// Discover element at screen coordinates.
  pub fn element_at(&self, x: f64, y: f64) -> AxioResult<AXElement> {
    core::element_ops::get_element_at_position(self, x, y)
  }

  /// Fetch and register children of element from platform.
  pub fn fetch_children(&self, element_id: ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
    core::element_ops::fetch_children(self, element_id, max_children)
  }

  /// Fetch and register parent of element from platform (None if element is root).
  pub fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<AXElement>> {
    core::element_ops::fetch_parent(self, element_id)
  }

  /// Fetch fresh element attributes from platform.
  pub fn fetch_element(&self, element_id: ElementId) -> AxioResult<AXElement> {
    core::element_ops::fetch_element(self, element_id)
  }

  /// Write a typed value to an element.
  pub fn write(&self, element_id: ElementId, value: &accessibility::Value) -> AxioResult<()> {
    self.write_element_value(element_id, value)
  }

  /// Click an element.
  pub fn click(&self, element_id: ElementId) -> AxioResult<()> {
    self.click_element(element_id)
  }

  /// Watch element for changes.
  pub fn watch(&self, element_id: ElementId) -> AxioResult<()> {
    self.watch_element(element_id)
  }

  /// Stop watching element.
  pub fn unwatch(&self, element_id: ElementId) -> AxioResult<()> {
    self.unwatch_element(element_id)
  }

  /// Get the root element for a window.
  pub fn window_root(&self, window_id: WindowId) -> AxioResult<AXElement> {
    core::element_ops::get_window_root(self, window_id)
  }

  /// Fetch currently focused element and text selection for a window from platform.
  pub fn fetch_window_focus(
    &self,
    window_id: WindowId,
  ) -> AxioResult<(Option<AXElement>, Option<TextSelection>)> {
    let window = self
      .get_window(window_id)
      .ok_or(AxioError::WindowNotFound(window_id))?;
    Ok(core::element_ops::fetch_focus(self, window.process_id.0))
  }
}
