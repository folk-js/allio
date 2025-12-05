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

// === Action types ===

/// Platform-agnostic action enum.
/// Maps to macOS kAX*Action constants and Windows UIA patterns.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum AXAction {
    /// Primary activation (click, press). macOS: AXPress, Windows: Invoke
    Press,
    /// Show context menu. macOS: AXShowMenu
    ShowMenu,
    /// Increase value. macOS: AXIncrement, Windows: RangeValue
    Increment,
    /// Decrease value. macOS: AXDecrement, Windows: RangeValue
    Decrement,
    /// Confirm/submit. macOS: AXConfirm
    Confirm,
    /// Cancel operation. macOS: AXCancel
    Cancel,
    /// Bring to front. macOS: AXRaise, Windows: SetFocus
    Raise,
    /// Pick from list. macOS: AXPick
    Pick,
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
    /// Available actions for this element
    pub actions: Vec<AXAction>,
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
    pub active_window: Option<String>,
    pub focused_window: Option<String>,
    pub focused_element: Option<AXElement>,
    pub selection: Option<Selection>,
    /// Window IDs in z-order (front to back)
    pub depth_order: Vec<WindowId>,
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
        window: AXWindow,
        depth_order: Vec<WindowId>,
    },

    // Element lifecycle (from RPC, watches)
    #[serde(rename = "element:added")]
    ElementAdded { element: AXElement },
    #[serde(rename = "element:changed")]
    ElementChanged { element: AXElement },
    #[serde(rename = "element:removed")]
    ElementRemoved { element: AXElement },

    // Window focus (from polling)
    #[serde(rename = "focus:changed")]
    FocusChanged { window_id: Option<WindowId> },
    #[serde(rename = "active:changed")]
    ActiveChanged { window_id: WindowId },

    // Element focus (from Tier 1 app-level observer)
    #[serde(rename = "focus:element")]
    FocusElement {
        window_id: String,
        element_id: ElementId,
        element: AXElement,
        previous_element_id: Option<ElementId>,
    },

    // Text selection (from Tier 1 app-level observer)
    #[serde(rename = "selection:changed")]
    SelectionChanged {
        window_id: String,
        element_id: ElementId,
        text: String,
        range: Option<TextRange>,
    },

    // Input tracking
    #[serde(rename = "mouse:position")]
    MousePosition { x: f64, y: f64 },
}

/// Text selection range within an element
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub struct TextRange {
    pub start: u32,
    pub length: u32,
}
