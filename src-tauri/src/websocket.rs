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
use std::{collections::HashSet, sync::Arc};
use tauri::Manager; // For get_webview_window
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::axio::AXNode;
use crate::platform::{get_children_by_element_id, write_to_element_by_id};
use crate::protocol::*;
use crate::windows::{WindowInfo, WindowUpdatePayload};

// ============================================================================
// Message Type Constants
// ============================================================================

pub mod msg_types {
    // Client -> Server request types
    pub const GET_CHILDREN: &str = "get_children";
    pub const WRITE_TO_ELEMENT: &str = "write_to_element";
    pub const CLICK_ELEMENT: &str = "click_element";
    pub const SET_CLICKTHROUGH: &str = "set_clickthrough";
    pub const WATCH_NODE: &str = "watch_node";
    pub const UNWATCH_NODE: &str = "unwatch_node";

    // Server -> Client response types
    pub const GET_CHILDREN_RESPONSE: &str = "get_children_response";
    pub const ACCESSIBILITY_WRITE_RESPONSE: &str = "accessibility_write_response";
    pub const CLICK_ELEMENT_RESPONSE: &str = "click_element_response";
    pub const SET_CLICKTHROUGH_RESPONSE: &str = "set_clickthrough_response";
    pub const WATCH_NODE_RESPONSE: &str = "watch_node_response";
    pub const UNWATCH_NODE_RESPONSE: &str = "unwatch_node_response";

    // Server -> Client push events (not request/response)
    pub const WINDOW_ROOT_UPDATE: &str = "window_root_update";
}

// ============================================================================
// Internal Message Types
// ============================================================================

/// Helper for extracting just the msg_type field for dispatching
#[derive(Debug, Serialize, Deserialize)]
struct MessageType {
    msg_type: Option<String>,
}

// All request/response types are now in protocol.rs

// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    pub current_windows: Arc<RwLock<Vec<WindowInfo>>>,
    pub app_handle: tauri::AppHandle,
}

impl WebSocketState {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        let (sender, _) = broadcast::channel(1000);
        let sender_arc = Arc::new(sender);

        Self {
            sender: sender_arc,
            current_windows: Arc::new(RwLock::new(Vec::new())),
            app_handle,
        }
    }

    pub fn broadcast(&self, data: &WindowUpdatePayload) {
        if let Ok(json) = serde_json::to_string(data) {
            let _ = self.sender.send(json);
        }
    }

    /// Broadcast a window root node to all connected clients
    pub fn broadcast_window_root(&self, window_id: &str, root: AXNode) {
        use crate::protocol::WindowRootUpdate;

        let update = WindowRootUpdate {
            msg_type: msg_types::WINDOW_ROOT_UPDATE.to_string(),
            window_id: window_id.to_string(),
            root,
        };

        if let Ok(json) = serde_json::to_string(&update) {
            let _ = self.sender.send(json);
        }
    }

    /// Get the broadcast sender (for ElementRegistry initialization)
    pub fn sender(&self) -> Arc<broadcast::Sender<String>> {
        self.sender.clone()
    }

    // Store current windows for polling loop
    pub async fn update_windows(&self, windows: &[WindowInfo]) {
        let mut current_windows = self.current_windows.write().await;
        *current_windows = windows.to_vec();
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

    // Note: Element watches are now managed by ElementRegistry per window
    // They will be cleaned up automatically when windows close

    println!("🔌 WebSocket client disconnected");
}

async fn handle_client_message(
    message: &str,
    current_window_id: &mut Option<String>,
    ws_state: &WebSocketState,
    socket: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Extract msg_type for dispatching
    let msg_type_parsed = serde_json::from_str::<MessageType>(message)?;
    let msg_type = msg_type_parsed.msg_type.as_deref().unwrap_or("");

    match msg_type {
        msg_types::WRITE_TO_ELEMENT => {
            // Parse as generic value to extract fields
            let value: serde_json::Value = serde_json::from_str(message)?;
            let element_id = value["element_id"].as_str();
            let text = value["text"].as_str();

            // Attempt to write to the element
            let response = if let (Some(element_id), Some(text)) = (element_id, text) {
                println!("✍️ Writing via element_id: {}", element_id);
                match write_to_element_by_id(element_id, text) {
                    Ok(_) => SetElementValueResponse {
                        msg_type: msg_types::ACCESSIBILITY_WRITE_RESPONSE.to_string(),
                        success: true,
                        error: None,
                    },
                    Err(e) => SetElementValueResponse {
                        msg_type: msg_types::ACCESSIBILITY_WRITE_RESPONSE.to_string(),
                        success: false,
                        error: Some(e),
                    },
                }
            } else {
                SetElementValueResponse {
                    msg_type: msg_types::ACCESSIBILITY_WRITE_RESPONSE.to_string(),
                    success: false,
                    error: Some("element_id or text not provided".to_string()),
                }
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::CLICK_ELEMENT => {
            // Parse request
            let value: serde_json::Value = serde_json::from_str(message)?;
            let req: ClickElementRequest = serde_json::from_value(value)?;

            println!("🖱️ Clicking element_id: {}", req.element_id);

            // Perform click action
            let response = match crate::platform::click_element_by_id(&req.element_id) {
                Ok(_) => ClickElementResponse {
                    msg_type: msg_types::CLICK_ELEMENT_RESPONSE.to_string(),
                    success: true,
                    error: None,
                },
                Err(e) => ClickElementResponse {
                    msg_type: msg_types::CLICK_ELEMENT_RESPONSE.to_string(),
                    success: false,
                    error: Some(e),
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::GET_CHILDREN => {
            // Parse request
            let value: serde_json::Value = serde_json::from_str(message)?;
            let req: GetChildrenRequest = serde_json::from_value(value)?;

            println!(
                "👶 Client requesting children for element_id: {} (max_depth: {}, max_children: {})",
                req.element_id, req.max_depth, req.max_children_per_level
            );

            // Get children
            let response = match get_children_by_element_id(
                &req.element_id,
                req.max_depth,
                req.max_children_per_level,
            ) {
                Ok(children) => GetChildrenResponse {
                    msg_type: msg_types::GET_CHILDREN_RESPONSE.to_string(),
                    success: true,
                    children: Some(children),
                    error: None,
                },
                Err(e) => GetChildrenResponse {
                    msg_type: msg_types::GET_CHILDREN_RESPONSE.to_string(),
                    success: false,
                    children: None,
                    error: Some(e),
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();

            if response.success {
                println!("✅ Sent children");
            } else {
                println!("❌ Failed to get children");
            }
        }

        msg_types::SET_CLICKTHROUGH => {
            // Parse request
            let value: serde_json::Value = serde_json::from_str(message)?;
            let req: SetClickthroughRequest = serde_json::from_value(value)?;

            // Set clickthrough state
            let response = match ws_state.app_handle.get_webview_window("main") {
                Some(window) => match window.set_ignore_cursor_events(req.enabled) {
                    Ok(_) => SetClickthroughResponse {
                        msg_type: msg_types::SET_CLICKTHROUGH_RESPONSE.to_string(),
                        success: true,
                        enabled: req.enabled,
                        error: None,
                    },
                    Err(e) => SetClickthroughResponse {
                        msg_type: msg_types::SET_CLICKTHROUGH_RESPONSE.to_string(),
                        success: false,
                        enabled: false,
                        error: Some(e.to_string()),
                    },
                },
                None => SetClickthroughResponse {
                    msg_type: msg_types::SET_CLICKTHROUGH_RESPONSE.to_string(),
                    success: false,
                    enabled: false,
                    error: Some("Main window not found".to_string()),
                },
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::WATCH_NODE => {
            // Parse request
            let value: serde_json::Value = serde_json::from_str(message)?;
            let req: WatchNodeRequest = serde_json::from_value(value)?;

            // Use ElementRegistry watch API
            use crate::element_registry::ElementRegistry;
            let result = ElementRegistry::watch(&req.element_id);

            let response = match result {
                Ok(_) => WatchNodeResponse {
                    msg_type: msg_types::WATCH_NODE_RESPONSE.to_string(),
                    success: true,
                    node_id: req.node_id,
                    error: None,
                },
                Err(e) => {
                    println!(
                        "{}",
                        format!("ERROR: Watch failed for {}: {}", req.node_id, e).red()
                    );
                    WatchNodeResponse {
                        msg_type: msg_types::WATCH_NODE_RESPONSE.to_string(),
                        success: false,
                        node_id: req.node_id,
                        error: Some(e),
                    }
                }
            };

            let response_json = serde_json::to_string(&response)?;
            socket.send(Message::Text(response_json)).await.ok();
        }

        msg_types::UNWATCH_NODE => {
            // Parse request
            let value: serde_json::Value = serde_json::from_str(message)?;
            let req: UnwatchNodeRequest = serde_json::from_value(value)?;

            println!(
                "🚫 Client requesting to unwatch element_id: {}",
                req.element_id
            );

            // Use ElementRegistry unwatch API
            use crate::element_registry::ElementRegistry;
            ElementRegistry::unwatch(&req.element_id);

            let response = UnwatchNodeResponse {
                msg_type: msg_types::UNWATCH_NODE_RESPONSE.to_string(),
                success: true,
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
