use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{collections::HashSet, sync::Arc};
use tauri::Manager; // For get_webview_window
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::axio::AXNode;
use crate::node_watcher::NodeWatcher;
use crate::platform::{get_children_by_element_id, write_to_element_by_id};
use crate::windows::{WindowInfo, WindowUpdatePayload};

// ============================================================================
// Message Type Constants
// ============================================================================

pub mod msg_types {
    // Server -> Client event types
    pub const WINDOW_FOCUSED: &str = "window_focused";
    pub const TREE_CHANGED: &str = "tree_changed";
    pub const VALUE_CHANGED: &str = "value_changed";

    // Client -> Server request types
    pub const GET_ACCESSIBILITY_TREE: &str = "get_accessibility_tree";
    pub const GET_CHILDREN: &str = "get_children";
    pub const WRITE_TO_ELEMENT: &str = "write_to_element";
    pub const SET_CLICKTHROUGH: &str = "set_clickthrough";
    pub const WATCH_NODE: &str = "watch_node";
    pub const UNWATCH_NODE: &str = "unwatch_node";

    // Server -> Client response types
    pub const IDENTIFICATION_RECEIVED: &str = "identification_received";
    pub const ACCESSIBILITY_TREE_RESPONSE: &str = "accessibility_tree_response";
    pub const GET_CHILDREN_RESPONSE: &str = "get_children_response";
    pub const ACCESSIBILITY_WRITE_RESPONSE: &str = "accessibility_write_response";
    pub const SET_CLICKTHROUGH_RESPONSE: &str = "set_clickthrough_response";
    pub const WATCH_NODE_RESPONSE: &str = "watch_node_response";
    pub const UNWATCH_NODE_RESPONSE: &str = "unwatch_node_response";
}

// ============================================================================
// Server Event Types (Push notifications from backend)
// ============================================================================

/// Sent when a window gains focus (should trigger frontend to fetch tree if needed)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowFocusedEvent {
    pub event_type: String, // "window_focused"
    pub window: WindowInfo,
}

/// Sent when the accessibility tree structure changes for the focused window
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeChangedEvent {
    pub event_type: String, // "tree_changed"
    pub pid: u32,
    pub tree: AXNode,
}

/// Sent when a specific value changes in the tree (future: for fine-grained updates)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValueChangedEvent {
    pub event_type: String, // "value_changed"
    pub pid: u32,
    pub path: Vec<usize>,
    pub new_value: String, // TODO: Use AXValue when we add incremental updates
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientIdentification {
    bottom_left_x: i32,
    bottom_left_y: i32,
    window_width: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageType {
    msg_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccessibilityTreeRequest {
    msg_type: String,
    #[serde(default)]
    window_id: Option<String>, // NEW: Preferred way - uses cached window element
    #[serde(default)]
    pid: Option<u32>, // LEGACY: Falls back to app element
    #[serde(default = "default_max_depth")]
    max_depth: usize,
    #[serde(default = "default_max_children_per_level")]
    max_children_per_level: usize,
}

fn default_max_depth() -> usize {
    50
}

fn default_max_children_per_level() -> usize {
    2000
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetChildrenRequest {
    msg_type: String,
    pid: u32,
    #[serde(default)]
    element_id: Option<String>, // NEW: Preferred (uses ElementRegistry)
    #[serde(default)]
    path: Option<Vec<usize>>, // LEGACY: Fallback (navigates by path)
    #[serde(default = "default_max_depth")]
    max_depth: usize,
    #[serde(default = "default_max_children_per_level")]
    max_children_per_level: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccessibilityWriteRequest {
    msg_type: String,
    #[serde(default)]
    pid: Option<u32>, // Optional for backwards compatibility
    #[serde(default)]
    element_id: Option<String>, // NEW: Preferred (uses ElementRegistry)
    #[serde(default)]
    element_path: Option<Vec<usize>>, // LEGACY: Fallback (navigates by path)
    text: String,
}

/// Generic WebSocket response wrapper with common fields
#[derive(Debug, Serialize, Deserialize)]
struct WsResponse<T> {
    msg_type: String,
    success: bool,
    #[serde(flatten)]
    data: T,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdentificationData {
    window_id: Option<String>,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityTreeData {
    pid: u32,
    tree: Option<AXNode>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetChildrenData {
    pid: u32,
    path: Vec<usize>,
    children: Option<Vec<AXNode>>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityWriteData {
    pid: u32,
    message: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetClickthroughRequest {
    msg_type: String,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SetClickthroughData {
    enabled: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchNodeRequest {
    msg_type: String,
    pid: u32,
    element_id: Option<String>, // NEW: Preferred
    #[serde(default)]
    path: Option<Vec<usize>>, // LEGACY: Deprecated
    node_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WatchNodeData {
    node_id: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnwatchNodeRequest {
    msg_type: String,
    pid: u32,
    element_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UnwatchNodeData {}

// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    pub connected_windows: Arc<RwLock<HashSet<String>>>, // Set of window IDs with connected clients
    pub current_windows: Arc<RwLock<Vec<WindowInfo>>>,
    pub app_handle: tauri::AppHandle,
    pub node_watcher: Arc<NodeWatcher>,
}

impl WebSocketState {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        let (sender, _) = broadcast::channel(1000);
        let sender_arc = Arc::new(sender);

        // Create node watcher with the sender
        let node_watcher = NodeWatcher::new(sender_arc.clone());

        Self {
            sender: sender_arc,
            connected_windows: Arc::new(RwLock::new(HashSet::new())),
            current_windows: Arc::new(RwLock::new(Vec::new())),
            app_handle,
            node_watcher,
        }
    }

    pub fn broadcast(&self, data: &WindowUpdatePayload) {
        if let Ok(json) = serde_json::to_string(data) {
            let _ = self.sender.send(json);
        }
    }

    // Store current windows for immediate matching
    pub async fn update_windows(&self, windows: &[WindowInfo]) {
        let mut current_windows = self.current_windows.write().await;
        *current_windows = windows.to_vec();
    }

    // Find best matching window using distance-based scoring
    fn find_best_match(
        &self,
        client_coords: &ClientIdentification,
        windows: &[WindowInfo],
    ) -> Option<String> {
        if windows.is_empty() {
            return None;
        }

        let max_distance = 150.0;
        let mut best_match: Option<(&WindowInfo, f64)> = None;

        for window in windows {
            // Calculate window's bottom-left coordinates
            let window_bottom_x = window.x;
            let window_bottom_y = window.y + window.h;

            // Position distance (Euclidean)
            let x_diff = (window_bottom_x - client_coords.bottom_left_x) as f64;
            let y_diff = (window_bottom_y - client_coords.bottom_left_y) as f64;
            let position_distance = (x_diff * x_diff + y_diff * y_diff).sqrt();

            // Width distance (weighted less than position)
            let width_diff = (window.w - client_coords.window_width).abs() as f64;
            let width_distance = width_diff * 0.5;

            let total_distance = position_distance + width_distance;

            // Update best match if this is better
            match best_match {
                None if total_distance <= max_distance => {
                    best_match = Some((window, total_distance));
                }
                Some((_, current_best))
                    if total_distance < current_best && total_distance <= max_distance =>
                {
                    best_match = Some((window, total_distance));
                }
                _ => {}
            }
        }

        best_match.map(|(window, _)| window.id.clone())
    }
}

pub async fn start_websocket_server(ws_state: WebSocketState) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(ws_state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030")
        .await
        .expect("Failed to bind WebSocket server");

    println!(
        "{}",
        "WebSocket server: ws://127.0.0.1:3030/ws".bright_black()
    );
    axum::serve(listener, app)
        .await
        .expect("WebSocket server failed");
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(ws_state): State<WebSocketState>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, ws_state))
}

async fn handle_websocket(mut socket: WebSocket, ws_state: WebSocketState) {
    let mut rx = ws_state.sender.subscribe();

    println!("{}", "Client connected".bright_black());

    // Send initial window state immediately
    {
        let current_windows = ws_state.current_windows.read().await;
        let window_update = WindowUpdatePayload {
            windows: current_windows.clone(),
        };
        if let Ok(msg_json) = serde_json::to_string(&window_update) {
            let _ = socket.send(Message::Text(msg_json)).await;
            println!(
                "📡 Sent initial window state ({} windows) to client",
                current_windows.len()
            );
        }
    }

    let mut current_window_id: Option<String> = None;

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(e) = handle_client_message(&text, &mut current_window_id, &ws_state, &mut socket).await {
                            println!("❌ Error handling message: {}", e);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        println!("❌ WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            // Send window updates to client
            update = rx.recv() => {
                match update {
                    Ok(data) => {
                        if socket.send(Message::Text(data)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // Clear all node watches to prevent stale observers on reconnect
    ws_state.node_watcher.clear_all();

    // Remove client from tracking if it was identified
    if let Some(window_id) = current_window_id {
        let mut connected_windows = ws_state.connected_windows.write().await;
        connected_windows.remove(&window_id);
        println!("🔌 Client disconnected: window {}", window_id);
    } else {
        println!("🔌 Unidentified client session ended");
    }
}

async fn handle_client_message(
    message: &str,
    current_window_id: &mut Option<String>,
    ws_state: &WebSocketState,
    socket: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Silently handle messages - no logging for normal operations

    // Try to parse as ClientIdentification first (doesn't have msg_type field)
    if let Ok(identification) = serde_json::from_str::<ClientIdentification>(message) {
        println!(
            "🎯 Client requesting identification at ({}, {}) width: {}px",
            identification.bottom_left_x, identification.bottom_left_y, identification.window_width
        );

        // Try to match immediately with current windows
        let window_id = {
            let current_windows = ws_state.current_windows.read().await;
            ws_state.find_best_match(&identification, &current_windows)
        };

        if let Some(ref wid) = window_id {
            // Store the window ID for this session
            *current_window_id = Some(wid.clone());

            // Track this client by window ID
            let mut connected_windows = ws_state.connected_windows.write().await;
            connected_windows.insert(wid.clone());

            println!("✅ Client matched to window {}", wid);
        } else {
            println!("❌ No match found for client");
        }

        let response = WsResponse {
            msg_type: msg_types::IDENTIFICATION_RECEIVED.to_string(),
            success: window_id.is_some(),
            data: IdentificationData {
                window_id: window_id.clone(),
                message: if window_id.is_some() {
                    format!("✅ Window matched!")
                } else {
                    format!("❌ No matching window found")
                },
            },
        };

        let response_json = serde_json::to_string(&response)?;
        socket.send(Message::Text(response_json)).await.ok();
        return Ok(());
    }

    // Extract msg_type for dispatching
    let msg_type_parsed = serde_json::from_str::<MessageType>(message)?;
    let msg_type = msg_type_parsed.msg_type.as_deref().unwrap_or("");

    match msg_type {
        msg_types::WRITE_TO_ELEMENT => {
            let write_request = serde_json::from_str::<AccessibilityWriteRequest>(message)?;
            println!(
                "✅ Successfully parsed as AccessibilityWriteRequest: {:?}",
                write_request
            );

            // Attempt to write to the element - prefer element_id over path
            let (success, message, error) = if let Some(ref element_id) = write_request.element_id {
                // NEW: Use element_id (direct registry lookup)
                println!("✍️ Writing via element_id: {}", element_id);
                match write_to_element_by_id(element_id, &write_request.text) {
                    Ok(_) => (
                        true,
                        format!("Successfully wrote '{}' to element", write_request.text),
                        None,
                    ),
                    Err(e) => (false, "Failed to write to element".to_string(), Some(e)),
                }
            } else {
                (
                    false,
                    "Neither element_id nor (pid + path) provided".to_string(),
                    Some("Invalid write request".to_string()),
                )
            };

            let response = WsResponse {
                msg_type: msg_types::ACCESSIBILITY_WRITE_RESPONSE.to_string(),
                success,
                data: AccessibilityWriteData {
                    pid: write_request.pid.unwrap_or(0),
                    message,
                    error,
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::GET_CHILDREN => {
            let get_children_req = serde_json::from_str::<GetChildrenRequest>(message)?;

            // Get children of the specified node by element_id
            let (success, children, error) = if let Some(ref element_id) =
                get_children_req.element_id
            {
                println!(
                    "👶 Client requesting children for element_id: {} (max_depth: {}, max_children: {})",
                    element_id, get_children_req.max_depth, get_children_req.max_children_per_level
                );

                match get_children_by_element_id(
                    get_children_req.pid,
                    element_id,
                    get_children_req.max_depth,
                    get_children_req.max_children_per_level,
                ) {
                    Ok(ch) => (true, Some(ch), None),
                    Err(e) => (false, None, Some(e)),
                }
            } else {
                (false, None, Some("element_id not provided".to_string()))
            };

            let response = WsResponse {
                msg_type: msg_types::GET_CHILDREN_RESPONSE.to_string(),
                success,
                data: GetChildrenData {
                    pid: get_children_req.pid,
                    path: get_children_req.path.unwrap_or_default(),
                    children,
                    error,
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            if success {
                println!("✅ Sent children");
            } else {
                println!("❌ Failed to get children");
            }
        }

        msg_types::GET_ACCESSIBILITY_TREE => {
            let ax_request = serde_json::from_str::<AccessibilityTreeRequest>(message)?;
            println!("🌳 Parsed as AccessibilityTreeRequest: {:?}", ax_request);

            // Get accessibility tree by window_id (uses cached window element)
            let (success, tree, error, pid) = if let Some(ref window_id) = ax_request.window_id {
                println!(
                    "🌳 Client requesting tree for window_id: {} (max_depth: {}, max_children: {})",
                    window_id, ax_request.max_depth, ax_request.max_children_per_level
                );

                // Use cached window element as root
                match crate::platform::get_ax_tree_by_window_id(
                    window_id,
                    ax_request.max_depth,
                    ax_request.max_children_per_level,
                    true, // Load full tree
                ) {
                    Ok(ax_tree) => {
                        let pid = ax_tree.pid;
                        (true, Some(ax_tree), None, pid)
                    }
                    Err(e) => (false, None, Some(e), 0),
                }
            } else {
                (false, None, Some("window_id not provided".to_string()), 0)
            };

            let response = WsResponse {
                msg_type: msg_types::ACCESSIBILITY_TREE_RESPONSE.to_string(),
                success,
                data: AccessibilityTreeData {
                    pid,
                    tree,
                    error: error.clone(),
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            if success {
                if ax_request.window_id.is_some() {
                    println!(
                        "✅ Sent accessibility tree for window_id: {:?}",
                        ax_request.window_id
                    );
                } else {
                    println!("✅ Sent accessibility tree for PID {}", pid);
                }
            } else {
                println!("❌ Failed to get accessibility tree: {:?}", error);
            }
        }

        msg_types::SET_CLICKTHROUGH => {
            let clickthrough_req = serde_json::from_str::<SetClickthroughRequest>(message)?;

            // Set clickthrough state
            let (success, error) = match ws_state.app_handle.get_webview_window("main") {
                Some(window) => match window.set_ignore_cursor_events(clickthrough_req.enabled) {
                    Ok(_) => (true, None),
                    Err(e) => (false, Some(e.to_string())),
                },
                None => (false, Some("Main window not found".to_string())),
            };

            let response = WsResponse {
                msg_type: msg_types::SET_CLICKTHROUGH_RESPONSE.to_string(),
                success,
                data: SetClickthroughData {
                    enabled: if success {
                        clickthrough_req.enabled
                    } else {
                        false
                    },
                    error,
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::WATCH_NODE => {
            let watch_req = serde_json::from_str::<WatchNodeRequest>(message)?;

            let result = if let Some(ref element_id) = watch_req.element_id {
                ws_state.node_watcher.watch_node_by_id(
                    watch_req.pid,
                    element_id.clone(),
                    watch_req.node_id.clone(),
                )
            } else {
                Err("element_id not provided".to_string())
            };

            let (success, error) = match result {
                Ok(_) => (true, None),
                Err(e) => {
                    println!(
                        "{}",
                        format!("ERROR: Watch failed for {}: {}", watch_req.node_id, e).red()
                    );
                    (false, Some(e))
                }
            };

            let response = WsResponse {
                msg_type: msg_types::WATCH_NODE_RESPONSE.to_string(),
                success,
                data: WatchNodeData {
                    node_id: watch_req.node_id,
                    error,
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::UNWATCH_NODE => {
            let unwatch_req = serde_json::from_str::<UnwatchNodeRequest>(message)?;
            println!(
                "🚫 Client requesting to unwatch node: PID {} element_id {}",
                unwatch_req.pid, unwatch_req.element_id
            );

            // Stop watching the node
            ws_state
                .node_watcher
                .unwatch_node_by_id(unwatch_req.pid, unwatch_req.element_id);

            let response = WsResponse {
                msg_type: msg_types::UNWATCH_NODE_RESPONSE.to_string(),
                success: true,
                data: UnwatchNodeData {},
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        _ => {
            println!("❓ Unrecognized message type: {}", msg_type);
        }
    }

    Ok(())
}
