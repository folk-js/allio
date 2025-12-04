//! WebSocket server - thin transport layer over axio.

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
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

/// Handler for app-specific RPC methods not in axio core.
/// Return Some(response_json) to handle, None to fall through to axio::rpc.
pub type CustomRpcHandler = Arc<dyn Fn(&str, &Value) -> Option<Value> + Send + Sync>;

#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    custom_handler: Option<CustomRpcHandler>,
}

impl WebSocketState {
    pub fn new(sender: Arc<broadcast::Sender<String>>) -> Self {
        Self {
            sender,
            custom_handler: None,
        }
    }

    /// Add a custom RPC handler for app-specific methods.
    pub fn with_custom_handler(mut self, handler: CustomRpcHandler) -> Self {
        self.custom_handler = Some(handler);
        self
    }

    pub fn sender(&self) -> Arc<broadcast::Sender<String>> {
        self.sender.clone()
    }
}

/// EventSink that broadcasts to WebSocket clients.
pub struct WsEventSink {
    sender: Arc<broadcast::Sender<String>>,
}

impl WsEventSink {
    pub fn new(sender: Arc<broadcast::Sender<String>>) -> Self {
        Self { sender }
    }
}

impl EventSink for WsEventSink {
    fn on_element_update(&self, update: ElementUpdate) {
        let msg = json!({ "event": "element_update", "data": update });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_window_update(&self, windows: &[AXWindow]) {
        let msg = json!({ "event": "window_update", "data": windows });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_window_root(&self, window_id: &str, root: &axio::AXNode) {
        let msg = json!({
            "event": "window_root",
            "data": { "window_id": window_id, "root": root }
        });
        let _ = self.sender.send(msg.to_string());
    }

    fn on_mouse_position(&self, x: f64, y: f64) {
        let msg = json!({ "event": "mouse_position", "data": { "x": x, "y": y } });
        let _ = self.sender.send(msg.to_string());
    }
}

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

    // Send current window state from axio's cache
    let windows = axio::get_current_windows();
    if !windows.is_empty() {
        let msg = json!({ "event": "window_update", "data": windows });
        let _ = socket.send(Message::Text(msg.to_string())).await;
    }

    loop {
        tokio::select! {
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
                    _ => {}
                }
            }

            broadcast = rx.recv() => {
                match broadcast {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn handle_request(request: &str, ws_state: &WebSocketState) -> String {
    let parsed: Result<Value, _> = serde_json::from_str(request);

    let req = match parsed {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req["method"].as_str().unwrap_or("");
    let args = req.get("args").unwrap_or(&Value::Null);

    // Try custom handler first (for app-specific methods like set_clickthrough)
    if let Some(ref handler) = ws_state.custom_handler {
        if let Some(result) = handler(method, args) {
            let mut response = result;
            response["id"] = id;
            return response.to_string();
        }
    }

    // Fall through to axio core RPC
    let mut response = axio::rpc::dispatch(method, args);
    response["id"] = id;
    response.to_string()
}
