/*!
WebSocket server implementation.
*/

use allio::{Allio, Event};
use axum::{
  extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
  },
  response::Response,
  routing::get,
  Router,
};
use log::error;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

/// Default WebSocket server port.
pub const DEFAULT_WS_PORT: u16 = 3030;
const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

/// Handler for app-specific RPC methods.
pub type CustomRpcHandler = Arc<dyn Fn(&str, &Value) -> Option<Value> + Send + Sync>;

/// WebSocket state.
#[derive(Clone)]
pub struct WebSocketState {
  allio: Allio,
  json_sender: Arc<broadcast::Sender<String>>,
  custom_handler: Option<CustomRpcHandler>,
  port: u16,
}

impl std::fmt::Debug for WebSocketState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("WebSocketState")
      .field("port", &self.port)
      .finish_non_exhaustive()
  }
}

impl WebSocketState {
  /// Create with default port.
  pub fn new(allio: Allio) -> Self {
    Self::with_port(allio, DEFAULT_WS_PORT)
  }

  /// Create with custom port.
  pub fn with_port(allio: Allio, port: u16) -> Self {
    let (json_tx, _) = broadcast::channel::<String>(DEFAULT_CHANNEL_CAPACITY);
    Self {
      allio,
      json_sender: Arc::new(json_tx),
      custom_handler: None,
      port,
    }
  }

  /// Add a custom RPC handler.
  #[must_use]
  pub fn with_custom_handler(mut self, handler: CustomRpcHandler) -> Self {
    self.custom_handler = Some(handler);
    self
  }
}

/// Start the WebSocket server.
pub async fn start_server(ws_state: WebSocketState) {
  let port = ws_state.port;
  let sender = ws_state.json_sender.clone();
  let mut rx = ws_state.allio.subscribe();
  tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
      if let Ok(json) = serde_json::to_string(&event) {
        drop(sender.send(json));
      }
    }
  });

  let cors = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any);

  let app = Router::new()
    .route("/ws", get(websocket_handler))
    .layer(cors)
    .with_state(ws_state);

  let addr = format!("127.0.0.1:{port}");
  let listener = match tokio::net::TcpListener::bind(&addr).await {
    Ok(l) => l,
    Err(e) => {
      error!("Failed to bind WebSocket server to {addr}: {e}");
      std::process::exit(1);
    }
  };

  println!("WebSocket server: ws://{addr}/ws");

  if let Err(e) = axum::serve(listener, app).await {
    error!("WebSocket server failed: {e}");
    std::process::exit(1);
  }
}

async fn websocket_handler(
  ws: WebSocketUpgrade,
  State(ws_state): State<WebSocketState>,
) -> Response {
  ws.on_upgrade(|socket| handle_websocket(socket, ws_state))
}

async fn handle_websocket(mut socket: WebSocket, ws_state: WebSocketState) {
  let mut rx = ws_state.json_sender.subscribe();
  let allio_for_init = ws_state.allio.clone();
  let init_result = tokio::task::spawn_blocking(move || allio_for_init.snapshot()).await;

  let Ok(init) = init_result else {
    return;
  };

  let event = Event::SyncInit(init);
  if let Ok(msg) = serde_json::to_string(&event) {
    if socket.send(Message::Text(msg)).await.is_err() {
      return;
    }
  }

  loop {
    tokio::select! {
        msg = socket.recv() => {
            match msg {
                Some(Ok(Message::Text(text))) => {
                    let response = handle_request_async(&text, &ws_state).await;
                    while let Ok(event_json) = rx.try_recv() {
                        drop(socket.send(Message::Text(event_json)).await);
                    }
                    drop(socket.send(Message::Text(response)).await);
                }
                Some(Ok(Message::Close(_))) => {
                    println!("[client] closed connection");
                    break;
                }
                Some(Err(e)) => {
                    eprintln!("WebSocket error: {e}");
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
                Ok(event_json) => {
                    if socket.send(Message::Text(event_json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[ws] Client lagged, dropped {n} events - consider increasing event_channel_capacity or client needs resync");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
  }
}

async fn handle_request_async(request: &str, ws_state: &WebSocketState) -> String {
  let parsed: Result<Value, _> = serde_json::from_str(request);

  let req = match parsed {
    Ok(v) => v,
    Err(e) => return json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
  };

  let id = req.get("id").cloned().unwrap_or(Value::Null);
  let method = req
    .get("method")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let args = req.get("args").cloned().unwrap_or(Value::Null);

  if let Some(ref handler) = ws_state.custom_handler {
    if let Some(mut response) = handler(&method, &args) {
      if let Some(obj) = response.as_object_mut() {
        obj.insert("id".to_string(), id);
      }
      return response.to_string();
    }
  }

  let allio = ws_state.allio.clone();
  let dispatch_result =
    tokio::task::spawn_blocking(move || crate::rpc::dispatch_json(&allio, &method, &args)).await;

  let mut response = match dispatch_result {
    Ok(r) => r,
    Err(_) => json!({ "error": "RPC task panicked" }),
  };
  if let Some(obj) = response.as_object_mut() {
    obj.insert("id".to_string(), id);
  }
  response.to_string()
}
