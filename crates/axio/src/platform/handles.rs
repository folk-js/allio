//! Opaque platform handles.

#[cfg(target_os = "macos")]
mod macos_impl {
  use accessibility::AXUIElement;
  use accessibility_sys::AXObserverRef;

  /// Opaque handle to a UI element.
  ///
  /// On macOS this wraps an AXUIElement reference.
  /// Clone is cheap (reference counted).
  #[derive(Clone)]
  pub struct ElementHandle(pub(in crate::platform) AXUIElement);

  impl ElementHandle {
    pub(in crate::platform) fn new(element: AXUIElement) -> Self {
      Self(element)
    }

    pub(in crate::platform) fn inner(&self) -> &AXUIElement {
      &self.0
    }
  }

  // SAFETY: AXUIElement is a CFTypeRef (reference-counted, immutable).
  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}

  /// Opaque handle to an observer (for watching element changes).
  ///
  /// On macOS this wraps an AXObserverRef.
  #[derive(Clone, Copy)]
  pub struct ObserverHandle(pub(in crate::platform) AXObserverRef);

  impl ObserverHandle {
    pub(in crate::platform) fn new(observer: AXObserverRef) -> Self {
      Self(observer)
    }

    pub(in crate::platform) fn inner(&self) -> AXObserverRef {
      self.0
    }
  }

  // SAFETY: AXObserverRef is a CFTypeRef, access is synchronized.
  unsafe impl Send for ObserverHandle {}
  unsafe impl Sync for ObserverHandle {}
}

#[cfg(target_os = "macos")]
pub use macos_impl::*;

// =============================================================================
// Windows Implementation (Future)
// =============================================================================

#[cfg(target_os = "windows")]
mod windows_impl {
  /// Opaque handle to a UI element.
  /// On Windows this would wrap IUIAutomationElement.
  #[derive(Clone)]
  pub struct ElementHandle {
    // Would contain: IUIAutomationElement or similar
    _placeholder: (),
  }

  /// Opaque handle to an observer.
  /// On Windows this would wrap IUIAutomationEventHandler.
  #[derive(Clone, Copy)]
  pub struct ObserverHandle {
    _placeholder: (),
  }

  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}
  unsafe impl Send for ObserverHandle {}
  unsafe impl Sync for ObserverHandle {}
}

#[cfg(target_os = "windows")]
pub use windows_impl::*;

// =============================================================================
// Linux Implementation (Future)
// =============================================================================

#[cfg(target_os = "linux")]
mod linux_impl {
  /// Opaque handle to a UI element.
  /// On Linux this would wrap an AT-SPI Accessible object.
  #[derive(Clone)]
  pub struct ElementHandle {
    // Would contain: atspi::Accessible or similar
    _placeholder: (),
  }

  /// Opaque handle to an observer.
  /// On Linux this would wrap AT-SPI event subscriptions.
  #[derive(Clone, Copy)]
  pub struct ObserverHandle {
    _placeholder: (),
  }

  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}
  unsafe impl Send for ObserverHandle {}
  unsafe impl Sync for ObserverHandle {}
}

#[cfg(target_os = "linux")]
pub use linux_impl::*;

// =============================================================================
// Fallback for unsupported platforms
// =============================================================================

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod fallback_impl {
  #[derive(Clone)]
  pub struct ElementHandle(());

  #[derive(Clone, Copy)]
  pub struct ObserverHandle(());

  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}
  unsafe impl Send for ObserverHandle {}
  unsafe impl Sync for ObserverHandle {}
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use fallback_impl::*;
