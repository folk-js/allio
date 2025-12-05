//! WebSocket server - thin transport layer over axio.

use axio::{AXElement, AXWindow, ElementId, EventSink, ServerEvent, Snapshot};
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

fn send_event(sender: &broadcast::Sender<String>, event: ServerEvent) {
    if let Ok(json) = serde_json::to_string(&event) {
        let _ = sender.send(json);
    }
}

/// Handler for app-specific RPC methods not in axio core.
pub type CustomRpcHandler = Arc<dyn Fn(&str, &Value) -> Option<Value> + Send + Sync>;

/// WebSocket state: broadcasts events to clients, handles RPC requests.
#[derive(Clone)]
pub struct WebSocketState {
    sender: Arc<broadcast::Sender<String>>,
    custom_handler: Option<CustomRpcHandler>,
}

impl WebSocketState {
    pub fn new(sender: Arc<broadcast::Sender<String>>) -> Self {
        Self {
            sender,
            custom_handler: None,
        }
    }

    pub fn with_custom_handler(mut self, handler: CustomRpcHandler) -> Self {
        self.custom_handler = Some(handler);
        self
    }
}

impl EventSink for WebSocketState {
    fn on_window_opened(&self, window: &AXWindow) {
        send_event(&self.sender, ServerEvent::WindowOpened(window.clone()));
    }

    fn on_window_closed(&self, window_id: &str) {
        send_event(
            &self.sender,
            ServerEvent::WindowClosed {
                window_id: window_id.to_string(),
            },
        );
    }

    fn on_window_updated(&self, window: &AXWindow) {
        send_event(&self.sender, ServerEvent::WindowUpdated(window.clone()));
    }

    fn on_window_active(&self, window_id: Option<&str>) {
        send_event(
            &self.sender,
            ServerEvent::WindowActive {
                window_id: window_id.map(|s| s.to_string()),
            },
        );
    }

    fn on_element_discovered(&self, element: &AXElement) {
        send_event(
            &self.sender,
            ServerEvent::ElementDiscovered(element.clone()),
        );
    }

    fn on_element_updated(&self, element: &AXElement, changed: &[String]) {
        send_event(
            &self.sender,
            ServerEvent::ElementUpdated {
                element: element.clone(),
                changed: changed.to_vec(),
            },
        );
    }

    fn on_element_destroyed(&self, element_id: &ElementId) {
        send_event(
            &self.sender,
            ServerEvent::ElementDestroyed {
                element_id: element_id.clone(),
            },
        );
    }

    fn on_mouse_position(&self, x: f64, y: f64) {
        send_event(&self.sender, ServerEvent::MousePosition { x, y });
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

    // Send sync:snapshot with current state
    let snapshot = Snapshot {
        windows: axio::get_current_windows(),
        active_window: axio::get_active_window(),
    };
    if let Ok(msg) = serde_json::to_string(&ServerEvent::Snapshot(snapshot)) {
        let _ = socket.send(Message::Text(msg)).await;
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

    // Try custom handler first
    if let Some(ref handler) = ws_state.custom_handler {
        if let Some(result) = handler(method, args) {
            let mut response = result;
            response["id"] = id;
            return response.to_string();
        }
    }

    // Fall through to axio RPC
    let mut response = crate::rpc::dispatch_json(method, args);
    response["id"] = id;
    response.to_string()
}
