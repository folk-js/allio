/**
 * Protocol Types
 *
 * WebSocket message definitions for client-server communication.
 * Only contains request/response types - AXIO types are in axio.rs.
 */
use serde::{Deserialize, Serialize};

use crate::axio::{AXNode, AXValue};

// ============================================================================
// Push Events (Server -> Client, not request/response)
// ============================================================================

/// Sent when a window's root accessibility node is available or updated
#[derive(Debug, Serialize, Deserialize)]
pub struct WindowRootUpdate {
    pub msg_type: String,
    pub window_id: String,
    pub root: AXNode,
}

// ============================================================================
// Request Types (Client -> Server)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct GetChildrenRequest {
    pub element_id: String,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_max_children")]
    pub max_children_per_level: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetElementValueRequest {
    pub element_id: String,
    pub value: AXValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClickElementRequest {
    pub element_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchNodeRequest {
    pub element_id: String,
    pub node_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnwatchNodeRequest {
    pub element_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetClickthroughRequest {
    pub enabled: bool,
}

// ============================================================================
// Response Types (Server -> Client)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct GetChildrenResponse {
    pub msg_type: String,
    pub success: bool,
    pub children: Option<Vec<AXNode>>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetElementValueResponse {
    pub msg_type: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClickElementResponse {
    pub msg_type: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchNodeResponse {
    pub msg_type: String,
    pub success: bool,
    pub node_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnwatchNodeResponse {
    pub msg_type: String,
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetClickthroughResponse {
    pub msg_type: String,
    pub success: bool,
    pub enabled: bool,
    pub error: Option<String>,
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
