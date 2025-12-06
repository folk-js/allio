//! Opaque platform handles.

#[cfg(target_os = "macos")]
mod macos_impl {
  use objc2_application_services::{AXObserver, AXUIElement};
  use objc2_core_foundation::CFRetained;

  /// Opaque handle to a UI element.
  ///
  /// On macOS this wraps an AXUIElement reference.
  /// Clone is cheap (reference counted via CFRetained).
  #[derive(Clone)]
  pub struct ElementHandle(pub(in crate::platform) CFRetained<AXUIElement>);

  impl ElementHandle {
    pub(in crate::platform) fn new(element: CFRetained<AXUIElement>) -> Self {
      Self(element)
    }

    pub(in crate::platform) fn inner(&self) -> &AXUIElement {
      &self.0
    }

    pub(in crate::platform) fn retained(&self) -> CFRetained<AXUIElement> {
      self.0.clone()
    }
  }

  // SAFETY: CFRetained<AXUIElement> is reference-counted and thread-safe.
  unsafe impl Send for ElementHandle {}
  unsafe impl Sync for ElementHandle {}

  /// Opaque handle to an observer (for watching element changes).
  ///
  /// On macOS this wraps an AXObserver.
  #[derive(Clone)]
  pub struct ObserverHandle(pub(in crate::platform) CFRetained<AXObserver>);

  impl ObserverHandle {
    pub(in crate::platform) fn new(observer: CFRetained<AXObserver>) -> Self {
      Self(observer)
    }

    pub(in crate::platform) fn inner(&self) -> &AXObserver {
      &self.0
    }
  }

  // SAFETY: CFRetained<AXObserver> is reference-counted and thread-safe.
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
  #[derive(Clone)]
  pub struct ElementHandle {
    _placeholder: (),
  }

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
  #[derive(Clone)]
  pub struct ElementHandle {
    _placeholder: (),
  }

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
