//! Window cache - tracks windows and fetches AX elements only for new windows.

use accessibility::AXUIElement;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{AXWindow, WindowId};

#[derive(Clone)]
pub struct ManagedWindow {
    pub info: AXWindow,
    pub ax_element: Option<AXUIElement>,
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
        let mut cache = WINDOW_CACHE.lock().unwrap();
        let mut added_ids = Vec::new();
        let mut removed_ids = Vec::new();

        let new_ids: std::collections::HashSet<&str> =
            new_windows.iter().map(|w| w.id.as_str()).collect();

        for existing_id in cache.windows.keys() {
            if !new_ids.contains(existing_id.0.as_str()) {
                removed_ids.push(existing_id.clone());
            }
        }

        for id in &removed_ids {
            cache.windows.remove(id);
            crate::element_registry::ElementRegistry::remove_window_elements(id);
        }

        let mut result = Vec::new();
        for window_info in new_windows {
            let window_id = WindowId::new(window_info.id.clone());

            if let Some(existing) = cache.windows.get_mut(&window_id) {
                existing.info = window_info;

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
                };
                cache.windows.insert(window_id, managed.clone());
                result.push(managed);
            }
        }

        (result, added_ids, removed_ids)
    }

    // TODO: Find better matching strategy. Currently uses bounds-based matching because
    // the private _AXUIElementGetWindow API doesn't work on current macOS versions.
    fn fetch_ax_element_for_window(window: &AXWindow) -> Option<AXUIElement> {
        use crate::platform::get_window_elements;
        use accessibility::AXAttribute;
        use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
        use core_foundation::string::CFString;

        let window_elements = match get_window_elements(window.process_id) {
            Ok(elements) => elements,
            Err(_) => return None,
        };

        if window_elements.is_empty() {
            return None;
        }

        const MARGIN: i32 = 2;

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
                let pos_ok = (ax_x as i32 - window.x).abs() <= MARGIN
                    && (ax_y as i32 - window.y).abs() <= MARGIN;
                let size_ok = (ax_w as i32 - window.w).abs() <= MARGIN
                    && (ax_h as i32 - window.h).abs() <= MARGIN;

                if pos_ok && size_ok {
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
        let cache = WINDOW_CACHE.lock().unwrap();
        cache.windows.get(window_id).cloned()
    }
}
