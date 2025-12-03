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
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

use crate::protocol::{ClientMessage, ServerMessage};
use axio::{ElementId, WindowInfo};

/// Callback type for setting clickthrough on the overlay window
pub type ClickthroughCallback = Arc<dyn Fn(bool) -> Result<(), String> + Send + Sync>;

// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    /// Cached windows for sending to newly connected clients
    pub current_windows: Arc<RwLock<Vec<WindowInfo>>>,
    /// Optional callback for setting clickthrough (provided by app layer)
    clickthrough_callback: Option<ClickthroughCallback>,
}

impl WebSocketState {
    pub fn new(sender: Arc<broadcast::Sender<String>>) -> Self {
        Self {
            sender,
            current_windows: Arc::new(RwLock::new(Vec::new())),
            clickthrough_callback: None,
        }
    }

    /// Set the clickthrough callback (called by app layer)
    pub fn with_clickthrough(mut self, callback: ClickthroughCallback) -> Self {
        self.clickthrough_callback = Some(callback);
        self
    }

    /// Get a clone of the sender for external broadcasting
    pub fn sender(&self) -> Arc<broadcast::Sender<String>> {
        self.sender.clone()
    }

    pub fn broadcast(&self, windows: &[WindowInfo]) {
        let msg = ServerMessage::WindowUpdate {
            windows: windows.to_vec(),
        };
        if let Ok(msg_json) = serde_json::to_string(&msg) {
            let _ = self.sender.send(msg_json);
        }
    }

    /// Broadcast a window root node to all connected clients
    pub fn broadcast_window_root(&self, window_id: &str, root: axio::AXNode) {
        let msg = ServerMessage::WindowRootUpdate {
            window_id: window_id.to_string(),
            root,
        };
        if let Ok(msg_json) = serde_json::to_string(&msg) {
            let _ = self.sender.send(msg_json);
        }
    }

    /// Update cached windows (called from polling callback)
    pub fn update_windows(&self, windows: &[WindowInfo]) {
        if let Ok(mut current) = self.current_windows.write() {
            *current = windows.to_vec();
        }
    }
}

pub async fn start_ws_server(ws_state: WebSocketState) {
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

    // Send cached window state immediately so client doesn't have to wait for next update
    let initial_windows = ws_state
        .current_windows
        .read()
        .ok()
        .map(|w| w.clone())
        .filter(|w| !w.is_empty());

    if let Some(windows) = initial_windows {
        let msg = ServerMessage::WindowUpdate { windows };
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
                    Some(Ok(Message::Close(_))) => {
                        println!("{}", "[client] closed connection".bright_black());
                        break;
                    }
                    Some(Err(e)) => {
                        println!("❌ WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        println!("{}", "[client] disconnected".bright_black());
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }

            // Handle broadcasts from other parts of the system
            broadcast_result = rx.recv() => {
                match broadcast_result {
                    Ok(msg) => {
                        // Send broadcast message to client
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Client is too slow, skip messages
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_client_message(
    text: &str,
    _current_window_id: &mut Option<String>,
    ws_state: &WebSocketState,
    socket: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let message: ClientMessage = serde_json::from_str(text)?;

    match message {
        ClientMessage::WriteToElement(req) => {
            let request_id = req.request_id;
            let element_id = ElementId::new(&req.element_id);
            let (success, error) = match axio::api::write(&element_id, &req.text) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            };
            let response = crate::protocol::write_to_element::Response {
                request_id,
                success,
                error,
            };

            let msg = ServerMessage::WriteToElementResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::ClickElement(req) => {
            let request_id = req.request_id;
            let element_id = ElementId::new(&req.element_id);
            let (success, error) = match axio::api::click(&element_id) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            };
            let response = crate::protocol::click_element::Response {
                request_id,
                success,
                error,
            };

            let msg = ServerMessage::ClickElementResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::GetChildren(req) => {
            let request_id = req.request_id;
            let element_id = ElementId::new(&req.element_id);
            let (success, children, error) =
                match axio::api::tree(&element_id, req.max_depth, req.max_children_per_level) {
                    Ok(children) => (true, Some(children), None),
                    Err(e) => (false, None, Some(e.to_string())),
                };
            let response = crate::protocol::get_children::Response {
                request_id,
                success,
                children,
                error,
            };

            let msg = ServerMessage::GetChildrenResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::SetClickthrough(req) => {
            let request_id = req.request_id;
            let (success, enabled, error) =
                if let Some(ref callback) = ws_state.clickthrough_callback {
                    match callback(req.enabled) {
                        Ok(_) => (true, req.enabled, None),
                        Err(e) => (false, false, Some(e)),
                    }
                } else {
                    (false, false, Some("Clickthrough not supported".to_string()))
                };
            let response = crate::protocol::set_clickthrough::Response {
                request_id,
                success,
                enabled,
                error,
            };

            let msg = ServerMessage::SetClickthroughResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::WatchNode(req) => {
            let request_id = req.request_id;
            let element_id = ElementId::new(&req.element_id);
            let (success, error) = match axio::api::watch(&element_id) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            };
            let response = crate::protocol::watch_node::Response {
                request_id,
                success,
                node_id: req.node_id,
                error,
            };

            let msg = ServerMessage::WatchNodeResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::UnwatchNode(req) => {
            let request_id = req.request_id;
            let element_id = ElementId::new(&req.element_id);
            axio::api::unwatch(&element_id);

            let response = crate::protocol::unwatch_node::Response {
                request_id,
                success: true,
            };

            let msg = ServerMessage::UnwatchNodeResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }

        ClientMessage::GetElementAtPosition(req) => {
            let request_id = req.request_id;
            let (success, element, error) = match axio::api::element_at(req.x, req.y) {
                Ok(node) => (true, Some(node), None),
                Err(e) => (false, None, Some(e.to_string())),
            };
            let response = crate::protocol::get_element_at_position::Response {
                request_id,
                success,
                element,
                error,
            };

            let msg = ServerMessage::GetElementAtPositionResponse(response);
            let json = serde_json::to_string(&msg)?;
            socket.send(Message::Text(json)).await.ok();
        }
    }

    Ok(())
}
