//! Core types for AXIO. TypeScript types auto-generated via ts-rs.
//! Regenerate: `npm run typegen` or `cargo test -p axio export_bindings`

use branded::Branded;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use ts_rs::TS;

// Branded ID types for type safety

#[derive(Branded, TS)]
#[branded(serde)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct ElementId(pub String);

impl Borrow<str> for ElementId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Branded)]
#[branded(serde)]
pub struct WindowId(pub String);

impl Borrow<str> for WindowId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

// Error types

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

// Value and geometry types

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "type", content = "value")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum AXValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Bounds {
    pub position: Position,
    pub size: Size,
}

// ARIA role subset

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum AXRole {
    Application,
    Document,
    Window,
    Group,
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
    Text,
    Heading,
    Image,
    List,
    Listitem,
    Table,
    Row,
    Cell,
    Progressbar,
    Scrollbar,
    Unknown,
}

/// The unified element type - stored in registry and returned from API.
/// Flat structure: children are IDs, not nested. Trees derived client-side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct AXElement {
    pub id: ElementId,
    pub window_id: String,
    /// null = no parent (root element)
    pub parent_id: Option<ElementId>,
    /// Child element IDs. null = not yet discovered, [] = no children
    pub children: Option<Vec<ElementId>>,
    pub role: AXRole,
    pub subrole: Option<String>,
    pub label: Option<String>,
    pub value: Option<AXValue>,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub bounds: Option<Bounds>,
    pub focused: Option<bool>,
    pub enabled: Option<bool>,
}

/// Window info from x-win + accessibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
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
    /// Top-level element IDs. null = not yet discovered, [] = empty
    pub children: Option<Vec<ElementId>>,
}

// Events

/// Snapshot sent on client connection - full current state
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct Snapshot {
    pub windows: Vec<AXWindow>,
    pub active_window: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum ServerEvent {
    // Sync
    #[serde(rename = "sync:snapshot")]
    Snapshot(Snapshot),

    // Window lifecycle
    #[serde(rename = "window:opened")]
    WindowOpened(AXWindow),
    #[serde(rename = "window:closed")]
    WindowClosed { window_id: String },
    #[serde(rename = "window:updated")]
    WindowUpdated(AXWindow),

    // Focus
    #[serde(rename = "window:active")]
    WindowActive { window_id: Option<String> },

    // Elements
    #[serde(rename = "element:discovered")]
    ElementDiscovered(AXElement),
    #[serde(rename = "element:updated")]
    ElementUpdated {
        element: AXElement,
        changed: Vec<String>,
    },
    #[serde(rename = "element:destroyed")]
    ElementDestroyed { element_id: ElementId },

    // Input
    #[serde(rename = "mouse:position")]
    MousePosition { x: f64, y: f64 },
}
