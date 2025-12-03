/**
 * Protocol Types
 *
 * Complete WebSocket message protocol for client-server communication.
 * All communication types are defined here for clarity and type safety.
 *
 * Design:
 * - Request/Response pairs co-located in modules
 * - ClientMessage and ServerMessage enums for serialization
 * - Simple, clear structure with compile-time type safety
 */
use serde::{Deserialize, Serialize};

use crate::axio::{AXNode, AXValue};
use crate::windows::WindowInfo;

// ============================================================================
// Request/Response Pair Modules
// ============================================================================

/// Get children of an accessibility element
pub mod get_children {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub element_id: String,
        #[serde(default = "super::default_max_depth")]
        pub max_depth: usize,
        #[serde(default = "super::default_max_children")]
        pub max_children_per_level: usize,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub children: Option<Vec<AXNode>>,
        pub error: Option<String>,
    }
}

/// Write text to an accessibility element
pub mod write_to_element {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub element_id: String,
        pub text: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub error: Option<String>,
    }
}

/// Click an accessibility element
pub mod click_element {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub element_id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub error: Option<String>,
    }
}

/// Enable/disable click-through on overlay window
pub mod set_clickthrough {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub enabled: bool,
        pub error: Option<String>,
    }
}

/// Start watching an element for changes
pub mod watch_node {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub element_id: String,
        pub node_id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub node_id: String,
        pub error: Option<String>,
    }
}

/// Stop watching an element
pub mod unwatch_node {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub element_id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
    }
}

/// Get accessibility element at screen position
pub mod get_element_at_position {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Request {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub x: f64,
        pub y: f64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub success: bool,
        pub element: Option<AXNode>,
        pub error: Option<String>,
    }
}

// ============================================================================
// Client -> Server Messages
// ============================================================================

/// All messages that can be sent from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    GetChildren(get_children::Request),
    WriteToElement(write_to_element::Request),
    ClickElement(click_element::Request),
    SetClickthrough(set_clickthrough::Request),
    WatchNode(watch_node::Request),
    UnwatchNode(unwatch_node::Request),
    GetElementAtPosition(get_element_at_position::Request),
}

// ============================================================================
// Server -> Client Messages
// ============================================================================

/// All messages that can be sent from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    // Push Events (not request/response)
    WindowUpdate { windows: Vec<WindowInfo> },
    WindowRootUpdate { window_id: String, root: AXNode },
    MousePosition { x: f64, y: f64 },
    ElementUpdate { update: ElementUpdate },

    // Response Messages (paired with requests above)
    GetChildrenResponse(get_children::Response),
    WriteToElementResponse(write_to_element::Response),
    ClickElementResponse(click_element::Response),
    SetClickthroughResponse(set_clickthrough::Response),
    WatchNodeResponse(watch_node::Response),
    UnwatchNodeResponse(unwatch_node::Response),
    GetElementAtPositionResponse(get_element_at_position::Response),
}

// ============================================================================
// Element Update Types (Server Push Events)
// ============================================================================

/// Update events for accessibility elements
/// Moved from axio.rs - this is part of the protocol, not core AXIO types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "update_type", rename_all = "PascalCase")]
pub enum ElementUpdate {
    ValueChanged { element_id: String, value: AXValue },
    LabelChanged { element_id: String, label: String },
    ElementDestroyed { element_id: String },
}

// ============================================================================
// Helper Functions
// ============================================================================

fn default_max_depth() -> usize {
    50
}

fn default_max_children() -> usize {
    2000
}
