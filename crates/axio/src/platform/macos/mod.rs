/*!
macOS platform implementation.

Implements the platform traits defined in `platform/mod.rs`.
All macOS-specific code (AXUIElement, CoreFoundation, etc.) stays within this module.
*/

// === Internal modules ===
mod cf_utils;
mod display;
mod display_link;
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

use std::sync::Arc;

use crate::accessibility::{Action, Notification, Value};
use crate::platform::traits::{
  AppNotificationHandle, DisplayLinkHandle, ElementAttributes, Platform, PlatformCallbacks,
  PlatformHandle, PlatformObserver, WatchHandle,
};
use crate::types::{AxioError, AxioResult, ElementId, Point};
use mapping::action_to_macos;

/// macOS platform implementation.
pub(crate) struct MacOS;

impl Platform for MacOS {
  type Handle = ElementHandle;
  type Observer = ObserverHandle;

  fn has_permissions() -> bool {
    util::has_permissions()
  }

  fn fetch_windows(_exclude_pid: Option<u32>) -> Vec<crate::types::Window> {
    // Note: exclude_pid filtering happens in polling.rs, not here
    window_list::enumerate_windows()
  }

  fn fetch_screen_size() -> (f64, f64) {
    display::get_main_screen_dimensions()
  }

  fn fetch_mouse_position() -> Point {
    mouse::get_mouse_position().unwrap_or_else(|| Point::new(0.0, 0.0))
  }

  fn fetch_window_handle(window: &crate::types::Window) -> Option<Self::Handle> {
    window::fetch_window_handle(window)
  }

  fn create_observer<C: PlatformCallbacks<Handle = Self::Handle>>(
    pid: u32,
    callbacks: Arc<C>,
  ) -> AxioResult<Self::Observer> {
    observer::create_observer_for_pid(pid, callbacks)
  }

  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle> {
    display_link::start_display_link(callback)
      .ok()
      .map(|inner| DisplayLinkHandle { inner })
  }

  fn enable_accessibility_for_pid(pid: u32) {
    window::enable_accessibility_for_pid(crate::ProcessId(pid));
  }

  fn app_element(pid: u32) -> Self::Handle {
    ElementHandle::new(util::app_element(pid))
  }
}

impl PlatformHandle for ElementHandle {
  fn pid(&self) -> u32 {
    self.cached_pid
  }

  fn fetch_children(&self) -> Vec<Self> {
    self.get_children()
  }

  fn fetch_parent(&self) -> Option<Self> {
    self.get_element("AXParent")
  }

  fn set_value(&self, value: &Value) -> AxioResult<()> {
    self
      .set_typed_value(value)
      .map_err(|e| AxioError::SetValueFailed {
        reason: format!("{e:?}"),
      })
  }

  fn perform_action(&self, action: Action) -> AxioResult<()> {
    let action_str = action_to_macos(action);
    self
      .perform_action_internal(action_str)
      .map_err(|e| AxioError::ActionFailed {
        action,
        reason: format!("{e:?}"),
      })
  }

  fn fetch_attributes(&self) -> ElementAttributes {
    self.fetch_attributes_internal(None)
  }

  fn fetch_element_at_position(&self, x: f64, y: f64) -> Option<Self> {
    handles::ElementHandle::element_at_position(self, x, y)
  }

  fn window(&self) -> Option<Self> {
    self.get_element("AXWindow")
  }
}

impl PlatformObserver for ObserverHandle {
  type Handle = ElementHandle;

  fn subscribe_app_notifications<C: PlatformCallbacks<Handle = Self::Handle>>(
    &self,
    pid: u32,
    callbacks: Arc<C>,
  ) -> AxioResult<AppNotificationHandle> {
    let inner = notifications::subscribe_app_notifications(pid, self, callbacks)?;
    Ok(AppNotificationHandle { _inner: inner })
  }

  fn create_watch<C: PlatformCallbacks<Handle = Self::Handle>>(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    initial_notifications: &[Notification],
    callbacks: Arc<C>,
  ) -> AxioResult<WatchHandle> {
    let inner =
      notifications::create_watch(self, handle, element_id, initial_notifications, callbacks)?;
    Ok(WatchHandle { inner })
  }
}

// === Type Exports ===

pub(crate) type MacOSDisplayLinkHandle = display_link::DisplayLinkHandle;
pub(crate) type WatchHandleInner = notifications::WatchHandleInner;
pub(crate) type AppNotificationHandleInner = notifications::AppNotificationHandleInner;
