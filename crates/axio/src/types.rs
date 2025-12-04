//! AXIO - Accessibility I/O Layer (Rust)
//!
//! Core types for the AXIO system.
//! TypeScript types are auto-generated via ts-rs to `src-web/src/generated/`.
//!
//! To regenerate: `npm run typegen` or `cargo test -p axio export_bindings`

use branded::Branded;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use ts_rs::TS;

// ============================================================================
// Identity Types (Branded newtypes for type safety)
// ============================================================================

/// Unique identifier for an accessibility element (UUID string)
///
/// Using the `branded` crate ensures type safety - you can't accidentally
/// pass a `WindowId` where an `ElementId` is expected.
#[derive(Branded, TS)]
#[branded(serde)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct ElementId(pub String);

impl Borrow<str> for ElementId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Unique identifier for a window (from x-win)
///
/// Using the `branded` crate ensures type safety - you can't accidentally
/// pass an `ElementId` where a `WindowId` is expected.
#[derive(Branded)]
#[branded(serde)]
pub struct WindowId(pub String);

impl Borrow<str> for WindowId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur in AXIO operations
///
/// Not all variants are used in every code path, but they form the complete
/// error surface for AXIO operations.
#[derive(Debug, thiserror::Error)]
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

impl AxioError {
    /// Create an AccessibilityError from any error type
    #[allow(dead_code)] // Utility function for error conversion
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
#[ts(export, export_to = "packages/axio-client/src/types/")]
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
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// 2D size dimensions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Geometric bounds (position + size)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
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
#[ts(export, export_to = "packages/axio-client/src/types/")]
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

// Platform-specific types (notifications, role mappings) are in platform/<os>.rs

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
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
// Window
// ============================================================================

/// A window with its accessibility tree root
///
/// Combines OS window metadata with the accessibility tree.
/// The root is populated asynchronously after window discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct AXWindow {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub focused: bool,
    pub process_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<AXNode>,
}

// ============================================================================
// Events (for notification broadcasts)
// ============================================================================

/// Update event for an element (broadcast when changes are observed)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "update_type", rename_all = "PascalCase")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum ElementUpdate {
    ValueChanged { element_id: String, value: AXValue },
    LabelChanged { element_id: String, label: String },
    ElementDestroyed { element_id: String },
}

/// Server-sent events (WebSocket)
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum ServerEvent {
    WindowUpdate(Vec<AXWindow>),
    WindowRoot { window_id: String, root: AXNode },
    MousePosition { x: f64, y: f64 },
    ElementUpdate(ElementUpdate),
}
