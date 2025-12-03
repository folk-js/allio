/**
 * Window Manager - Efficient Window Tracking with AX Elements
 *
 * Maintains a cache of windows with their AXUIElement references.
 * Only fetches AX elements when windows are added, not on every poll.
 */
use accessibility::AXUIElement;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{WindowId, WindowInfo};

/// Cached window data with AX element
#[derive(Clone)]
pub struct ManagedWindow {
    pub info: WindowInfo,
    pub ax_element: Option<AXUIElement>,
    pub ax_window_id: Option<u32>, // CGWindowID from _AXUIElementGetWindow
}

/// Global window cache - tracks windows and their AX elements
static WINDOW_CACHE: Lazy<Mutex<WindowCache>> = Lazy::new(|| Mutex::new(WindowCache::new()));

struct WindowCache {
    windows: HashMap<WindowId, ManagedWindow>, // window_id -> ManagedWindow
}

impl WindowCache {
    fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }
}

/// Public interface for window management
pub struct WindowManager;

impl WindowManager {
    /// Update windows from polling, fetching AX elements only for new windows
    ///
    /// Returns: (current_windows, added_ids, removed_ids)
    pub fn update_windows(
        new_windows: Vec<WindowInfo>,
    ) -> (Vec<ManagedWindow>, Vec<WindowId>, Vec<WindowId>) {
        let mut cache = WINDOW_CACHE.lock().unwrap();

        // Track which windows are new/removed
        let mut added_ids = Vec::new();
        let mut removed_ids = Vec::new();

        // Build set of current window IDs
        let new_ids: std::collections::HashSet<_> =
            new_windows.iter().map(|w| WindowId::new(&w.id)).collect();

        // Find removed windows
        for existing_id in cache.windows.keys() {
            if !new_ids.contains(existing_id) {
                removed_ids.push(existing_id.clone());
            }
        }

        // Remove old windows from cache and clean up their elements
        for id in &removed_ids {
            cache.windows.remove(id);

            // Clean up all accessibility elements associated with this window
            crate::element_registry::ElementRegistry::remove_window_elements(id);
        }

        // Process new/updated windows
        let mut result = Vec::new();
        for window_info in new_windows {
            let window_id = WindowId::new(&window_info.id);

            if let Some(existing) = cache.windows.get_mut(&window_id) {
                // Existing window - update info
                existing.info = window_info.clone();

                // If we don't have an AX element yet, try fetching it again
                // This handles the case where the AXWindow element wasn't in the children
                // list when we first detected the window (timing issue with macOS AX API)
                if existing.ax_element.is_none() {
                    let (ax_element, ax_window_id) =
                        Self::fetch_ax_element_for_window(&window_info);
                    if ax_element.is_some() {
                        existing.ax_element = ax_element;
                        existing.ax_window_id = ax_window_id;
                    }
                }

                result.push(existing.clone());
            } else {
                // New window - fetch AX element
                added_ids.push(window_id.clone());

                let (ax_element, ax_window_id) = Self::fetch_ax_element_for_window(&window_info);

                let managed = ManagedWindow {
                    info: window_info,
                    ax_element: ax_element.clone(),
                    ax_window_id,
                };

                cache.windows.insert(window_id.clone(), managed.clone());
                result.push(managed);
            }
        }

        (result, added_ids, removed_ids)
    }

    /// Fetch AX element for a window (only called for new windows)
    ///
    /// NOTE: This currently uses bounds-based matching (position + size) because the
    /// private _AXUIElementGetWindow API is not working on current macOS versions.
    ///
    /// TODO: Find a better matching strategy. Possible alternatives:
    /// - Use AXUIElement equality/hashing if available
    /// - Match by window layer number or other attributes
    /// - Investigate if there's a public API we're missing
    fn fetch_ax_element_for_window(window: &WindowInfo) -> (Option<AXUIElement>, Option<u32>) {
        use crate::platform::get_window_elements;
        use accessibility::AXAttribute;
        use accessibility_sys::kAXPositionAttribute;
        use accessibility_sys::kAXSizeAttribute;
        use core_foundation::string::CFString;

        // Get all window elements for this PID
        let window_elements = match get_window_elements(window.process_id) {
            Ok(elements) => elements,
            Err(_) => return (None, None),
        };

        if window_elements.is_empty() {
            return (None, None);
        }

        // Match windows by bounds (position + size) with 2px margin
        const POSITION_MARGIN: i32 = 2;
        const SIZE_MARGIN: i32 = 2;

        for element in window_elements.iter() {
            // Get element position
            let position_attr = CFString::new(kAXPositionAttribute);
            let ax_position_attr = AXAttribute::new(&position_attr);

            let element_pos = element.attribute(&ax_position_attr).ok().and_then(|p| {
                use crate::platform::macos::extract_position;
                extract_position(&p)
            });

            // Get element size
            let size_attr = CFString::new(kAXSizeAttribute);
            let ax_size_attr = AXAttribute::new(&size_attr);

            let element_size = element.attribute(&ax_size_attr).ok().and_then(|s| {
                use crate::platform::macos::extract_size;
                extract_size(&s)
            });

            // Check if bounds match (within margin)
            if let (Some((ax_x, ax_y)), Some((ax_w, ax_h))) = (element_pos, element_size) {
                let pos_diff_x = (ax_x as i32 - window.x).abs();
                let pos_diff_y = (ax_y as i32 - window.y).abs();
                let size_diff_w = (ax_w as i32 - window.w).abs();
                let size_diff_h = (ax_h as i32 - window.h).abs();

                let position_matches =
                    pos_diff_x <= POSITION_MARGIN && pos_diff_y <= POSITION_MARGIN;
                let size_matches = size_diff_w <= SIZE_MARGIN && size_diff_h <= SIZE_MARGIN;

                if position_matches && size_matches {
                    return (Some(element.clone()), None); // No longer return CGWindowID
                }
            }
        }

        // Fallback: If only 1 window element exists, use it
        if window_elements.len() == 1 {
            return (Some(window_elements[0].clone()), None);
        }

        (None, None)
    }

    /// Get a managed window by ID
    pub fn get_window(window_id: &WindowId) -> Option<ManagedWindow> {
        let cache = WINDOW_CACHE.lock().unwrap();
        cache.windows.get(window_id).cloned()
    }
}

// Manual Send/Sync implementation (AXUIElement is thread-safe behind Mutex)
unsafe impl Send for ManagedWindow {}
unsafe impl Sync for ManagedWindow {}
