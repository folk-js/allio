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
///
/// These are static methods that don't require an element handle.
/// Implementations live in `platform/{os}/mod.rs`.
pub(crate) trait Platform {
  /// Element handle type for this platform.
  type Handle: PlatformHandle;
  /// Observer type for this platform.
  type Observer: PlatformObserver<Handle = Self::Handle>;

  /// Check if accessibility permissions are granted.
  /// On macOS: calls `AXIsProcessTrusted()`.
  fn check_permissions() -> bool;

  /// Fetch all visible windows from the window server.
  /// Returns windows from all apps (filtering is done in core).
  fn fetch_windows(exclude_pid: Option<u32>) -> Vec<AXWindow>;

  /// Get main screen dimensions (width, height) in points.
  fn screen_size() -> (f64, f64);

  /// Get current mouse position in screen coordinates.
  fn mouse_position() -> crate::types::Point;

  /// Get the accessibility element handle for a window.
  /// Returns None if the window has no accessibility element.
  fn window_handle(window: &AXWindow) -> Option<Self::Handle>;

  /// Create a notification observer for a process.
  /// On macOS: creates an `AXObserver` and adds it to the run loop.
  fn create_observer(pid: u32, axio: Axio) -> AxioResult<Self::Observer>;

  /// Start a display-linked callback (vsync-synchronized).
  /// Returns None if display link is not available.
  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle>;

  /// Enable accessibility for apps that require explicit activation.
  /// On macOS: sets `AXManualAccessibility` for Chromium/Electron apps.
  fn enable_accessibility_for_pid(pid: u32);

  /// Get the currently focused element within an app.
  /// On macOS: queries `AXFocusedUIElement` on the app element.
  fn focused_element(app_handle: &Self::Handle) -> Option<Self::Handle>;

  /// Get the root application element for a process.
  /// Called once when process is registered, stored in ProcessState.
  /// On macOS: calls `AXUIElementCreateApplication(pid)`.
  fn app_element(pid: u32) -> Self::Handle;
}

/// Per-element operations.
///
/// This is the opaque handle that core code holds onto.
/// Clone is cheap (reference-counted on macOS).
pub(crate) trait PlatformHandle: Clone + Send + Sync + 'static {
  /// Get child element handles. Returns empty vec if no children.
  fn children(&self) -> Vec<Self>;

  /// Get parent element handle. Returns None for root elements.
  fn parent(&self) -> Option<Self>;

  /// Get a unique hash for this element (for deduplication).
  /// On macOS: uses `CFHash()` on the underlying `AXUIElement`.
  fn element_hash(&self) -> u64;

  /// Set a typed value on this element (string, number, or boolean).
  /// Fails if element doesn't support value setting.
  fn set_value(&self, value: &Value) -> AxioResult<()>;

  /// Perform a named action on this element.
  /// On macOS: action names like "AXPress", "AXShowMenu", etc.
  fn perform_action(&self, action: &str) -> AxioResult<()>;

  /// Fetch current attributes from the platform (not cached).
  fn get_attributes(&self) -> ElementAttributes;

  /// Hit test within this element's coordinate space.
  /// Used for drilling down through nested containers.
  #[allow(dead_code)] // Used through concrete type (trait bound satisfied)
  fn element_at_position(&self, x: f64, y: f64) -> Option<Self>;

  /// Get selected text and optional range (start, end) for text elements.
  /// Returns None if element has no selection or isn't a text element.
  fn get_selection(&self) -> Option<(String, Option<(u32, u32)>)>;
}

/// Observer for element notifications.
///
/// One observer exists per process. Handles subscription lifecycle via
/// WatchHandle RAII - dropping the handle automatically unsubscribes.
pub(crate) trait PlatformObserver: Send + Sync {
  /// Element handle type (must match Platform::Handle).
  type Handle: PlatformHandle;

  /// Subscribe to app-level focus and selection notifications.
  /// Called once when process is registered. Lives for process lifetime.
  fn subscribe_app_notifications(&self, pid: u32, axio: Axio) -> AxioResult<()>;

  /// Create a watch handle for an element with initial notifications.
  /// The handle manages subscriptions - use add/remove to modify.
  /// Drop handle to unsubscribe from all.
  fn create_watch(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    initial_notifications: &[Notification],
    axio: Axio,
  ) -> AxioResult<WatchHandle>;
}

/// Opaque handle to a notification subscription.
/// Manages a set of notifications for an element.
/// Unsubscribes automatically when dropped (RAII).
pub(crate) struct WatchHandle {
  #[cfg(target_os = "macos")]
  pub(crate) inner: super::macos::WatchHandleInner,
}

impl WatchHandle {
  /// Add notifications to the watch set.
  /// Returns number of newly subscribed notifications.
  pub(crate) fn add(&mut self, notifs: &[Notification]) -> usize {
    #[cfg(target_os = "macos")]
    {
      self.inner.add(notifs)
    }
  }

  /// Remove notifications from the watch set.
  pub(crate) fn remove(&mut self, notifs: &[Notification]) {
    #[cfg(target_os = "macos")]
    self.inner.remove(notifs);
  }
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
