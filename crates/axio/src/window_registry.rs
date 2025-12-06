//! Window registry - single source of truth for all window state.
//!
//! Consolidates:
//! - Window data (AXWindow)
//! - Active/focused window tracking
//! - Depth order (z-index)
//! - Platform handles (AXUIElement) - internal

use accessibility::AXUIElement;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::types::{AXWindow, Bounds, WindowId};

/// Internal storage - window data plus platform handle.
struct StoredWindow {
  /// Window info (pure data, serializable)
  info: AXWindow,
  /// Platform handle for accessibility operations
  handle: Option<AXUIElement>,
}

// SAFETY: AXUIElement is a CFTypeRef (reference-counted). All access is behind RwLock.
unsafe impl Send for StoredWindow {}
unsafe impl Sync for StoredWindow {}

static REGISTRY: LazyLock<RwLock<WindowRegistry>> =
  LazyLock::new(|| RwLock::new(WindowRegistry::new()));

struct WindowRegistry {
  /// All tracked windows by ID
  windows: HashMap<WindowId, StoredWindow>,
  /// Currently active window (preserved when desktop focused)
  active: Option<WindowId>,
  /// Window IDs in z-order (front to back)
  depth_order: Vec<WindowId>,
}

impl WindowRegistry {
  fn new() -> Self {
    Self {
      windows: HashMap::new(),
      active: None,
      depth_order: Vec::new(),
    }
  }
}

// =============================================================================
// Public API - operates on WindowId, returns pure data
// =============================================================================

/// Get all current windows (pure data snapshot).
pub fn get_windows() -> Vec<AXWindow> {
  REGISTRY
    .read()
    .windows
    .values()
    .map(|s| s.info.clone())
    .collect()
}

/// Get a specific window by ID.
pub fn get_window(window_id: &WindowId) -> Option<AXWindow> {
  REGISTRY
    .read()
    .windows
    .get(window_id)
    .map(|s| s.info.clone())
}

/// Get the active window ID (preserved when desktop is focused).
pub fn get_active() -> Option<WindowId> {
  REGISTRY.read().active.clone()
}

/// Get window IDs in depth order (front to back).
pub fn get_depth_order() -> Vec<WindowId> {
  REGISTRY.read().depth_order.clone()
}

/// Find window containing a point. Returns frontmost (lowest z_index).
pub fn find_at_point(x: f64, y: f64) -> Option<AXWindow> {
  let registry = REGISTRY.read();
  let point = crate::Point::new(x, y);

  let mut candidates: Vec<_> = registry
    .windows
    .values()
    .filter(|s| s.info.bounds.contains(point))
    .collect();

  candidates.sort_by_key(|s| s.info.z_index);
  candidates.first().map(|s| s.info.clone())
}

/// Find window by bounds (for matching AX elements to windows).
pub fn find_by_bounds(bounds: &Bounds) -> Option<WindowId> {
  let registry = REGISTRY.read();
  const MARGIN: f64 = 2.0;

  registry
    .windows
    .iter()
    .find(|(_, s)| s.info.bounds.matches(bounds, MARGIN))
    .map(|(id, _)| id.clone())
}

// =============================================================================
// Internal API - for polling loop and platform operations
// =============================================================================

/// Update result from polling loop.
pub struct UpdateResult {
  pub added: Vec<WindowId>,
  pub removed: Vec<WindowId>,
  pub changed: Vec<WindowId>,
  pub depth_order: Vec<WindowId>,
}

/// Update windows from polling. Returns what changed.
pub(crate) fn update(new_windows: Vec<AXWindow>) -> UpdateResult {
  let mut registry = REGISTRY.write();
  let mut added = Vec::new();
  let mut removed = Vec::new();
  let mut changed = Vec::new();

  // Find removed windows
  let new_ids: std::collections::HashSet<&WindowId> = new_windows.iter().map(|w| &w.id).collect();
  for existing_id in registry.windows.keys() {
    if !new_ids.contains(existing_id) {
      removed.push(existing_id.clone());
    }
  }

  // Remove them (and clean up elements)
  for id in &removed {
    registry.windows.remove(id);
    crate::element_registry::ElementRegistry::remove_window_elements(id);
  }

  // Process new/existing windows
  for window_info in new_windows {
    let window_id = window_info.id.clone();

    if let Some(existing) = registry.windows.get_mut(&window_id) {
      // Check if changed
      if existing.info != window_info {
        changed.push(window_id.clone());
      }

      existing.info = window_info;

      // Retry fetching handle if missing
      if existing.handle.is_none() {
        existing.handle = fetch_handle_for_window(&existing.info);
      }
    } else {
      // New window
      added.push(window_id.clone());
      let handle = fetch_handle_for_window(&window_info);
      registry.windows.insert(
        window_id,
        StoredWindow {
          info: window_info,
          handle,
        },
      );
    }
  }

  // Update depth order
  let mut windows: Vec<_> = registry.windows.values().map(|s| &s.info).collect();
  windows.sort_by_key(|w| w.z_index);
  registry.depth_order = windows.into_iter().map(|w| w.id.clone()).collect();

  UpdateResult {
    added,
    removed,
    changed,
    depth_order: registry.depth_order.clone(),
  }
}

/// Set active window.
pub(crate) fn set_active(window_id: Option<WindowId>) {
  REGISTRY.write().active = window_id;
}

/// Get window info with handle (for operations that need both).
pub(crate) fn get_with_handle(window_id: &WindowId) -> Option<(AXWindow, Option<AXUIElement>)> {
  REGISTRY
    .read()
    .windows
    .get(window_id)
    .map(|s| (s.info.clone(), s.handle.clone()))
}

// =============================================================================
// Handle fetching (platform-specific)
// =============================================================================

fn fetch_handle_for_window(window: &AXWindow) -> Option<AXUIElement> {
  use crate::platform::get_window_elements;
  use accessibility::AXAttribute;
  use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
  use core_foundation::string::CFString;

  let window_elements = get_window_elements(window.process_id.as_u32()).ok()?;

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
