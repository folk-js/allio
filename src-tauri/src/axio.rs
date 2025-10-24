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

/// Typed update events for accessibility elements
/// Each variant represents a specific change to an element
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "update_type")]
pub enum ElementUpdate {
    /// Value changed (text, number, boolean)
    ValueChanged { element_id: String, value: AXValue },

    /// Title/label changed
    TitleChanged { element_id: String, title: String },

    /// Element was destroyed (no longer exists)
    ElementDestroyed { element_id: String },
}

/// Core accessibility node
///
/// Represents a single element in the accessibility tree.
/// Each node has a unique ID (UUID from ElementRegistry) for direct access.
/// Forms a tree structure via the children field and parent_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXNode {
    // Identity - UUID from ElementRegistry (for direct lookup)
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>, // UUID of parent element (None for root)

    // Role information
    pub role: AXRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,

    // Content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<AXValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,

    // State
    pub focused: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,

    // Geometry (optional, not all nodes have screen position)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    // Tree structure
    pub children_count: usize, // Total number of children (whether loaded or not)
    pub children: Vec<AXNode>, // Loaded children (may be empty even if children_count > 0)
}
