/*! Element type representing a UI element in the accessibility tree. */

use super::{Bounds, ElementId, ProcessId, WindowId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Core Element type - stored in registry and returned from API.
///
/// Elements are flat: children are IDs, not nested. Trees are derived client-side.
///
/// Parent linkage semantics:
/// - `is_root=true, parent_id=None` → window root element
/// - `is_root=false, parent_id=Some(id)` → parent is loaded (linked)
/// - `is_root=false, parent_id=None` → orphan (parent exists but not loaded)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Element {
  pub id: ElementId,
  /// Window this element belongs to
  pub window_id: WindowId,
  /// Process that owns this element (may differ from window's process for helper processes)
  pub pid: ProcessId,
  pub is_root: bool,
  pub parent_id: Option<ElementId>,
  pub children: Option<Vec<ElementId>>,
  pub role: crate::accessibility::Role,
  /// Raw platform role string for debugging (e.g., "`AXRadioGroup`", "AXButton/AXCloseButton")
  pub platform_role: String,

  // === Text properties ===
  pub label: Option<String>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  /// URL for links, file paths (Finder), documents
  pub url: Option<String>,

  // === Value ===
  pub value: Option<crate::accessibility::Value>,

  // === Geometry ===
  pub bounds: Option<Bounds>,

  // === States ===
  pub focused: Option<bool>,
  /// Whether the element is disabled (matches ARIA aria-disabled)
  pub disabled: bool,
  /// Selection state for items in lists/tables
  pub selected: Option<bool>,
  /// Expansion state for tree nodes, disclosure triangles
  pub expanded: Option<bool>,

  // === Table/Collection position ===
  /// Row index for cells/rows in tables (0-based)
  pub row_index: Option<usize>,
  /// Column index for cells in tables (0-based)
  pub column_index: Option<usize>,
  /// Total row count (for table containers)
  pub row_count: Option<usize>,
  /// Total column count (for table containers)
  pub column_count: Option<usize>,

  // === Actions ===
  pub actions: Vec<crate::accessibility::Action>,

  // === Hit Test Status ===
  /// True if this element is a fallback container from Chromium/Electron lazy init.
  /// Only meaningful for elements returned from `fetch_element_at`.
  /// Client should retry hit test on next frame to get the real element.
  #[serde(default)]
  pub is_fallback: bool,
}
