//! WebSocket server - thin transport layer over axio.

use axio::{EventSink, ServerEvent, SyncInit};
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
  fn emit(&self, event: ServerEvent) {
    if let Ok(json) = serde_json::to_string(&event) {
      let _ = self.sender.send(json);
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

  // Send initial state as sync:init (run on blocking thread pool)
  let init_result = tokio::task::spawn_blocking(|| {
    let active = axio::get_active_window();
    let mut windows = axio::get_current_windows();

    // Compute depth_order (window IDs sorted by z_index, front to back)
    windows.sort_by_key(|w| w.z_index);
    let depth_order: Vec<axio::WindowId> = windows.iter().map(|w| w.id.clone()).collect();

    // Query focused element and selection for the active window's app
    let (focused_element, selection) = if let Some(ref window_id) = active {
      if let Some(window) = windows.iter().find(|w| w.id == *window_id) {
        axio::get_current_focus(window.process_id.as_u32())
      } else {
        (None, None)
      }
    } else {
      (None, None)
    };

    SyncInit {
      windows,
      elements: axio::element_registry::ElementRegistry::get_all(),
      active_window: active.clone(),
      focused_window: active,
      focused_element,
      selection,
      depth_order,
      accessibility_enabled: axio::platform::check_accessibility_permissions(),
    }
  })
  .await;

  let init = match init_result {
    Ok(init) => init,
    Err(_) => return,
  };

  let event = ServerEvent::SyncInit(init);
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
                    // Drain any pending events before sending RPC response
                    // This ensures events triggered by RPC are sent first
                    while let Ok(event_json) = rx.try_recv() {
                        let _ = socket.send(Message::Text(event_json)).await;
                    }
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
                Ok(event_json) => {
                    if socket.send(Message::Text(event_json)).await.is_err() {
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

async fn handle_request_async(request: &str, ws_state: &WebSocketState) -> String {
  let parsed: Result<Value, _> = serde_json::from_str(request);

  let req = match parsed {
    Ok(v) => v,
    Err(e) => return json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
  };

  let id = req.get("id").cloned().unwrap_or(Value::Null);
  let method = req["method"].as_str().unwrap_or("").to_string();
  let args = req.get("args").cloned().unwrap_or(Value::Null);

  // Try custom handler first (runs on current thread - fast, may need main thread access)
  if let Some(ref handler) = ws_state.custom_handler {
    if let Some(result) = handler(&method, &args) {
      let mut response = result;
      response["id"] = id;
      return response.to_string();
    }
  }

  // Axio RPC - run on blocking thread pool (AX API calls are slow IPC)
  let dispatch_result =
    tokio::task::spawn_blocking(move || crate::rpc::dispatch_json(&method, &args)).await;

  let mut response = match dispatch_result {
    Ok(r) => r,
    Err(_) => json!({ "error": "RPC task panicked" }),
  };
  response["id"] = id;
  response.to_string()
}
