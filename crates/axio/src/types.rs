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

impl AxioError {
    #[allow(dead_code)]
    pub fn ax<E: std::error::Error>(e: E) -> Self {
        Self::AccessibilityError(e.to_string())
    }
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

/// Accessibility element node.
/// Hydration: None = "not fetched", Some = "value when queried" (may be stale).
/// Only `id` and `role` are always present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub struct AXNode {
    pub id: ElementId,
    pub role: AXRole,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<ElementId>,
    /// Platform-specific subtype (or native role name for Unknown)
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
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<AXNode>>,
}

/// Window with accessibility tree root (root populated client-side from WindowRoot event).
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

// Events

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "update_type", rename_all = "PascalCase")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum ElementUpdate {
    ValueChanged { element_id: String, value: AXValue },
    LabelChanged { element_id: String, label: String },
    ElementDestroyed { element_id: String },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
#[ts(export, export_to = "packages/axio-client/src/types/")]
pub enum ServerEvent {
    WindowUpdate(Vec<AXWindow>),
    WindowRoot { window_id: String, root: AXNode },
    MousePosition { x: f64, y: f64 },
    ElementUpdate(ElementUpdate),
}
