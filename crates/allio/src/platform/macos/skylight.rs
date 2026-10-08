/*! Spaces, from `SkyLight` (the window server's private framework).

Everything here is looked up at runtime, so a macOS without one of these functions simply has no
Spaces (an empty list, windows on no Space) rather than failing to load. Only reads, plus putting
*our own* windows on a Space: moving other apps' windows between Spaces needs SIP off.
*/

#![allow(unsafe_code)]
#![allow(
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::cast_possible_wrap,
  clippy::missing_transmute_annotations // each target type is spelled out in `Api`
)] // Space ids are small positive numbers, carried as `CFNumber`s

use crate::types::{Space, SpaceId, SpaceKind};
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString};
use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

extern "C" {
  fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
  fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_NOW: i32 = 2;

type Cid = i32;

struct Api {
  connection: Cid,
  copy_managed_display_spaces: unsafe extern "C" fn(Cid) -> *const CFArray,
  copy_spaces_for_windows: unsafe extern "C" fn(Cid, i32, *const CFArray) -> *const CFArray,
  add_windows_to_spaces: unsafe extern "C" fn(Cid, *const CFArray, *const CFArray),
  remove_windows_from_spaces: unsafe extern "C" fn(Cid, *const CFArray, *const CFArray),
}

// The function pointers and the connection id are plain data, usable from any thread.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn api() -> Option<&'static Api> {
  static API: OnceLock<Option<Api>> = OnceLock::new();
  API
    .get_or_init(|| unsafe {
      let lib = dlopen(
        c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
        RTLD_NOW,
      );
      if lib.is_null() {
        log::warn!("SkyLight is missing: no Spaces");
        return None;
      }
      let find = |name: &CStr| {
        let f = dlsym(lib, name.as_ptr());
        if f.is_null() {
          log::warn!("SkyLight has no {}: no Spaces", name.to_string_lossy());
        }
        (!f.is_null()).then_some(f)
      };
      let main_connection: unsafe extern "C" fn() -> Cid =
        std::mem::transmute(find(c"SLSMainConnectionID")?);
      Some(Api {
        connection: main_connection(),
        copy_managed_display_spaces: std::mem::transmute(find(c"SLSCopyManagedDisplaySpaces")?),
        copy_spaces_for_windows: std::mem::transmute(find(c"SLSCopySpacesForWindows")?),
        add_windows_to_spaces: std::mem::transmute(find(c"SLSAddWindowsToSpaces")?),
        remove_windows_from_spaces: std::mem::transmute(find(c"SLSRemoveWindowsFromSpaces")?),
      })
    })
    .as_ref()
}

// --- Core Foundation plumbing ---

unsafe fn owned<T: objc2_core_foundation::Type>(ptr: *const T) -> Option<CFRetained<T>> {
  std::ptr::NonNull::new(ptr.cast_mut()).map(|p| CFRetained::from_raw(p))
}

unsafe fn value(dict: &CFDictionary, key: &str) -> *const c_void {
  let key = CFString::from_str(key);
  let key_ref: *const CFString = std::ptr::from_ref(&*key);
  if CFDictionary::contains_ptr_key(dict, key_ref.cast()) {
    CFDictionary::value(dict, key_ref.cast())
  } else {
    std::ptr::null()
  }
}

unsafe fn number(dict: &CFDictionary, key: &str) -> Option<i64> {
  let n = value(dict, key).cast::<CFNumber>();
  if n.is_null() {
    return None;
  }
  let mut out: i64 = 0;
  CFNumber::value(&*n, CFNumberType::SInt64Type, (&raw mut out).cast())
    .then_some(out)
}

unsafe fn string(dict: &CFDictionary, key: &str) -> Option<String> {
  let s = value(dict, key).cast::<CFString>();
  (!s.is_null()).then(|| (*s).to_string())
}

unsafe fn array_items<'a, T: 'a>(array: &'a CFArray) -> impl Iterator<Item = &'a T> + 'a {
  (0..array.count()).filter_map(move |i| array.value_at_index(i).cast::<T>().as_ref())
}

fn numbers(values: &[i64]) -> CFRetained<CFArray<CFNumber>> {
  let numbers: Vec<CFRetained<CFNumber>> = values.iter().map(|v| CFNumber::new_i64(*v)).collect();
  CFArray::from_retained_objects(&numbers)
}

/// An array as `SkyLight` takes it (untyped).
const fn untyped(array: &CFArray<CFNumber>) -> *const CFArray {
  std::ptr::from_ref(array).cast()
}

// --- Spaces ---

/// Every Space, display by display, in Mission Control's order.
pub(crate) fn spaces() -> Vec<Space> {
  let Some(api) = api() else {
    return Vec::new();
  };
  let mut out = Vec::new();
  unsafe {
    let Some(displays) = owned((api.copy_managed_display_spaces)(api.connection)) else {
      return out;
    };
    for display in array_items::<CFDictionary>(&displays) {
      let name = string(display, "Display Identifier").unwrap_or_default();
      let current = value(display, "Current Space").cast::<CFDictionary>();
      let current = current.as_ref().and_then(|c| number(c, "ManagedSpaceID"));
      let list = value(display, "Spaces").cast::<CFArray>();
      let Some(list) = list.as_ref() else { continue };
      for (index, space) in array_items::<CFDictionary>(list).enumerate() {
        let Some(id) = number(space, "ManagedSpaceID").or_else(|| number(space, "id64")) else {
          continue;
        };
        out.push(Space {
          id: SpaceId(id as u64),
          display: name.clone(),
          // 0 is a desktop, 4 a full-screen Space; anything else is treated as a desktop.
          kind: if number(space, "type") == Some(4) {
            SpaceKind::Fullscreen
          } else {
            SpaceKind::Desktop
          },
          index: index as u32,
          current: Some(id) == current,
        });
      }
    }
  }
  out
}

/// The Spaces a window is on (any app's window).
pub(crate) fn spaces_of_window(window: u32) -> Vec<SpaceId> {
  let Some(api) = api() else {
    return Vec::new();
  };
  unsafe {
    let windows = numbers(&[i64::from(window)]);
    // Mask 7: every kind of Space.
    let Some(spaces) = owned((api.copy_spaces_for_windows)(api.connection, 7, untyped(&windows)))
    else {
      return Vec::new();
    };
    array_items::<CFNumber>(&spaces)
      .filter_map(CFNumber::as_i64)
      .map(|id| SpaceId(id as u64))
      .collect()
  }
}

/// Puts one of *our own* windows on exactly `space`. Call it only once `AppKit` has ordered the
/// window in (after the run loop turn that ordered it): ordering in puts a window on the
/// current Space, undoing an earlier placement. Returns whether it is now there.
pub(crate) fn place_window_on_space(window: u32, space: SpaceId) -> bool {
  let Some(api) = api() else {
    return false;
  };
  let before = spaces_of_window(window);
  unsafe {
    let windows = numbers(&[i64::from(window)]);
    let target = numbers(&[space.0 as i64]);
    (api.add_windows_to_spaces)(api.connection, untyped(&windows), untyped(&target));
    let others: Vec<i64> = before.iter().filter(|s| **s != space).map(|s| s.0 as i64).collect();
    if !others.is_empty() {
      let others = numbers(&others);
      (api.remove_windows_from_spaces)(api.connection, untyped(&windows), untyped(&others));
    }
  }
  spaces_of_window(window) == [space]
}
