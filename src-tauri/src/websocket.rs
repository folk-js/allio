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
use tauri::Manager;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::platform::{get_children_by_element_id, write_to_element_by_id};
use crate::protocol::{ClientMessage, ServerMessage};
use crate::windows::WindowInfo;
use std::sync::Arc;

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

    pub fn broadcast(&self, windows: &[WindowInfo]) {
        let msg = ServerMessage::WindowUpdate {
            windows: windows.to_vec(),
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.sender.send(json);
        }
    }

    /// Broadcast a window root node to all connected clients
    pub fn broadcast_window_root(&self, window_id: &str, root: crate::axio::AXNode) {
        let msg = ServerMessage::WindowRootUpdate {
            window_id: window_id.to_string(),
            root,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
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

    println!("{}", "[client] connected".bright_black());

    // Send initial window state immediately
    {
        let current_windows = ws_state.current_windows.read().await;
        let msg = ServerMessage::WindowUpdate {
            windows: current_windows.clone(),
        };
        if let Ok(msg_json) = serde_json::to_string(&msg) {
            let _ = socket.send(Message::Text(msg_json)).await;
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

    println!("{}", "[client] disconnected".bright_black());
}

async fn handle_client_message(
    message: &str,
    _current_window_id: &mut Option<String>,
    ws_state: &WebSocketState,
    socket: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Parse message to strongly-typed ClientMessage enum
    let client_msg: ClientMessage = serde_json::from_str(message)?;

    // Type-safe pattern matching with exhaustive checking
    match client_msg {
        ClientMessage::WriteToElement(req) => {
            let response = match write_to_element_by_id(&req.element_id, &req.text) {
                Ok(_) => crate::protocol::write_to_element::Response {
                    success: true,
                    error: None,
                },
                Err(e) => crate::protocol::write_to_element::Response {
                    success: false,
                    error: Some(e),
                },
            };

            let msg = ServerMessage::WriteToElementResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::ClickElement(req) => {
            let response = match crate::platform::click_element_by_id(&req.element_id) {
                Ok(_) => crate::protocol::click_element::Response {
                    success: true,
                    error: None,
                },
                Err(e) => crate::protocol::click_element::Response {
                    success: false,
                    error: Some(e),
                },
            };

            let msg = ServerMessage::ClickElementResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::GetChildren(req) => {
            let response = match get_children_by_element_id(
                &req.element_id,
                req.max_depth,
                req.max_children_per_level,
            ) {
                Ok(children) => crate::protocol::get_children::Response {
                    success: true,
                    children: Some(children),
                    error: None,
                },
                Err(e) => crate::protocol::get_children::Response {
                    success: false,
                    children: None,
                    error: Some(e),
                },
            };

            let msg = ServerMessage::GetChildrenResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::SetClickthrough(req) => {
            let response = match ws_state.app_handle.get_webview_window("main") {
                Some(window) => match window.set_ignore_cursor_events(req.enabled) {
                    Ok(_) => crate::protocol::set_clickthrough::Response {
                        success: true,
                        enabled: req.enabled,
                        error: None,
                    },
                    Err(e) => crate::protocol::set_clickthrough::Response {
                        success: false,
                        enabled: false,
                        error: Some(e.to_string()),
                    },
                },
                None => crate::protocol::set_clickthrough::Response {
                    success: false,
                    enabled: false,
                    error: Some("Main window not found".to_string()),
                },
            };

            let msg = ServerMessage::SetClickthroughResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::WatchNode(req) => {
            use crate::element_registry::ElementRegistry;
            let result = ElementRegistry::watch(&req.element_id);

            let response = match result {
                Ok(_) => crate::protocol::watch_node::Response {
                    success: true,
                    node_id: req.node_id,
                    error: None,
                },
                Err(e) => crate::protocol::watch_node::Response {
                    success: false,
                    node_id: req.node_id,
                    error: Some(e),
                },
            };

            let msg = ServerMessage::WatchNodeResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::UnwatchNode(req) => {
            use crate::element_registry::ElementRegistry;
            ElementRegistry::unwatch(&req.element_id);

            let response = crate::protocol::unwatch_node::Response { success: true };
            let msg = ServerMessage::UnwatchNodeResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::GetElementAtPosition(req) => {
            let response = match crate::platform::get_element_at_position(req.x, req.y) {
                Ok(element) => crate::protocol::get_element_at_position::Response {
                    success: true,
                    element: Some(element),
                    error: None,
                },
                Err(e) => crate::protocol::get_element_at_position::Response {
                    success: false,
                    element: None,
                    error: Some(e),
                },
            };

            let msg = ServerMessage::GetElementAtPositionResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }
    }

    Ok(())
}
