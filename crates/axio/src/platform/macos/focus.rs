/*!
Focus and selection utilities for macOS accessibility.

Provides helper functions for extracting selection data from element handles.
*/

#![allow(unsafe_code)]
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use super::handles::ElementHandle;

/// Get selected text and range from an element handle.
pub(super) fn get_selection_from_handle(
  handle: &ElementHandle,
) -> Option<(String, Option<(u32, u32)>)> {
  let selected_text = handle.get_string("AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  let range = get_selected_text_range(handle);
  Some((selected_text, range))
}

/// Get the selected text range from an element handle.
fn get_selected_text_range(handle: &ElementHandle) -> Option<(u32, u32)> {
  use objc2_application_services::{AXValue as AXValueRef, AXValueType};
  use objc2_core_foundation::{CFRange, CFString};
  use std::ffi::c_void;
  use std::ptr::NonNull;

  let attr_name = CFString::from_static_str("AXSelectedTextRange");
  let value = handle.get_raw_attr_internal(&attr_name)?;

  let ax_value = value.downcast_ref::<AXValueRef>()?;

  unsafe {
    let mut range = CFRange {
      location: 0,
      length: 0,
    };
    if ax_value.value(
      AXValueType::CFRange,
      NonNull::new((&raw mut range).cast::<c_void>())?,
    ) {
      let start = range.location as u32;
      let end = (range.location + range.length) as u32;
      Some((start, end))
    } else {
      None
    }
  }
}
