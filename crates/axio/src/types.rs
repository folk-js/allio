/*! Regenerate: `npm run typegen` */

use derive_more::{Display, From, Into};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use ts_rs::TS;

#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct WindowId(pub u32);

#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct ElementId(pub u32);

/// Global counter for ElementId generation. Starts at 1 (0 could be confused with "null").
static ELEMENT_COUNTER: AtomicU32 = AtomicU32::new(1);

impl ElementId {
  /// Generate a new unique ElementId.
  pub fn new() -> Self {
    Self(ELEMENT_COUNTER.fetch_add(1, Ordering::Relaxed))
  }
}

impl Default for ElementId {
  fn default() -> Self {
    Self::new()
  }
}

/// Process ID - branded type to distinguish from other u32 values.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct ProcessId(pub u32);

// === Error types ===

#[derive(Debug, thiserror::Error)]
pub enum AxioError {
  #[error("Element not found: {0}")]
  ElementNotFound(ElementId),

  #[error("Window not found: {0}")]
  WindowNotFound(WindowId),

  #[error("Accessibility operation failed: {0}")]
  AccessibilityError(String),

  #[error("Observer error: {0}")]
  ObserverError(String),

  #[error("Operation not supported: {0}")]
  NotSupported(String),

  #[error("Internal error: {0}")]
  Internal(String),
}

pub type AxioResult<T> = Result<T, AxioError>;

// === Geometry types ===

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Bounds {
  pub x: f64,
  pub y: f64,
  pub w: f64,
  pub h: f64,
}

impl Bounds {
  /// Check if two bounds match within a margin of error.
  pub fn matches(&self, other: &Bounds, margin: f64) -> bool {
    (self.x - other.x).abs() <= margin
      && (self.y - other.y).abs() <= margin
      && (self.w - other.w).abs() <= margin
      && (self.h - other.h).abs() <= margin
  }

  /// Check if a point is contained within these bounds.
  pub fn contains(&self, point: Point) -> bool {
    point.x >= self.x
      && point.x <= self.x + self.w
      && point.y >= self.y
      && point.y <= self.y + self.h
  }
}

/// A 2D point in screen coordinates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Point {
  pub x: f64,
  pub y: f64,
}

impl Point {
  pub fn new(x: f64, y: f64) -> Self {
    Self { x, y }
  }

  /// Euclidean distance to another point.
  pub fn distance_to(&self, other: Point) -> f64 {
    ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
  }

  /// Check if this point moved more than threshold from another.
  pub fn moved_from(&self, other: Point, threshold: f64) -> bool {
    (self.x - other.x).abs() >= threshold || (self.y - other.y).abs() >= threshold
  }
}

// === Core types ===

/// Window info from x-win + accessibility.
/// Note: Windows don't have children - elements reference windows via window_id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct AXWindow {
  pub id: WindowId,
  pub title: String,
  pub app_name: String,
  pub bounds: Bounds,
  pub focused: bool,
  pub process_id: ProcessId,
  /// Z-order index: 0 = frontmost, higher = further back
  pub z_index: u32,
}

/// The unified element type - stored in registry and returned from API.
/// Flat structure: children are IDs, not nested. Trees derived client-side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct AXElement {
  pub id: ElementId,
  /// Window this element belongs to
  pub window_id: WindowId,
  /// None = root element of window
  pub parent_id: Option<ElementId>,
  /// Child element IDs. None = not yet fetched, Some([]) = no children
  pub children: Option<Vec<ElementId>>,
  pub role: crate::accessibility::Role,
  pub subrole: Option<String>,
  pub label: Option<String>,
  pub value: Option<crate::accessibility::Value>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub bounds: Option<Bounds>,
  pub focused: Option<bool>,
  pub enabled: Option<bool>,
  /// Available actions for this element
  pub actions: Vec<crate::accessibility::Action>,
}

// === Events ===
// Events notify clients when the Registry changes.
// Any registry change emits an event, regardless of trigger.

/// Initial state sent on connection
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Selection {
  pub element_id: ElementId,
  pub text: String,
  pub range: Option<TextRange>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct SyncInit {
  pub windows: Vec<AXWindow>,
  pub elements: Vec<AXElement>,
  pub active_window: Option<WindowId>,
  pub focused_window: Option<WindowId>,
  pub focused_element: Option<AXElement>,
  pub selection: Option<Selection>,
  /// Window IDs in z-order (front to back)
  pub depth_order: Vec<WindowId>,
  /// Whether accessibility permissions are granted
  pub accessibility_enabled: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum Event {
  // Initial sync (on connection)
  #[serde(rename = "sync:init")]
  SyncInit(SyncInit),

  // Window lifecycle (from polling)
  #[serde(rename = "window:added")]
  WindowAdded {
    window: AXWindow,
    depth_order: Vec<WindowId>,
  },
  #[serde(rename = "window:changed")]
  WindowChanged {
    window: AXWindow,
    depth_order: Vec<WindowId>,
  },
  #[serde(rename = "window:removed")]
  WindowRemoved {
    window_id: WindowId,
    depth_order: Vec<WindowId>,
  },

  // Element lifecycle (from RPC, watches)
  #[serde(rename = "element:added")]
  ElementAdded { element: AXElement },
  #[serde(rename = "element:changed")]
  ElementChanged { element: AXElement },
  #[serde(rename = "element:removed")]
  ElementRemoved { element_id: ElementId },

  // Window focus (from polling)
  #[serde(rename = "focus:changed")]
  FocusChanged { window_id: Option<WindowId> },
  #[serde(rename = "active:changed")]
  ActiveChanged { window_id: WindowId },

  // Element focus (from Tier 1 app-level observer)
  #[serde(rename = "focus:element")]
  FocusElement {
    element: AXElement,
    previous_element_id: Option<ElementId>,
  },

  // Text selection (from Tier 1 app-level observer)
  #[serde(rename = "selection:changed")]
  SelectionChanged {
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<TextRange>,
  },

  // Input tracking
  #[serde(rename = "mouse:position")]
  MousePosition(Point),
}

/// Text selection range within an element
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct TextRange {
  pub start: u32,
  pub length: u32,
}

impl TextRange {
  pub fn new(start: u32, length: u32) -> Self {
    Self { start, length }
  }

  /// End position (exclusive).
  pub fn end(&self) -> u32 {
    self.start + self.length
  }

  pub fn is_empty(&self) -> bool {
    self.length == 0
  }

  /// Check if a position falls within this range.
  pub fn contains(&self, position: u32) -> bool {
    position >= self.start && position < self.end()
  }
}
