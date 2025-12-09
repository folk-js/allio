/*!
macOS platform implementation.

Implements the platform traits defined in `platform/mod.rs`.
All macOS-specific code (AXUIElement, CoreFoundation, etc.) stays within this module.
*/

// === Internal modules ===
mod cf_utils;
mod display;
mod display_link;
mod element;
mod focus;
mod handles;
pub(crate) mod mapping;
mod mouse;
mod notifications;
mod observer;
mod util;
mod window;
mod window_list;

// === Re-exports for internal use ===
pub(super) use handles::{ElementHandle, ObserverHandle};

// === Trait Implementations ===

use crate::accessibility::{Notification, Value};
use crate::core::Axio;
use crate::platform::traits::{
  DisplayLinkHandle, ElementAttributes, Platform, PlatformHandle, PlatformObserver, WatchHandle,
};
use crate::types::{AxioError, AxioResult, ElementId, Point};

/// macOS platform implementation.
pub(crate) struct MacOS;

impl Platform for MacOS {
  type Handle = ElementHandle;
  type Observer = ObserverHandle;

  fn check_permissions() -> bool {
    util::check_accessibility_permissions()
  }

  fn fetch_windows(_exclude_pid: Option<u32>) -> Vec<crate::types::AXWindow> {
    // Note: exclude_pid filtering happens in polling.rs, not here
    window_list::enumerate_windows()
  }

  fn fetch_screen_size() -> (f64, f64) {
    display::get_main_screen_dimensions()
  }

  fn fetch_mouse_position() -> Point {
    mouse::get_mouse_position().unwrap_or_else(|| Point::new(0.0, 0.0))
  }

  fn window_handle(window: &crate::types::AXWindow) -> Option<Self::Handle> {
    window::fetch_window_handle(window)
  }

  fn create_observer(pid: u32, axio: Axio) -> AxioResult<Self::Observer> {
    observer::create_observer_for_pid(pid, axio)
  }

  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle> {
    display_link::start_display_link(callback)
      .ok()
      .map(|inner| DisplayLinkHandle { inner })
  }

  fn enable_accessibility_for_pid(pid: u32) {
    window::enable_accessibility_for_pid(crate::ProcessId(pid));
  }

  fn fetch_focused_element(app_handle: &Self::Handle) -> Option<Self::Handle> {
    app_handle.get_element("AXFocusedUIElement")
  }

  fn app_element(pid: u32) -> Self::Handle {
    ElementHandle::new(util::app_element(pid))
  }
}

impl PlatformHandle for ElementHandle {
  fn fetch_children(&self) -> Vec<Self> {
    self.get_children()
  }

  fn fetch_parent(&self) -> Option<Self> {
    self.get_element("AXParent")
  }

  fn element_hash(&self) -> u64 {
    element::element_hash(self)
  }

  fn set_value(&self, value: &Value) -> AxioResult<()> {
    self
      .set_typed_value(value)
      .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {e:?}")))
  }

  fn perform_action(&self, action: &str) -> AxioResult<()> {
    self
      .perform_action_internal(action)
      .map_err(|e| AxioError::AccessibilityError(format!("Action '{action}' failed: {e:?}")))
  }

  fn fetch_attributes(&self) -> ElementAttributes {
    self.fetch_attributes_internal(None)
  }

  fn fetch_element_at_position(&self, x: f64, y: f64) -> Option<Self> {
    handles::ElementHandle::element_at_position(self, x, y)
  }

  fn fetch_selection(&self) -> Option<(String, Option<(u32, u32)>)> {
    focus::get_selection_from_handle(self)
  }
}

impl PlatformObserver for ObserverHandle {
  type Handle = ElementHandle;

  fn subscribe_app_notifications(&self, pid: u32, axio: Axio) -> AxioResult<()> {
    notifications::subscribe_app_notifications(pid, self, axio)
  }

  fn create_watch(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    initial_notifications: &[Notification],
    axio: Axio,
  ) -> AxioResult<WatchHandle> {
    let inner = notifications::create_watch(self, handle, element_id, initial_notifications, axio)?;
    Ok(WatchHandle { inner })
  }
}

// === Type Exports ===

pub(crate) type MacOSDisplayLinkHandle = display_link::DisplayLinkHandle;
pub(crate) type WatchHandleInner = notifications::WatchHandleInner;
