//! Core types for AXIO. TypeScript types auto-generated via ts-rs.
//! Regenerate: `npm run typegen` or `cargo test -p axio export_bindings`

use branded::Branded;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use ts_rs::TS;

// === Branded ID types ===

#[derive(Branded, TS)]
#[branded(serde)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct ElementId(pub String);

impl Borrow<str> for ElementId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Branded, TS)]
#[branded(serde)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct WindowId(pub String);

impl Borrow<str> for WindowId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

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

// === Value and geometry types ===

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
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

// === ARIA role subset ===

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

// === Core types ===

/// Window info from x-win + accessibility.
/// Note: Windows don't have children - elements reference windows via window_id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct AXWindow {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub bounds: Bounds,
    pub focused: bool,
    pub process_id: u32,
}

/// The unified element type - stored in registry and returned from API.
/// Flat structure: children are IDs, not nested. Trees derived client-side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct AXElement {
    pub id: ElementId,
    /// Window this element belongs to
    pub window_id: String,
    /// None = root element of window
    pub parent_id: Option<ElementId>,
    /// Child element IDs. None = not yet fetched, Some([]) = no children
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

// === Events ===
// Events notify clients when the Registry changes.
// Any registry change emits an event, regardless of trigger.

/// Initial state sent on connection
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct SyncInit {
    pub windows: Vec<AXWindow>,
    pub elements: Vec<AXElement>,
    pub active_window: Option<String>,
    pub focused_window: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum ServerEvent {
    // Initial sync (on connection)
    #[serde(rename = "sync:init")]
    SyncInit(SyncInit),

    // Window lifecycle (from polling)
    #[serde(rename = "window:added")]
    WindowAdded { window: AXWindow },
    #[serde(rename = "window:changed")]
    WindowChanged { window: AXWindow },
    #[serde(rename = "window:removed")]
    WindowRemoved { window: AXWindow },

    // Element lifecycle (from RPC, watches)
    #[serde(rename = "element:added")]
    ElementAdded { element: AXElement },
    #[serde(rename = "element:changed")]
    ElementChanged { element: AXElement },
    #[serde(rename = "element:removed")]
    ElementRemoved { element: AXElement },

    // Focus (from polling)
    #[serde(rename = "focus:changed")]
    FocusChanged { window_id: Option<WindowId> },
    #[serde(rename = "active:changed")]
    ActiveChanged { window_id: WindowId },

    // Input tracking
    #[serde(rename = "mouse:position")]
    MousePosition { x: f64, y: f64 },
}
