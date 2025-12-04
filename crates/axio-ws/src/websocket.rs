//! WebSocket Server for AXIO
//!
//! Provides a thin WebSocket layer over AXIO's RPC dispatch.
//! Events are broadcast to all connected clients via the EventSink trait.

use axio::{AXWindow, ElementUpdate, EventSink};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use serde_json::json;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

/// Callback type for setting clickthrough on the overlay window
/// This is the only non-axio operation (it's Tauri/window-specific)
pub type ClickthroughCallback = Arc<dyn Fn(bool) -> Result<(), String> + Send + Sync>;

/// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    /// Broadcast sender for outgoing messages
    pub sender: Arc<broadcast::Sender<String>>,
    /// Cached windows for initial client connections
    pub current_windows: Arc<RwLock<Vec<AXWindow>>>,
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

    /// Get a clone of the current_windows cache Arc
    ///
    /// Use this when creating WsEventSink to share the cache:
    /// ```ignore
    /// let ws_state = WebSocketState::new(sender.clone());
    /// let event_sink = WsEventSink::new(sender, ws_state.current_windows());
    /// ```
    pub fn current_windows(&self) -> Arc<RwLock<Vec<AXWindow>>> {
        self.current_windows.clone()
    }
}

/// EventSink implementation that broadcasts to WebSocket clients
///
/// This bridges the axio event system to WebSocket clients.
/// Also maintains a cache of current windows for new client connections.
pub struct WsEventSink {
    sender: Arc<broadcast::Sender<String>>,
    /// Shared cache for new client connections (same instance as WebSocketState)
    current_windows: Arc<RwLock<Vec<AXWindow>>>,
}

impl WsEventSink {
    pub fn new(
        sender: Arc<broadcast::Sender<String>>,
        current_windows: Arc<RwLock<Vec<AXWindow>>>,
    ) -> Self {
        Self {
            sender,
            current_windows,
        }
    }
}

impl EventSink for WsEventSink {
    fn on_element_update(&self, update: ElementUpdate) {
        let msg = json!({
            "event": "element_update",
            "data": update
        });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_window_update(&self, windows: &[AXWindow]) {
        // Update cache for new client connections
        if let Ok(mut cached) = self.current_windows.write() {
            *cached = windows.to_vec();
        }

        // Broadcast to connected clients
        let msg = json!({
            "event": "window_update",
            "data": windows
        });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_window_root(&self, window_id: &str, root: &axio::AXNode) {
        let msg = json!({
            "event": "window_root",
            "data": {
                "window_id": window_id,
                "root": root
            }
        });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_mouse_position(&self, x: f64, y: f64) {
        let msg = json!({
            "event": "mouse_position",
            "data": { "x": x, "y": y }
        });
        let _ = self.sender.send(msg.to_string());
    }
}

/// Start the WebSocket server
pub async fn start_ws_server(ws_state: WebSocketState) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .layer(cors)
        .with_state(ws_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030")
        .await
        .expect("Failed to bind WebSocket server");

    println!("WebSocket server: ws://127.0.0.1:3030/ws");

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

    println!("[client] connected");

    // Send cached window state immediately
    let initial_windows = ws_state
        .current_windows
        .read()
        .ok()
        .map(|w| w.clone())
        .filter(|w| !w.is_empty());

    if let Some(windows) = initial_windows {
        let msg = json!({
            "event": "window_update",
            "data": windows
        });
        let _ = socket.send(Message::Text(msg.to_string())).await;
    }

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let response = handle_request(&text, &ws_state);
                        let _ = socket.send(Message::Text(response)).await;
                    }
                    Some(Ok(Message::Close(_))) => {
                        println!("[client] closed connection");
                        break;
                    }
                    Some(Err(e)) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        println!("[client] disconnected");
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }

            // Handle outgoing broadcasts
            broadcast = rx.recv() => {
                match broadcast {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Client is too slow, continue
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}

/// Handle an RPC request
///
/// Uses axio::rpc::dispatch for most operations.
/// Handles clickthrough specially since it's window-specific.
fn handle_request(request: &str, ws_state: &WebSocketState) -> String {
    // Parse the request
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(request);

    let req = match parsed {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
    };

    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req["method"].as_str().unwrap_or("");
    let args = req.get("args").unwrap_or(&serde_json::Value::Null);

    // Handle clickthrough specially (not part of axio core)
    if method == "set_clickthrough" {
        let enabled = args["enabled"].as_bool().unwrap_or(false);
        let (success, error) = if let Some(ref callback) = ws_state.clickthrough_callback {
            match callback(enabled) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e)),
            }
        } else {
            (false, Some("Clickthrough not supported".to_string()))
        };

        return json!({
            "id": id,
            "result": if success { json!({ "enabled": enabled }) } else { serde_json::Value::Null },
            "error": error
        })
        .to_string();
    }

    // Use axio's RPC dispatch for everything else
    let mut response = axio::rpc::dispatch(method, args);
    response["id"] = id;
    response.to_string()
}
