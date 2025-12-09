/*! Core Foundation utilities for macOS.

Clean, type-safe wrappers around CF types for dictionary access,
number/string/boolean extraction, and window bounds parsing.
*/

#![allow(unsafe_code)]

use objc2_core_foundation::{
  CFBoolean, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CGRect,
};
use objc2_core_graphics::CGRectMakeWithDictionaryRepresentation;
use std::ffi::c_void;

/// Safely get a value from a `CFDictionary` by key.
fn get_cf_dictionary_value<T>(dict: &CFDictionary, key: &str) -> Option<*const T> {
  let key = CFString::from_str(key);
  let key_ref = key.as_ref() as *const CFString;
  if unsafe { CFDictionary::contains_ptr_key(dict, key_ref.cast()) } {
    let value = unsafe { CFDictionary::value(dict, key_ref.cast()) };
    Some(value.cast::<T>())
  } else {
    None
  }
}

/// Extract an i32 number from a `CFDictionary`.
pub(super) fn get_cf_number(dict: &CFDictionary, key: &str) -> i32 {
  unsafe {
    let mut value: i32 = 0;
    if let Some(number) = get_cf_dictionary_value::<CFNumber>(dict, key) {
      CFNumber::value(
        &*number,
        CFNumberType::IntType,
        (&raw mut value).cast::<c_void>(),
      );
    }
    value
  }
}

/// Extract a boolean from a `CFDictionary`.
pub(super) fn get_cf_boolean(dict: &CFDictionary, key: &str) -> bool {
  unsafe {
    match get_cf_dictionary_value::<CFBoolean>(dict, key) {
      Some(value) => CFBoolean::value(&*value),
      None => false,
    }
  }
}

/// Extract a string from a `CFDictionary`.
pub(super) fn get_cf_string(dict: &CFDictionary, key: &str) -> String {
  unsafe {
    match get_cf_dictionary_value::<CFString>(dict, key) {
      Some(value) => (*value).to_string(),
      None => String::new(),
    }
  }
}

/// Extract window bounds (`CGRect`) from a `CFDictionary`.
pub(super) fn get_cf_window_bounds(dict: &CFDictionary) -> Option<CGRect> {
  match get_cf_dictionary_value::<CFDictionary>(dict, "kCGWindowBounds") {
    Some(dict_rect) => unsafe {
      let mut cg_rect = CGRect::default();
      if !dict_rect.is_null()
        && CGRectMakeWithDictionaryRepresentation(Some(&*dict_rect), &raw mut cg_rect)
      {
        Some(cg_rect)
      } else {
        None
      }
    },
    None => None,
  }
}

/// Retain a `CFDictionary` from a raw pointer.
pub(super) fn retain_cf_dictionary(ptr: *const CFDictionary) -> Option<CFRetained<CFDictionary>> {
  if ptr.is_null() {
    None
  } else {
    Some(unsafe { CFRetained::retain(std::ptr::NonNull::from(&*ptr)) })
  }
}
