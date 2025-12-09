/*! Shared utilities for macOS accessibility. */

#![allow(unsafe_code)]
#![allow(clippy::cast_possible_wrap)]

use objc2_application_services::{AXIsProcessTrusted, AXUIElement};
use objc2_core_foundation::CFRetained;

/// Create an `AXUIElement` for an application by PID.
/// Encapsulates the unsafe FFI call.
pub(super) fn app_element(pid: u32) -> CFRetained<AXUIElement> {
  unsafe { AXUIElement::new_application(pid as i32) }
}

/// Check if accessibility permissions are granted.
/// Returns true if trusted, false otherwise.
pub(crate) fn check_accessibility_permissions() -> bool {
  unsafe { AXIsProcessTrusted() }
}
