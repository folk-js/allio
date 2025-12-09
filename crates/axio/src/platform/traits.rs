/*!
Platform abstraction traits.

These traits define the contract between core code and platform implementations.
Platform-specific code (e.g., macOS) implements these traits.
Core code only uses these traits - never platform-specific types directly.
*/

#![allow(unsafe_code)]

use crate::accessibility::{Notification, Value};
use crate::core::Axio;
use crate::types::{AXWindow, AxioResult, ElementId};

/// Attributes fetched from a platform element.
/// This is the cross-platform interface for element data.
#[derive(Debug, Default)]
pub(crate) struct ElementAttributes {
  pub role: Option<String>,
  pub subrole: Option<String>,
  pub title: Option<String>,
  pub value: Option<Value>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub url: Option<String>,
  pub bounds: Option<crate::types::Bounds>,
  pub focused: Option<bool>,
  pub disabled: bool,
  pub selected: Option<bool>,
  pub expanded: Option<bool>,
  pub row_index: Option<usize>,
  pub column_index: Option<usize>,
  pub row_count: Option<usize>,
  pub column_count: Option<usize>,
  pub actions: Vec<crate::accessibility::Action>,
}

/// Platform-global operations (not tied to a specific element).
pub(crate) trait Platform {
  type Handle: PlatformHandle;
  type Observer: PlatformObserver<Handle = Self::Handle>;

  /// Check if accessibility permissions are granted.
  fn check_permissions() -> bool;

  /// Fetch all visible windows.
  fn fetch_windows(exclude_pid: Option<u32>) -> Vec<AXWindow>;

  /// Get main screen dimensions (width, height).
  fn screen_size() -> (f64, f64);

  /// Get current mouse position.
  fn mouse_position() -> crate::types::Point;

  /// Get the root element handle for a window.
  fn window_handle(window: &AXWindow) -> Option<Self::Handle>;

  /// Create an observer for a process.
  fn create_observer(pid: u32, axio: Axio) -> AxioResult<Self::Observer>;

  /// Start a display-linked callback (vsync).
  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle>;

  /// Enable accessibility for a process (mostly for Chromium/Electron apps).
  fn enable_accessibility_for_pid(pid: u32);

  /// Get the currently focused element for an app.
  fn focused_element(pid: u32) -> Option<Self::Handle>;

  /// Get the application element for a process (used for hit testing within a specific app).
  fn app_element(pid: u32) -> Self::Handle;
}

/// Per-element operations.
/// This is the handle that core code holds onto.
pub(crate) trait PlatformHandle: Clone + Send + Sync + 'static {
  /// Get child element handles.
  fn children(&self) -> Vec<Self>;

  /// Get parent element handle.
  fn parent(&self) -> Option<Self>;

  /// Get a unique hash for this element (for deduplication).
  fn element_hash(&self) -> u64;

  /// Set a value on this element.
  fn set_value(&self, value: &Value) -> AxioResult<()>;

  /// Perform an action on this element.
  fn perform_action(&self, action: &str) -> AxioResult<()>;

  /// Refresh and return current attributes.
  fn get_attributes(&self) -> ElementAttributes;

  /// Get element at position (for drilling down through nested elements).
  /// Used by core hit testing to recursively drill into containers.
  #[allow(dead_code)] // Used through concrete type (trait bound satisfied)
  fn element_at_position(&self, x: f64, y: f64) -> Option<Self>;

  /// Get selected text and optional range from this element.
  fn get_selection(&self) -> Option<(String, Option<(u32, u32)>)>;
}

/// Observer for element notifications.
/// Handles subscription lifecycle via WatchHandle RAII.
pub(crate) trait PlatformObserver: Send + Sync {
  type Handle: PlatformHandle;

  /// Subscribe to app-level notifications (focus, selection).
  fn subscribe_app_notifications(&self, pid: u32, axio: Axio) -> AxioResult<()>;

  /// Watch for element destruction.
  /// Returns a WatchHandle that unsubscribes when dropped.
  fn watch_destruction(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    axio: Axio,
  ) -> AxioResult<WatchHandle>;

  /// Watch an element for notifications.
  /// Returns a WatchHandle that unsubscribes when dropped.
  fn watch_element(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    notifications: &[Notification],
    axio: Axio,
  ) -> AxioResult<WatchHandle>;
}

/// Opaque handle to a notification subscription.
/// Unsubscribes automatically when dropped (RAII).
pub(crate) struct WatchHandle {
  /// Platform-specific implementation (cleanup happens via Drop).
  #[cfg(target_os = "macos")]
  #[allow(dead_code)] // Field exists for its Drop impl
  pub(crate) inner: super::macos::WatchHandleInner,
}

// Send + Sync are safe because WatchHandleInner manages its own thread safety
unsafe impl Send for WatchHandle {}
unsafe impl Sync for WatchHandle {}

/// Handle to a display link (vsync callback).
/// Stops the display link when dropped.
pub(crate) struct DisplayLinkHandle {
  #[cfg(target_os = "macos")]
  pub(crate) inner: super::macos::MacOSDisplayLinkHandle,
}

impl DisplayLinkHandle {
  pub(crate) fn stop(&self) {
    #[cfg(target_os = "macos")]
    self.inner.stop();
  }
}
