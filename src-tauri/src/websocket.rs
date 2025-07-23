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
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::accessibility::{walk_app_tree_by_pid, write_to_element_by_pid_and_path, UITreeNode};
use crate::windows::{WindowInfo, WindowUpdatePayload};

#[derive(Debug, Serialize, Deserialize)]
struct ClientIdentification {
    bottom_left_x: i32,
    bottom_left_y: i32,
    window_width: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityTreeRequest {
    #[serde(rename = "type")]
    msg_type: String,
    pid: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityWriteRequest {
    #[serde(rename = "type")]
    msg_type: String,
    pid: u32,
    element_path: Vec<usize>,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerResponse {
    #[serde(rename = "type")]
    msg_type: String,
    window_id: Option<String>,
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityTreeResponse {
    #[serde(rename = "type")]
    msg_type: String,
    pid: u32,
    success: bool,
    tree: Option<UITreeNode>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessibilityWriteResponse {
    #[serde(rename = "type")]
    msg_type: String,
    pid: u32,
    success: bool,
    message: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OverlayInfoResponse {
    #[serde(rename = "type")]
    msg_type: String,
    overlay_pid: u32,
}

// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    pub connected_windows: Arc<RwLock<HashSet<String>>>, // Set of window IDs with connected clients
    pub current_windows: Arc<RwLock<Vec<WindowInfo>>>,
}

impl WebSocketState {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000);
        Self {
            sender: Arc::new(sender),
            connected_windows: Arc::new(RwLock::new(HashSet::new())),
            current_windows: Arc::new(RwLock::new(Vec::new())),
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

    // Send overlay PID immediately when client connects
    let overlay_info = OverlayInfoResponse {
        msg_type: "overlay_info".to_string(),
        overlay_pid: std::process::id(),
    };

    if let Ok(overlay_json) = serde_json::to_string(&overlay_info) {
        if socket.send(Message::Text(overlay_json)).await.is_ok() {
            println!("📡 Sent overlay PID {} to client", std::process::id());
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
            let (success, message, error) = match write_to_element_by_pid_and_path(
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
    // Try to parse as AccessibilityTreeRequest (less specific)
    else if let Ok(ax_request) = serde_json::from_str::<AccessibilityTreeRequest>(message) {
        println!("🌳 Parsed as AccessibilityTreeRequest: {:?}", ax_request);
        if ax_request.msg_type == "get_accessibility_tree" {
            println!(
                "🌳 Client requesting accessibility tree for PID: {}",
                ax_request.pid
            );

            // Get the accessibility tree
            let (success, tree, error) = match walk_app_tree_by_pid(ax_request.pid) {
                Ok(tree) => (true, Some(tree), None),
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
    // Catch-all for unrecognized messages
    else {
        println!("❓ Unrecognized message format: {}", message);
    }

    Ok(())
}
