#![deny(unused_imports)]

use std::ffi::c_void;

use crate::common::{
  api::{empty_entity, Api},
  result::Result,
  x_win_struct::window_info::WindowInfo,
};
use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::{
  CFArray, CFBoolean, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CGRect,
};
use objc2_core_graphics::{
  kCGNullWindowID, CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo,
  CGWindowListOption,
};

pub struct MacosAPI {}

impl Api for MacosAPI {
  fn get_active_window(&self) -> Result<WindowInfo> {
    let windows: Vec<WindowInfo> = get_windows_informations(true)?;
    let active_window = if !windows.is_empty() {
      windows.first().cloned().unwrap_or_else(empty_entity)
    } else {
      empty_entity()
    };
    Ok(active_window)
  }

  fn get_open_windows(&self) -> Result<Vec<WindowInfo>> {
    get_windows_informations(false)
  }
}

fn get_windows_informations(only_active: bool) -> Result<Vec<WindowInfo>> {
  // IMPORTANT: Wrap in autorelease pool to prevent memory leaks.
  // Objective-C methods like runningApplicationWithProcessIdentifier: return
  // autoreleased objects that accumulate without a pool drain.
  objc2::rc::autoreleasepool(|_pool| get_windows_informations_inner(only_active))
}

fn get_windows_informations_inner(only_active: bool) -> Result<Vec<WindowInfo>> {
  let mut windows: Vec<WindowInfo> = Vec::new();

  let option = CGWindowListOption::OptionOnScreenOnly
    | CGWindowListOption::ExcludeDesktopElements
    | CGWindowListOption::OptionIncludingWindow;
  let window_list_info: &CFArray = &CGWindowListCopyWindowInfo(option, kCGNullWindowID).unwrap();
  let windows_count = CFArray::count(window_list_info);

  for idx in 0..windows_count {
    let window_cf_dictionary_ref =
      unsafe { CFArray::value_at_index(window_list_info, idx) as *const CFDictionary };

    if window_cf_dictionary_ref.is_null() {
      continue;
    }
    let window_cf_dictionary =
      unsafe { CFRetained::retain(std::ptr::NonNull::from(&*window_cf_dictionary_ref)) };
    let is_screen: bool = get_cf_boolean_value(&window_cf_dictionary, "kCGWindowIsOnscreen");
    if !is_screen {
      continue;
    }

    let window_layer = get_cf_number_value(&window_cf_dictionary, "kCGWindowLayer");

    if window_layer < 0 || window_layer > 100 {
      continue;
    }

    let bounds = match get_cf_window_bounds_value(&window_cf_dictionary) {
      Some(bounds) => bounds,
      None => continue,
    };

    if bounds.size.height < 50.0 || bounds.size.width < 50.0 {
      continue;
    }

    let process_id = get_cf_number_value(&window_cf_dictionary, "kCGWindowOwnerPID");
    if process_id == 0 {
      continue;
    }

    let app = match get_running_application_from_pid(process_id as u32) {
      Ok(app) => app,
      Err(_) => continue,
    };

    let is_not_active = !app.isActive();

    if only_active && is_not_active {
      continue;
    }

    let bundle_identifier = get_bundle_identifier(app);

    if bundle_identifier.eq("com.apple.dock") {
      continue;
    }

    let app_name = get_cf_string_value(&window_cf_dictionary, "kCGWindowOwnerName");
    let title = get_cf_string_value(&window_cf_dictionary, "kCGWindowName");
    let id = get_cf_number_value(&window_cf_dictionary, "kCGWindowNumber");

    windows.push(WindowInfo {
      id: id as u32,
      title,
      process_id: process_id as u32,
      process_name: app_name,
      x: bounds.origin.x as i32,
      y: bounds.origin.y as i32,
      width: bounds.size.width as i32,
      height: bounds.size.height as i32,
    });

    if only_active && is_not_active {
      break;
    }
  }

  Ok(windows)
}

fn get_bundle_identifier(app: &NSRunningApplication) -> String {
  match app.bundleIdentifier() {
    Some(bundle_identifier) => bundle_identifier.to_string(),
    None => String::from(""),
  }
}

fn get_cf_dictionary_get_value<T>(dict: &CFDictionary, key: &str) -> Option<*const T> {
  let key = CFString::from_str(key);
  let key_ref = key.as_ref() as *const CFString;
  if unsafe { CFDictionary::contains_ptr_key(dict, key_ref.cast()) } {
    let value = unsafe { CFDictionary::value(dict, key_ref.cast()) };
    Some(value as *const T)
  } else {
    None
  }
}

fn get_cf_number_value(dict: &CFDictionary, key: &str) -> i32 {
  unsafe {
    let mut value: i32 = 0;
    match get_cf_dictionary_get_value::<CFNumber>(dict, key) {
      Some(number) => {
        CFNumber::value(
          &*number,
          CFNumberType::IntType,
          &mut value as *mut _ as *mut c_void,
        );
        value
      }
      None => value,
    }
  }
}

fn get_cf_boolean_value(dict: &CFDictionary, key: &str) -> bool {
  unsafe {
    match get_cf_dictionary_get_value::<CFBoolean>(dict, key) {
      Some(value) => CFBoolean::value(&*value),
      None => false,
    }
  }
}

fn get_cf_window_bounds_value(dict: &CFDictionary) -> Option<CGRect> {
  match get_cf_dictionary_get_value::<CFDictionary>(dict, "kCGWindowBounds") {
    Some(dict_react) => unsafe {
      let mut cg_rect = CGRect::default();
      if !dict_react.is_null()
        && CGRectMakeWithDictionaryRepresentation(Some(&*dict_react), &mut cg_rect)
      {
        Some(cg_rect)
      } else {
        None
      }
    },
    None => None,
  }
}

fn get_cf_string_value(dict: &CFDictionary, key: &str) -> String {
  unsafe {
    match get_cf_dictionary_get_value::<CFString>(dict, key) {
      Some(value) => (*value).to_string(),
      None => String::from(""),
    }
  }
}

fn get_running_application_from_pid(process_id: u32) -> Result<&'static NSRunningApplication> {
  let process_id = process_id as i64;
  let app: *mut NSRunningApplication = unsafe {
    objc2::msg_send![
      objc2::class!(NSRunningApplication),
      runningApplicationWithProcessIdentifier: process_id as i32
    ]
  };
  if app.is_null() {
    Err(String::from("Application not found with pid").into())
  } else {
    Ok(unsafe { &*app })
  }
}
