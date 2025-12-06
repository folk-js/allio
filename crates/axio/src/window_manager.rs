//! Window cache - tracks windows and fetches AX elements only for new windows.

use accessibility::AXUIElement;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

use crate::types::{AXWindow, Bounds, WindowId};

#[derive(Clone)]
pub struct ManagedWindow {
  pub info: AXWindow,
  pub ax_element: Option<AXUIElement>,
  /// Title from accessibility API (higher precedence than x-win)
  pub ax_title: Option<String>,
}

// SAFETY: AXUIElement is a CFTypeRef (reference-counted). All access is behind Mutex.
unsafe impl Send for ManagedWindow {}
unsafe impl Sync for ManagedWindow {}

static WINDOW_CACHE: Lazy<Mutex<WindowCache>> = Lazy::new(|| Mutex::new(WindowCache::new()));

struct WindowCache {
  windows: HashMap<WindowId, ManagedWindow>,
}

impl WindowCache {
  fn new() -> Self {
    Self {
      windows: HashMap::new(),
    }
  }
}

pub struct WindowManager;

impl WindowManager {
  /// Returns: (current_windows, added_ids, removed_ids)
  pub fn update_windows(
    new_windows: Vec<AXWindow>,
  ) -> (Vec<ManagedWindow>, Vec<WindowId>, Vec<WindowId>) {
    let mut cache = WINDOW_CACHE.lock();
    let mut added_ids = Vec::new();
    let mut removed_ids = Vec::new();

    let new_ids: std::collections::HashSet<&WindowId> = new_windows.iter().map(|w| &w.id).collect();

    for existing_id in cache.windows.keys() {
      if !new_ids.contains(existing_id) {
        removed_ids.push(existing_id.clone());
      }
    }

    for id in &removed_ids {
      cache.windows.remove(id);
      crate::element_registry::ElementRegistry::remove_window_elements(id);
    }

    let mut result = Vec::new();
    for window_info in new_windows {
      let window_id = window_info.id.clone();

      if let Some(existing) = cache.windows.get_mut(&window_id) {
        // Preserve ax_title across polls
        let preserved_ax_title = existing.ax_title.clone();

        existing.info = window_info;
        existing.ax_title = preserved_ax_title;

        // Apply ax_title if we have it (higher precedence)
        if let Some(ref ax_title) = existing.ax_title {
          existing.info.title = ax_title.clone();
        }

        // Retry fetching AX element if missing (timing issue with macOS AX API)
        if existing.ax_element.is_none() {
          existing.ax_element = Self::fetch_ax_element_for_window(&existing.info);
        }

        result.push(existing.clone());
      } else {
        added_ids.push(window_id.clone());
        let ax_element = Self::fetch_ax_element_for_window(&window_info);
        let managed = ManagedWindow {
          info: window_info,
          ax_element,
          ax_title: None,
        };
        cache.windows.insert(window_id, managed.clone());
        result.push(managed);
      }
    }

    (result, added_ids, removed_ids)
  }

  /// Set the accessibility-derived title for a window (higher precedence than x-win)
  pub fn set_ax_title(window_id: &WindowId, title: String) {
    let mut cache = WINDOW_CACHE.lock();
    if let Some(managed) = cache.windows.get_mut(window_id) {
      managed.ax_title = Some(title.clone());
      managed.info.title = title;
    }
  }

  // TODO: Find better matching strategy. Currently uses bounds-based matching because
  // the private _AXUIElementGetWindow API doesn't work on current macOS versions.
  fn fetch_ax_element_for_window(window: &AXWindow) -> Option<AXUIElement> {
    use crate::platform::get_window_elements;
    use accessibility::AXAttribute;
    use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
    use core_foundation::string::CFString;

    let window_elements = match get_window_elements(window.process_id.as_u32()) {
      Ok(elements) => elements,
      Err(_) => return None,
    };

    if window_elements.is_empty() {
      return None;
    }

    const MARGIN: f64 = 2.0;

    for element in window_elements.iter() {
      let position_attr = CFString::new(kAXPositionAttribute);
      let ax_position_attr = AXAttribute::new(&position_attr);
      let element_pos = element
        .attribute(&ax_position_attr)
        .ok()
        .and_then(|p| crate::platform::macos::extract_position(&p));

      let size_attr = CFString::new(kAXSizeAttribute);
      let ax_size_attr = AXAttribute::new(&size_attr);
      let element_size = element
        .attribute(&ax_size_attr)
        .ok()
        .and_then(|s| crate::platform::macos::extract_size(&s));

      if let (Some((ax_x, ax_y)), Some((ax_w, ax_h))) = (element_pos, element_size) {
        let element_bounds = Bounds {
          x: ax_x,
          y: ax_y,
          w: ax_w,
          h: ax_h,
        };
        if window.bounds.matches(&element_bounds, MARGIN) {
          return Some(element.clone());
        }
      }
    }

    // Fallback: use only element if there's just one
    if window_elements.len() == 1 {
      return Some(window_elements[0].clone());
    }

    None
  }

  pub fn get_window(window_id: &WindowId) -> Option<ManagedWindow> {
    let cache = WINDOW_CACHE.lock();
    cache.windows.get(window_id).cloned()
  }

  /// Find a tracked window by its bounds (position and size).
  /// Used to match an AXWindow element back to its real window ID.
  pub fn find_window_id_by_bounds(bounds: &Bounds) -> Option<WindowId> {
    let cache = WINDOW_CACHE.lock();
    const MARGIN: f64 = 2.0;

    for (window_id, managed) in cache.windows.iter() {
      if managed.info.bounds.matches(bounds, MARGIN) {
        return Some(window_id.clone());
      }
    }
    None
  }

  /// Find a tracked window that contains the given screen point.
  /// Returns the frontmost window (lowest z_index) that contains the point.
  /// Since tracked windows exclude our own PID, this naturally skips our overlay.
  pub fn find_window_at_point(x: f64, y: f64) -> Option<ManagedWindow> {
    let cache = WINDOW_CACHE.lock();

    // Collect windows containing the point
    let mut candidates: Vec<_> = cache
      .windows
      .values()
      .filter(|managed| managed.info.bounds.contains(crate::Point::new(x, y)))
      .collect();

    // Sort by z_index (lowest = frontmost)
    candidates.sort_by_key(|m| m.info.z_index);

    candidates.first().map(|m| (*m).clone())
  }
}
