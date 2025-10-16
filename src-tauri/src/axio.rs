/**
 * AXIO - Accessibility I/O Layer (Rust)
 *
 * Core types for the AXIO system, mirroring TypeScript types exactly.
 * Based on a principled subset of ARIA roles.
 */
use serde::{Deserialize, Serialize};

// ============================================================================
// Re-export AXValue from ax_value module
// ============================================================================

pub use crate::ax_value::AXValue;

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

/// Core accessibility node
///
/// Represents a single element in the accessibility tree with:
/// - Identity (id, role)
/// - Content (title, value, description)
/// - State (focused, enabled)
/// - Geometry (position, size)
/// - Tree structure (children)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXNode {
    // Identity
    pub id: String,
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
    pub children: Vec<AXNode>,
}

/// Root of an accessibility tree (represents an application window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXRoot {
    #[serde(flatten)]
    pub node: AXNode,
    pub process_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<u64>,
}

impl AXNode {
    /// Create a new node with required fields
    pub fn new(id: String, role: AXRole) -> Self {
        Self {
            id,
            role,
            subrole: None,
            title: None,
            value: None,
            description: None,
            placeholder: None,
            focused: false,
            enabled: true,
            selected: None,
            bounds: None,
            children: Vec::new(),
        }
    }

    /// Builder pattern for setting bounds
    pub fn with_bounds(mut self, position: (f64, f64), size: (f64, f64)) -> Self {
        self.bounds = Some(Bounds {
            position: Position {
                x: position.0,
                y: position.1,
            },
            size: Size {
                width: size.0,
                height: size.1,
            },
        });
        self
    }

    /// Builder pattern for setting title
    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    /// Builder pattern for setting value
    pub fn with_value(mut self, value: AXValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Builder pattern for adding children
    pub fn with_children(mut self, children: Vec<AXNode>) -> Self {
        self.children = children;
        self
    }
}

impl AXRoot {
    /// Create a new root node
    pub fn new(id: String, role: AXRole, process_id: u32) -> Self {
        Self {
            node: AXNode::new(id, role),
            process_id,
            window_id: None,
        }
    }
}
