/**
 * AXIO - Accessibility I/O Layer (Rust)
 *
 * Core types for the AXIO system, mirroring TypeScript types exactly.
 * Based on a principled subset of ARIA roles.
 */
use serde::{Deserialize, Serialize};

// ============================================================================
// Value Types
// ============================================================================

/// Represents a properly typed accessibility value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// 2D size dimensions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Geometric bounds (position + size)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub position: Position,
    pub size: Size,
}

// ============================================================================
// ARIA Role Subset
// ============================================================================

/// ARIA role subset covering common UI elements
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXNode {
    // ══════════════════════════════════════════════════════════════════
    // IDENTITY (always present)
    // ══════════════════════════════════════════════════════════════════
    /// UUID from ElementRegistry (for direct lookup)
    pub id: String,

    /// ARIA-style role
    pub role: AXRole,

    // ══════════════════════════════════════════════════════════════════
    // OPTIONAL FIELDS (populated based on query type)
    // ══════════════════════════════════════════════════════════════════
    /// UUID of parent element (None for root or if not fetched)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

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
