//! AXIO - Accessibility I/O Layer (Rust)
//!
//! Core types for the AXIO system.
//! TypeScript types are auto-generated via ts-rs to `src-web/src/generated/`.
//!
//! To regenerate: `npm run typegen` or `cargo test -p axio export_bindings`

use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;

// ============================================================================
// Identity Types (Newtypes for type safety)
// ============================================================================

/// Unique identifier for an accessibility element
///
/// Wraps a UUID string. Using a newtype prevents accidentally passing
/// a WindowId where an ElementId is expected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct ElementId(pub String);

impl ElementId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ElementId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ElementId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a window
///
/// Wraps an ID string from x-win. Using a newtype prevents accidentally
/// passing an ElementId where a WindowId is expected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(pub String);

#[allow(dead_code)]
impl WindowId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for WindowId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for WindowId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur in AXIO operations
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Not all variants used yet
pub enum AxioError {
    /// Element with given ID was not found in the registry
    #[error("Element not found: {0}")]
    ElementNotFound(ElementId),

    /// Window with given ID was not found
    #[error("Window not found: {0}")]
    WindowNotFound(WindowId),

    /// An accessibility API operation failed
    #[error("Accessibility operation failed: {0}")]
    AccessibilityError(String),

    /// Failed to create or manage an AXObserver
    #[error("Observer error: {0}")]
    ObserverError(String),

    /// Element doesn't support the requested operation
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Internal error (should not happen in normal operation)
    #[error("Internal error: {0}")]
    Internal(String),
}

#[allow(dead_code)]
impl AxioError {
    /// Create an AccessibilityError from any error type
    pub fn ax<E: std::error::Error>(e: E) -> Self {
        Self::AccessibilityError(e.to_string())
    }
}

/// Result type for AXIO operations
pub type AxioResult<T> = Result<T, AxioError>;

// ============================================================================
// Value Types
// ============================================================================

/// Represents a properly typed accessibility value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "type", content = "value")]
#[ts(export, export_to = "src-web/src/generated/")]
pub enum AXValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

// ============================================================================
// Geometry Types
// ============================================================================

/// 2D position in screen coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// 2D size dimensions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Geometric bounds (position + size)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct Bounds {
    pub position: Position,
    pub size: Size,
}

// ============================================================================
// ARIA Role Subset
// ============================================================================

/// ARIA role subset covering common UI elements
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "src-web/src/generated/")]
pub enum AXRole {
    // Document structure
    Application,
    Document,
    Window,
    Group,

    // Interactive elements
    Button,
    Checkbox,
    Radio,
    Toggle,
    Textbox,
    Searchbox,
    Slider,
    Menu,
    Menuitem,
    Menubar,
    Link,
    Tab,
    Tablist,

    // Static content
    Text,
    Heading,
    Image,
    List,
    Listitem,
    Table,
    Row,
    Cell,

    // Other
    Progressbar,
    Scrollbar,
    Unknown,
}

// Platform-specific role conversions are handled in the platform module
// to keep AXIO types platform-agnostic

// ============================================================================
// Node Structure
// ============================================================================

/// Core accessibility element
///
/// Represents a single element in the accessibility tree.
/// Each element has a unique ID (UUID from ElementRegistry) for direct access.
///
/// **Hydration semantics:**
/// - `None` = "not fetched" (unknown)
/// - `Some(value)` = "value when queried" (may be stale)
///
/// Only `id` and `role` are always present (required for identity).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct AXNode {
    // ══════════════════════════════════════════════════════════════════
    // IDENTITY (always present)
    // ══════════════════════════════════════════════════════════════════
    /// UUID from ElementRegistry (for direct lookup)
    pub id: ElementId,

    /// ARIA-style role
    pub role: AXRole,

    // ══════════════════════════════════════════════════════════════════
    // OPTIONAL FIELDS (populated based on query type)
    // ══════════════════════════════════════════════════════════════════
    /// UUID of parent element (None for root or if not fetched)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<ElementId>,

    /// Platform-specific subtype (or native role name for Unknown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,

    // Content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<AXValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,

    // State
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,

    // Geometry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    // Tree structure
    /// Total number of children (None if not fetched)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_count: Option<usize>,
    /// Loaded children (None if not fetched, Some([]) if no children)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<AXNode>>,
}

// ============================================================================
// Window Information
// ============================================================================

/// Information about a visible window
///
/// This is the core window metadata type used throughout AXIO.
/// It's populated from platform-specific APIs (x-win on macOS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "src-web/src/generated/")]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub focused: bool,
    pub process_id: u32,
}

// ============================================================================
// Events (for notification broadcasts)
// ============================================================================

/// Update event for an element (broadcast when changes are observed)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "update_type", rename_all = "PascalCase")]
#[ts(export, export_to = "src-web/src/generated/")]
pub enum ElementUpdate {
    ValueChanged { element_id: String, value: AXValue },
    LabelChanged { element_id: String, label: String },
    ElementDestroyed { element_id: String },
}
