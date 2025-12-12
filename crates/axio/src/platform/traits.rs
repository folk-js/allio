/*!
Platform abstraction traits.

These traits define the contract between core code and platform implementations.
Platform-specific code (e.g., macOS) implements these traits.
Core code only uses these traits - never platform-specific types directly.
*/

#![allow(unsafe_code)]

use std::hash::Hash;
use std::sync::Arc;

use crate::accessibility::{Notification, Value};
use crate::types::{AxioResult, ElementId, Window};

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
/// This is the cross-platform interface for element data.
///
/// All platform-specific details (like raw role strings) are converted
/// by the platform layer before being returned here.
#[derive(Debug, Default)]
pub(crate) struct ElementAttributes {
  /// Semantic role (mapped from platform-specific role).
  pub role: crate::accessibility::Role,
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
  pub actions: Vec<crate::accessibility::Action>,
}

// ============================================================================
// Callback Trait
// ============================================================================

/// Callbacks from Platform to Core when OS events fire.
///
/// This trait defines the ONLY interface Platform uses to communicate
/// back to Core. Axio implements this trait. This decouples Platform
/// from the full Axio API.
///
/// All methods take `&self` - implementations should use interior mutability.
pub(crate) trait PlatformCallbacks: Send + Sync + 'static {
  /// The handle type for this platform.
  type Handle: PlatformHandle;

  /// Called when a platform event occurs.
  ///
  /// Events include element changes (for tracked elements by ID) and
  /// handle-based events (for potentially new elements like focus changes).
  ///
  /// For handle-based events, use `handle.pid()` to get the process ID.
  fn on_element_event(&self, event: ElementEvent<Self::Handle>);
}

// ============================================================================
// Platform Traits
// ============================================================================

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
  fn has_permissions() -> bool;

  /// Fetch all visible windows from the window server.
  /// Returns windows from all apps (filtering is done in core).
  fn fetch_windows(exclude_pid: Option<u32>) -> Vec<Window>;

  /// Fetch main screen dimensions (width, height) in points.
  fn fetch_screen_size() -> (f64, f64);

  /// Fetch current mouse position in screen coordinates.
  fn fetch_mouse_position() -> crate::types::Point;

  /// Fetch the accessibility element handle for a window from OS.
  /// This makes platform calls to enumerate window elements and match by bounds.
  /// Returns None if the window has no accessibility element.
  fn fetch_window_handle(window: &Window) -> Option<Self::Handle>;

  /// Create a notification observer for a process.
  /// On macOS: creates an `AXObserver` and adds it to the run loop.
  ///
  /// Takes a callbacks trait instead of Axio directly to decouple Platform
  /// from the full Axio API.
  fn create_observer<C: PlatformCallbacks<Handle = Self::Handle>>(
    pid: u32,
    callbacks: Arc<C>,
  ) -> AxioResult<Self::Observer>;

  /// Start a display-linked callback (vsync-synchronized).
  /// Returns None if display link is not available.
  fn start_display_link<F: Fn() + Send + Sync + 'static>(callback: F) -> Option<DisplayLinkHandle>;

  /// Enable accessibility for apps that require explicit activation.
  /// On macOS: sets `AXManualAccessibility` for Chromium/Electron apps.
  fn enable_accessibility_for_pid(pid: u32);

  /// Fetch the currently focused element within an app.
  /// On macOS: queries `AXFocusedUIElement` on the app element.
  fn fetch_focused_element(app_handle: &Self::Handle) -> Option<Self::Handle>;

  /// Get the root application element for a process.
  /// Called once when process is registered, stored in ProcessEntry.
  /// On macOS: calls `AXUIElementCreateApplication(pid)`.
  fn app_element(pid: u32) -> Self::Handle;
}

/// Per-element operations.
///
/// This is the opaque handle that core code holds onto.
/// Clone is cheap (reference-counted on macOS).
/// All methods hit the OS (no caching) - that's why they use `fetch_` prefix.
///
/// ## Identity Semantics
///
/// Handles implement `Hash` and `Eq` for use as HashMap keys:
/// - `Hash`: Returns a stable hash (cached from platform, e.g., CFHash on macOS)
/// - `Eq`: Compares by identity, not pointer (e.g., CFEqual on macOS)
///
/// This gives O(1) lookups with correct collision resolution.
///
/// ## Cached Properties
///
/// - `pid()`: Process ID is cached at construction (no FFI in hot path)
pub(crate) trait PlatformHandle: Clone + Send + Sync + Hash + Eq + 'static {
  /// Get the process ID this element belongs to.
  ///
  /// This is cached at construction - no FFI call in hot path.
  fn pid(&self) -> u32;

  /// Fetch child element handles from OS. Returns empty vec if no children.
  fn fetch_children(&self) -> Vec<Self>;

  /// Fetch parent element handle from OS. Returns None for root elements.
  fn fetch_parent(&self) -> Option<Self>;

  /// Set a typed value on this element (string, number, or boolean).
  /// Fails if element doesn't support value setting.
  fn set_value(&self, value: &Value) -> AxioResult<()>;

  /// Perform a named action on this element.
  /// On macOS: action names like "AXPress", "AXShowMenu", etc.
  fn perform_action(&self, action: &str) -> AxioResult<()>;

  /// Fetch current attributes from the platform (not cached).
  fn fetch_attributes(&self) -> ElementAttributes;

  /// Fetch element at position within this element's coordinate space.
  /// Used for drilling down through nested containers.
  fn fetch_element_at_position(&self, x: f64, y: f64) -> Option<Self>;

  /// Fetch selected text and optional range (start, end) for text elements.
  /// Returns None if element has no selection or isn't a text element.
  fn fetch_selection(&self) -> Option<(String, Option<(u32, u32)>)>;

  /// Fetch the containing window element.
  /// Returns None for elements that aren't in a window (menu bar, system tray, etc.).
  /// On macOS: queries `AXWindow` attribute.
  fn window(&self) -> Option<Self>;
}

/// Observer for element notifications.
///
/// One observer exists per process. Handles subscription lifecycle via
/// WatchHandle RAII - dropping the handle automatically unsubscribes.
pub(crate) trait PlatformObserver: Send + Sync {
  /// Element handle type (must match Platform::Handle).
  type Handle: PlatformHandle;

  /// Subscribe to app-level focus and selection notifications.
  /// Called once when process is registered. Returns a handle that
  /// cleans up subscriptions when dropped.
  fn subscribe_app_notifications<C: PlatformCallbacks<Handle = Self::Handle>>(
    &self,
    pid: u32,
    callbacks: Arc<C>,
  ) -> AxioResult<AppNotificationHandle>;

  /// Create a watch handle for an element with initial notifications.
  /// The handle manages subscriptions - use add/remove to modify.
  /// Drop handle to unsubscribe from all.
  fn create_watch<C: PlatformCallbacks<Handle = Self::Handle>>(
    &self,
    handle: &Self::Handle,
    element_id: ElementId,
    initial_notifications: &[Notification],
    callbacks: Arc<C>,
  ) -> AxioResult<WatchHandle>;
}

// TODO: move these behind the platform boundary (once we figure out more for other OSs):

/// Handle to app-level notification subscriptions.
/// Cleans up the observer context when dropped.
pub(crate) struct AppNotificationHandle {
  #[cfg(target_os = "macos")]
  pub(crate) _inner: super::macos::AppNotificationHandleInner,
}

// Send + Sync are safe because the inner type manages its own thread safety
unsafe impl Send for AppNotificationHandle {}
unsafe impl Sync for AppNotificationHandle {}

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
