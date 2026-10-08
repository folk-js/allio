/*! Window enumeration for macOS.

Uses `CGWindowListCopyWindowInfo` to enumerate windows, and `SkyLight` for the Spaces they're on.
*/

#![allow(unsafe_code)]
#![allow(
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::cast_possible_wrap
)]

use super::cf_utils::{
  get_cf_boolean, get_cf_number, get_cf_string, get_cf_window_bounds, retain_cf_dictionary,
};
use super::skylight;
use crate::types::{Bounds, Presence, ProcessId, SpaceId, Window, WindowId};
use objc2::rc::Retained;
use objc2::ClassType;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use objc2_core_foundation::{CFArray, CFDictionary};
use objc2_core_graphics::{
  kCGNullWindowID, CGWindowListCopyWindowInfo, CGWindowListCreate, CGWindowListOption,
};

const FILTERED_BUNDLE_IDS: &[&str] = &[
  "com.apple.dock",
  "com.apple.screencaptureui",
  "com.apple.screenshot.launcher",
  "com.apple.ScreenContinuity",
];

/// Enumerate windows in z-order (frontmost first): every window that is on screen, then the
/// windows that aren't (on another Space, minimised, hidden), with where each one is.
pub(crate) fn enumerate_windows() -> Vec<Window> {
  objc2::rc::autoreleasepool(|_pool| enumerate_windows_inner())
}

/// How often every window's Spaces are re-read, besides when a window appears or comes on or off
/// screen. One window server call per window is too many for every poll.
const SPACES_REFRESH: Duration = Duration::from_millis(500);

struct SpacesCache {
  /// Per window: whether it was on screen, and its Spaces.
  windows: HashMap<u32, (bool, Vec<SpaceId>)>,
  refreshed: Instant,
}

fn spaces_cache() -> &'static Mutex<SpacesCache> {
  static CACHE: OnceLock<Mutex<SpacesCache>> = OnceLock::new();
  CACHE.get_or_init(|| {
    Mutex::new(SpacesCache {
      windows: HashMap::new(),
      refreshed: Instant::now(),
    })
  })
}

/// What enumeration needs to know about a window's app.
#[derive(Clone, Copy)]
struct AppInfo {
  /// An ordinary app, with a Dock icon.
  regular: bool,
  active: bool,
  hidden: bool,
}

impl AppInfo {
  /// None for processes that aren't running applications, or whose windows are left out.
  fn of(process_id: u32) -> Option<Self> {
    let app = get_running_application(process_id)?;
    if get_bundle_identifier(&app).is_some_and(|id| FILTERED_BUNDLE_IDS.contains(&id.as_str())) {
      return None;
    }
    Some(Self {
      regular: app.activationPolicy() == NSApplicationActivationPolicy::Regular,
      active: app.isActive(),
      hidden: app.isHidden(),
    })
  }
}

/// Where a window is, from whether it is on screen, its Spaces and whether its app is hidden.
const fn presence(on_screen: bool, spaces: &[SpaceId], app_hidden: bool) -> Presence {
  if on_screen {
    Presence::Here
  } else if !spaces.is_empty() {
    Presence::Elsewhere
  } else if app_hidden {
    Presence::Hidden
  } else {
    Presence::Minimised
  }
}

fn enumerate_windows_inner() -> Vec<Window> {
  let mut here = Vec::new();
  let mut elsewhere = Vec::new();
  // Track which PIDs we've already seen a window for (to mark only frontmost as focused)
  let mut seen_active_pid: Option<u32> = None;

  let option = CGWindowListOption::OptionAll | CGWindowListOption::ExcludeDesktopElements;

  let Some(window_list_info) = CGWindowListCopyWindowInfo(option, kCGNullWindowID) else {
    return here;
  };
  // Most windows share an app: ask each app once.
  let mut apps: HashMap<i32, Option<AppInfo>> = HashMap::new();

  let mut cache = spaces_cache().lock();
  let refresh_all = cache.refreshed.elapsed() >= SPACES_REFRESH;
  if refresh_all {
    cache.refreshed = Instant::now();
  }
  let mut seen = HashMap::new();

  let windows_count = CFArray::count(&window_list_info);

  for idx in 0..windows_count {
    let window_cf_dictionary_ref =
      unsafe { CFArray::value_at_index(&window_list_info, idx).cast::<CFDictionary>() };

    let Some(dict) = retain_cf_dictionary(window_cf_dictionary_ref) else {
      continue;
    };

    let on_screen = get_cf_boolean(&dict, "kCGWindowIsOnscreen");

    // On screen: app windows and panels (layers 0 to 100). Off screen: only ordinary windows,
    // as the window server keeps many invisible helper windows.
    let window_layer = get_cf_number(&dict, "kCGWindowLayer");
    let layers = if on_screen { 0..=100 } else { 0..=0 };
    if !layers.contains(&window_layer) {
      continue;
    }

    // Must have valid bounds
    let Some(cg_bounds) = get_cf_window_bounds(&dict) else {
      continue;
    };

    if cg_bounds.size.height < 50.0 || cg_bounds.size.width < 50.0 {
      continue;
    }

    // Must have valid PID
    let process_id = get_cf_number(&dict, "kCGWindowOwnerPID");
    if process_id == 0 {
      continue;
    }

    let Some(app) = *apps.entry(process_id).or_insert_with(|| AppInfo::of(process_id as u32))
    else {
      continue;
    };

    // Off screen, only windows of ordinary apps (with a Dock icon).
    if !on_screen && !app.regular {
      continue;
    }

    let id = get_cf_number(&dict, "kCGWindowNumber") as u32;
    let spaces = match cache.windows.get(&id) {
      Some((was_on_screen, spaces)) if !refresh_all && *was_on_screen == on_screen => {
        spaces.clone()
      }
      _ => skylight::spaces_of_window(id),
    };
    seen.insert(id, (on_screen, spaces.clone()));

    let focused = if on_screen && app.active && seen_active_pid.is_none() {
      seen_active_pid = Some(process_id as u32);
      true
    } else {
      false
    };

    let window = Window {
      id: WindowId::from(id),
      title: get_cf_string(&dict, "kCGWindowName"),
      app_name: get_cf_string(&dict, "kCGWindowOwnerName"),
      bounds: Bounds {
        x: cg_bounds.origin.x,
        y: cg_bounds.origin.y,
        w: cg_bounds.size.width,
        h: cg_bounds.size.height,
      },
      focused,
      process_id: ProcessId::from(process_id as u32),
      z_index: 0,
      presence: presence(on_screen, &spaces, app.hidden),
      spaces,
    };
    if on_screen {
      here.push(window);
    } else {
      elsewhere.push(window);
    }
  }
  cache.windows = seen;

  // Windows that are here come first, front to back; then the rest. Front to back is only
  // documented for on-screen listings, so take it from one (ids only: cheap) rather than from
  // the order of the full listing.
  let order = on_screen_order();
  here.sort_by_key(|w| order.get(&w.id.0).copied().unwrap_or(usize::MAX));
  here.extend(elsewhere);
  for (z, window) in here.iter_mut().enumerate() {
    window.z_index = z as u32;
  }
  here
}

/// Position of each on-screen window, front to back.
fn on_screen_order() -> HashMap<u32, usize> {
  let option =
    CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
  let Some(ids) = CGWindowListCreate(option, kCGNullWindowID) else {
    return HashMap::new();
  };
  // The array holds window ids themselves, not objects.
  (0..CFArray::count(&ids))
    .map(|i| unsafe { CFArray::value_at_index(&ids, i) } as usize as u32)
    .enumerate()
    .map(|(position, id)| (id, position))
    .collect()
}

/// Bundle identifier for a process, if it's a running application.
pub(super) fn bundle_identifier_for_pid(process_id: u32) -> Option<String> {
  get_running_application(process_id).as_deref().and_then(get_bundle_identifier)
}

fn get_bundle_identifier(app: &NSRunningApplication) -> Option<String> {
  app.bundleIdentifier().map(|s| s.to_string())
}

fn get_running_application(process_id: u32) -> Option<Retained<NSRunningApplication>> {
  unsafe {
    objc2::msg_send![
      NSRunningApplication::class(),
      runningApplicationWithProcessIdentifier: process_id as i32
    ]
  }
}

#[cfg(test)]
mod tests {
  use super::presence;
  use crate::types::{Presence, SpaceId};

  #[test]
  fn presence_is_here_then_elsewhere_then_hidden_or_minimised() {
    assert_eq!(presence(true, &[SpaceId(1)], false), Presence::Here);
    assert_eq!(presence(false, &[SpaceId(2)], true), Presence::Elsewhere);
    assert_eq!(presence(false, &[], true), Presence::Hidden);
    assert_eq!(presence(false, &[], false), Presence::Minimised);
  }

  /// Lists this machine's windows with where they are, and times enumeration (cold, then
  /// warm). Depends on the machine, so run it by hand: `cargo test -p allio live_windows -- --ignored --nocapture`.
  #[test]
  #[ignore = "reads this machine's windows and Spaces"]
  fn live_windows() {
    let spaces = super::skylight::spaces();
    println!("spaces: {spaces:?}");
    let start = std::time::Instant::now();
    let windows = super::enumerate_windows();
    let cold = start.elapsed();
    let start = std::time::Instant::now();
    drop(super::enumerate_windows());
    let warm = start.elapsed();
    for w in &windows {
      println!("{:>6} {:<12} {:?} {:?} {}", w.id.0, w.app_name, w.presence, w.spaces, w.title);
    }
    println!("{} windows; enumeration cold {cold:?}, warm {warm:?}", windows.len());
  }
}
