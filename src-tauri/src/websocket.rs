use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{collections::HashSet, sync::Arc};
use tauri::Manager; // For get_webview_window
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::axio::AXNode;
use crate::node_watcher::NodeWatcher;
use crate::platform::{get_ax_tree_by_pid, write_to_element};
use crate::windows::{WindowInfo, WindowUpdatePayload};

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
#[serde(deny_unknown_fields)]
struct AccessibilityTreeRequest {
    msg_type: String,
    pid: u32,
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
    path: Vec<usize>,
    #[serde(default = "default_max_depth")]
    max_depth: usize,
    #[serde(default = "default_max_children_per_level")]
    max_children_per_level: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccessibilityWriteRequest {
    msg_type: String,
    pid: u32,
    element_path: Vec<usize>,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerResponse {
    msg_type: String,
    window_id: Option<String>,
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityTreeResponse {
    msg_type: String,
    pid: u32,
    success: bool,
    tree: Option<AXNode>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetChildrenResponse {
    msg_type: String,
    pid: u32,
    path: Vec<usize>,
    success: bool,
    children: Option<Vec<AXNode>>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityWriteResponse {
    msg_type: String,
    pid: u32,
    success: bool,
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
struct SetClickthroughResponse {
    msg_type: String,
    success: bool,
    enabled: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchNodeRequest {
    msg_type: String,
    pid: u32,
    path: Vec<usize>,
    node_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WatchNodeResponse {
    msg_type: String,
    success: bool,
    node_id: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnwatchNodeRequest {
    msg_type: String,
    pid: u32,
    path: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UnwatchNodeResponse {
    msg_type: String,
    success: bool,
}

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

    /// Automatically push accessibility tree for a window when it gains focus
    /// NOTE: This is currently unused - trees are fetched on-demand by the frontend
    #[allow(dead_code)]
    pub fn push_tree_for_window(&self, pid: u32) {
        // Get tree with default limits, fully loaded
        let tree_result = get_ax_tree_by_pid(pid, 100, 5000, true);

        match tree_result {
            Ok(tree) => {
                let event = TreeChangedEvent {
                    event_type: "tree_changed".to_string(),
                    pid,
                    tree,
                };

                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = self.sender.send(json);
                    println!("📤 Pushed tree for PID {}", pid);
                }
            }
            Err(e) => {
                println!("❌ Failed to get tree for PID {}: {}", pid, e);
            }
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

    println!("🔌 WebSocket server running on ws://127.0.0.1:3030/ws");
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

    println!("🔗 Client session started");

    // Send overlay PID to frontend so it can filter itself out
    let overlay_pid_msg = serde_json::json!({
        "overlay_pid": std::process::id()
    });
    if let Ok(msg_json) = serde_json::to_string(&overlay_pid_msg) {
        let _ = socket.send(Message::Text(msg_json)).await;
        // println!("📡 Sent overlay PID {} to client", std::process::id());
    }

    // Send initial window state immediately
    {
        let current_windows = ws_state.current_windows.read().await;
        // Convert WindowInfo to AXNode (filter out any that fail to convert)
        let window_nodes: Vec<AXNode> = current_windows
            .iter()
            .filter_map(|w| w.to_ax_node())
            .collect();
        let window_update = WindowUpdatePayload {
            windows: window_nodes,
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
    println!("📨 Received WebSocket message: {}", message);
    println!("🔄 Starting handler chain...");

    // Debug: Try to parse as WatchNodeRequest to see the error
    if message.contains("watch_node") {
        println!("🔍 Message contains 'watch_node', attempting parse...");
        match serde_json::from_str::<WatchNodeRequest>(message) {
            Ok(req) => println!("  ✅ Parsed successfully: {:?}", req),
            Err(e) => println!("  ❌ Parse failed: {}", e),
        }
    }

    // Try to parse as ClientIdentification first
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

        let response = ServerResponse {
            msg_type: "identification_received".to_string(),
            window_id: window_id.clone(),
            success: window_id.is_some(),
            message: if window_id.is_some() {
                format!("✅ Window matched!")
            } else {
                format!("❌ No matching window found")
            },
        };

        let response_json = serde_json::to_string(&response)?;
        socket.send(Message::Text(response_json)).await.ok();
    }
    // Try to parse as AccessibilityWriteRequest FIRST (more specific)
    else if let Ok(write_request) = serde_json::from_str::<AccessibilityWriteRequest>(message) {
        println!(
            "✅ Successfully parsed as AccessibilityWriteRequest: {:?}",
            write_request
        );
        if write_request.msg_type == "write_to_element" {
            // Reduced logging for live updates to avoid spam

            // Attempt to write to the element
            let (success, message, error) = match write_to_element(
                write_request.pid,
                &write_request.element_path,
                &write_request.text,
            ) {
                Ok(_) => (
                    true,
                    format!("Successfully wrote '{}' to element", write_request.text),
                    None,
                ),
                Err(e) => (false, "Failed to write to element".to_string(), Some(e)),
            };

            let response = AccessibilityWriteResponse {
                msg_type: "accessibility_write_response".to_string(),
                pid: write_request.pid,
                success,
                message,
                error,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            // Reduced logging for live updates
        }
    }
    // Try to parse as GetChildrenRequest
    else if let Ok(get_children_req) = serde_json::from_str::<GetChildrenRequest>(message) {
        if get_children_req.msg_type == "get_children" {
            println!(
                "👶 Client requesting children for PID: {} path: {:?}",
                get_children_req.pid, get_children_req.path
            );

            // Get children of the specified node
            let (success, children, error) = match crate::platform::get_children_by_path(
                get_children_req.pid,
                &get_children_req.path,
                get_children_req.max_depth,
                get_children_req.max_children_per_level,
            ) {
                Ok(ch) => (true, Some(ch), None),
                Err(e) => (false, None, Some(e)),
            };

            let response = GetChildrenResponse {
                msg_type: "get_children_response".to_string(),
                pid: get_children_req.pid,
                path: get_children_req.path,
                success,
                children,
                error,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            if success {
                println!("✅ Sent children for PID {}", get_children_req.pid);
            } else {
                println!("❌ Failed to get children for PID {}", get_children_req.pid);
            }
        }
    }
    // Try to parse as AccessibilityTreeRequest (less specific)
    else if let Ok(ax_request) = serde_json::from_str::<AccessibilityTreeRequest>(message) {
        println!("🌳 Parsed as AccessibilityTreeRequest: {:?}", ax_request);
        if ax_request.msg_type == "get_accessibility_tree" {
            println!(
                "🌳 Client requesting accessibility tree for PID: {} (max_depth: {}, max_children: {})",
                ax_request.pid, ax_request.max_depth, ax_request.max_children_per_level
            );

            // Get the accessibility tree with configurable limits
            // load_children=true to maintain backward compatibility (full tree)
            let (success, tree, error) = match get_ax_tree_by_pid(
                ax_request.pid,
                ax_request.max_depth,
                ax_request.max_children_per_level,
                true, // Load full tree
            ) {
                Ok(ax_tree) => (true, Some(ax_tree), None),
                Err(e) => (false, None, Some(e)),
            };

            let response = AccessibilityTreeResponse {
                msg_type: "accessibility_tree_response".to_string(),
                pid: ax_request.pid,
                success,
                tree,
                error,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            if success {
                println!("✅ Sent accessibility tree for PID {}", ax_request.pid);
            } else {
                println!(
                    "❌ Failed to get accessibility tree for PID {}",
                    ax_request.pid
                );
            }
        } else {
            println!(
                "🤔 AccessibilityTreeRequest with unexpected type: {}",
                ax_request.msg_type
            );
        }
    }
    // Try to parse as SetClickthroughRequest
    else if let Ok(clickthrough_req) = serde_json::from_str::<SetClickthroughRequest>(message) {
        if clickthrough_req.msg_type == "set_clickthrough" {
            println!(
                "🖱️ Client requesting clickthrough: {}",
                clickthrough_req.enabled
            );

            // Set clickthrough state
            let (success, error) = match ws_state.app_handle.get_webview_window("main") {
                Some(window) => match window.set_ignore_cursor_events(clickthrough_req.enabled) {
                    Ok(_) => {
                        println!(
                            "✅ Clickthrough {} for window",
                            if clickthrough_req.enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                        (true, None)
                    }
                    Err(e) => {
                        println!("❌ Failed to set clickthrough: {}", e);
                        (false, Some(e.to_string()))
                    }
                },
                None => {
                    println!("❌ Main window not found");
                    (false, Some("Main window not found".to_string()))
                }
            };

            let response = SetClickthroughResponse {
                msg_type: "set_clickthrough_response".to_string(),
                success,
                enabled: if success {
                    clickthrough_req.enabled
                } else {
                    false
                },
                error,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }
    }
    // Try to parse as WatchNodeRequest
    else if let Ok(watch_req) = serde_json::from_str::<WatchNodeRequest>(message) {
        println!(
            "🔍 Parsed as WatchNodeRequest: msg_type={}",
            watch_req.msg_type
        );
        if watch_req.msg_type == "watch_node" {
            println!(
                "👁️  Client requesting to watch node: PID {} path {:?} ID {}",
                watch_req.pid, watch_req.path, watch_req.node_id
            );

            // Start watching the node
            let result = ws_state.node_watcher.watch_node(
                watch_req.pid,
                watch_req.path,
                watch_req.node_id.clone(),
            );

            let (success, error) = match result {
                Ok(_) => {
                    println!("✅ Watch succeeded for node {}", watch_req.node_id);
                    (true, None)
                }
                Err(e) => {
                    println!("❌ Watch failed for node {}: {}", watch_req.node_id, e);
                    (false, Some(e))
                }
            };

            let response = WatchNodeResponse {
                msg_type: "watch_node_response".to_string(),
                success,
                node_id: watch_req.node_id,
                error,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }
    }
    // Try to parse as UnwatchNodeRequest
    else if let Ok(unwatch_req) = serde_json::from_str::<UnwatchNodeRequest>(message) {
        if unwatch_req.msg_type == "unwatch_node" {
            println!(
                "🚫 Client requesting to unwatch node: PID {} path {:?}",
                unwatch_req.pid, unwatch_req.path
            );

            // Stop watching the node
            ws_state
                .node_watcher
                .unwatch_node(unwatch_req.pid, unwatch_req.path);

            let response = UnwatchNodeResponse {
                msg_type: "unwatch_node_response".to_string(),
                success: true,
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }
    }
    // Catch-all for unrecognized messages
    else {
        println!("❓ Unrecognized message format: {}", message);
    }

    Ok(())
}
