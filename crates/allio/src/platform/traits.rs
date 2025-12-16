/*!
Platform abstraction traits.

These traits define the contract between core code and platform implementations.
Platform-specific code (e.g., macOS) implements these traits.
Core code only uses these traits - never platform-specific types directly.
*/

#![allow(unsafe_code)]

use std::hash::Hash;
use std::sync::Arc;

use crate::a11y::{Action, Notification, Value};
use crate::types::{AllioResult, ElementId, Window};

/// Event types from platform to core.
///
/// Generic over handle type to work with the trait system.
/// PID can be derived from handles via `handle.pid()`.
#[derive(Debug)]
pub(crate) enum ElementEvent<H> {
  /// Element was destroyed by the OS.
  Destroyed(ElementId),

  /// Element's value/title/bounds changed.
  Changed(ElementId, Notification),

  /// Element's children structure changed.
  ChildrenChanged(ElementId),

  /// App focus changed to this element.
  FocusChanged(H),

  /// Text selection changed.
  SelectionChanged {
    handle: H,
    text: String,
    range: Option<(u32, u32)>,
  },
}

/// Attributes fetched from a platform element.
#[derive(Debug, Default, Clone)]
pub(crate) struct ElementAttributes {
  /// Semantic role (mapped from platform-specific role).
  pub role: crate::a11y::Role,
  /// Platform-specific role string for debugging (e.g., "AXButton/AXMenuItem").
  pub platform_role: String,
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
  pub actions: Vec<crate::a11y::Action>,
  /// Platform accessibility identifier (AXIdentifier on macOS).
  /// May provide stable identity across element moves if the app sets it.
  pub identifier: Option<String>,
}

/// Callbacks from platform to core when OS events fire.
pub(crate) trait EventHandler: Send + Sync + 'static {
  /// The handle type for this platform.
  type Handle: PlatformHandle;

  /// Called when a platform event occurs.
  fn on_element_event(&self, event: ElementEvent<Self::Handle>);
}

/// Platform-global operations.
pub(crate) trait Platform {
  /// Element handle type for this platform.
  type Handle: PlatformHandle;
  /// Observer type for this platform.
  type Observer: PlatformObserver<Handle = Self::Handle>;

  /// Check if accessibility permissions are granted.
  fn has_permissions() -> bool;

  /// Fetch all visible windows from the window server.
  fn fetch_windows(exclude_pid: Option<u32>) -> Vec<Window>;

  /// Fetch main screen dimensions (width, height) in points.
  fn fetch_screen_size() -> (f64, f64);

  /// Fetch current mouse position in screen coordinates.
  fn fetch_mouse_position() -> crate::types::Point;

  /// Fetch the accessibility element handle for a window.
  fn fetch_window_handle(window: &Window) -> Option<Self::Handle>;

  /// Create a notification observer for a process.
  fn create_observer<C: EventHandler<Handle = Self::Handle>>(
    pid: u32,
    callbacks: Arc<C>,
  ) -> AllioResult<Self::Observer>;

  /// Start a display-linked callback (vsync-synchronized).
  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle>;

  /// Enable accessibility for apps that require explicit activation (Chromium/Electron).
  fn enable_accessibility_for_pid(pid: u32);

  /// Get the root application element for a process.
  fn app_element(pid: u32) -> Self::Handle;
}

/// Per-element operations. Clone is cheap (reference-counted).
pub(crate) trait PlatformHandle: Clone + Send + Sync + Hash + Eq + 'static {
  /// Get the process ID (cached at construction).
  fn pid(&self) -> u32;

  /// Fetch child element handles from OS. Returns empty vec if no children.
  fn fetch_children(&self) -> Vec<Self>;

  /// Fetch parent element handle from OS. Returns None for root elements.
  fn fetch_parent(&self) -> Option<Self>;

  /// Set a typed value on this element.
  fn set_value(&self, value: &Value) -> AllioResult<()>;

  /// Perform an action on this element.
  fn perform_action(&self, action: Action) -> AllioResult<()>;

  /// Fetch current attributes from the platform.
  fn fetch_attributes(&self) -> ElementAttributes;

  /// Fetch element at position within this element's coordinate space.
  fn fetch_element_at_position(&self, x: f64, y: f64) -> Option<Self>;

  /// Fetch the containing window element.
  fn window(&self) -> Option<Self>;
}

/// Observer for element notifications. One observer per process.
pub(crate) trait PlatformObserver: Send + Sync {
  type Handle: PlatformHandle;

  /// Subscribe to app-level focus and selection notifications.
  fn subscribe_app_notifications<C: EventHandler<Handle = Self::Handle>>(
    &self,
    pid: u32,
    callbacks: Arc<C>,
  ) -> AllioResult<AppNotificationHandle>;

  /// Create a watch handle for an element with initial notifications.
  fn create_watch<C: EventHandler<Handle = Self::Handle>>(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    initial_notifications: &[Notification],
    callbacks: Arc<C>,
  ) -> AllioResult<WatchHandle>;
}

/// Handle to app-level notification subscriptions. Cleans up on drop.
pub(crate) struct AppNotificationHandle {
  #[cfg(target_os = "macos")]
  pub(crate) _inner: super::macos::AppNotificationHandleInner,
}

unsafe impl Send for AppNotificationHandle {}
unsafe impl Sync for AppNotificationHandle {}

/// Handle to notification subscriptions for an element. Unsubscribes on drop.
pub(crate) struct WatchHandle {
  #[cfg(target_os = "macos")]
  pub(crate) inner: super::macos::WatchHandleInner,
}

impl WatchHandle {
  /// Add notifications to the watch set.
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

unsafe impl Send for WatchHandle {}
unsafe impl Sync for WatchHandle {}

/// Handle to a display link (vsync callback). Stops on drop.
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
