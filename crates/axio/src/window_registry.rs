use crate::platform::{self, ElementHandle};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::types::{AXWindow, WindowId};

/// Internal storage - window data plus platform handle.
struct StoredWindow {
  /// Window info (pure data, serializable)
  info: AXWindow,
  /// Platform handle for accessibility operations (opaque)
  handle: Option<ElementHandle>,
}

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
        existing.handle = platform::fetch_window_handle(&existing.info);
      }
    } else {
      // New window
      added.push(window_id.clone());
      let handle = platform::fetch_window_handle(&window_info);
      registry.windows.insert(
        window_id,
        StoredWindow {
          info: window_info,
          handle,
        },
      );
    }
  }

  // Update depth order - build once, store reference
  let mut windows: Vec<_> = registry.windows.values().map(|s| &s.info).collect();
  windows.sort_by_key(|w| w.z_index);
  let depth_order: Vec<WindowId> = windows.into_iter().map(|w| w.id.clone()).collect();
  registry.depth_order.clone_from(&depth_order);

  UpdateResult {
    added,
    removed,
    changed,
    depth_order,
  }
}

/// Set active window.
pub(crate) fn set_active(window_id: Option<WindowId>) {
  REGISTRY.write().active = window_id;
}

/// Get window info with handle (for operations that need both).
pub(crate) fn get_with_handle(window_id: &WindowId) -> Option<(AXWindow, Option<ElementHandle>)> {
  REGISTRY
    .read()
    .windows
    .get(window_id)
    .map(|s| (s.info.clone(), s.handle.clone()))
}
