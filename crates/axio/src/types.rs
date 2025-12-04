//! Core types for AXIO. TypeScript types auto-generated via ts-rs.
//! Regenerate: `npm run typegen` or `cargo test -p axio export_bindings`

use branded::Branded;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use ts_rs::TS;

// Branded ID types for type safety

#[derive(Branded, TS)]
#[branded(serde)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
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
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum AXValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct Bounds {
    pub position: Position,
    pub size: Size,
}

// ARIA role subset

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
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
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct AXElement {
    // Identity
    pub id: ElementId,
    pub window_id: String,

    // Relationships (flat, not nested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<ElementId>,
    /// None = children not yet discovered, Some([]) = no children
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_ids: Option<Vec<ElementId>>,

    // Attributes
    pub role: AXRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<AXValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Window info from x-win. Root element ID populated from WindowRoot event.
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
    /// Root element ID (client looks up from element registry)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_element_id: Option<ElementId>,
}

// Events

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum ServerEvent {
    /// Window list changed
    WindowUpdate(Vec<AXWindow>),
    /// Elements discovered/updated (batch)
    Elements(Vec<AXElement>),
    /// Element destroyed
    ElementDestroyed { element_id: ElementId },
    /// Mouse position
    MousePosition { x: f64, y: f64 },
}
