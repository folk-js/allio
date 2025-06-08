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
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};
use uuid;

use crate::{WindowInfo, WindowUpdatePayload};

// Helper function to safely get short ID
fn get_short_id(id: &str) -> String {
    if id.len() >= 8 {
        id[..8].to_string()
    } else {
        id.to_string()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientIdentification {
    bottom_left_x: i32,
    bottom_left_y: i32,
    window_width: i32,
    debug_info: Option<BrowserDebugInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BrowserDebugInfo {
    #[serde(rename = "screenX")]
    screen_x: i32,
    #[serde(rename = "screenY")]
    screen_y: i32,
    #[serde(rename = "outerWidth")]
    outer_width: i32,
    #[serde(rename = "outerHeight")]
    outer_height: i32,
    #[serde(rename = "innerWidth")]
    inner_width: i32,
    #[serde(rename = "innerHeight")]
    inner_height: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerResponse {
    #[serde(rename = "type")]
    msg_type: String,
    window_id: Option<String>,
    success: bool,
    message: String,
}

#[derive(Debug, Clone)]
pub struct ConnectedClient {
    pub last_coordinates: (i32, i32, i32), // bottom_left_x, bottom_left_y, width
}

// WebSocket state for broadcasting to clients
#[derive(Clone)]
pub struct WebSocketState {
    pub sender: Arc<broadcast::Sender<String>>,
    pub clients: Arc<RwLock<HashMap<String, ConnectedClient>>>, // window_id -> client info
    pub current_windows: Arc<RwLock<Vec<WindowInfo>>>,
}

impl WebSocketState {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000);
        Self {
            sender: Arc::new(sender),
            clients: Arc::new(RwLock::new(HashMap::new())),
            current_windows: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn broadcast(&self, data: &WindowUpdatePayload) {
        if let Ok(json) = serde_json::to_string(data) {
            let _ = self.sender.send(json);
        }
    }

    // Store current windows and match unidentified clients
    pub async fn update_windows(&self, windows: &[WindowInfo]) {
        // Store current windows for immediate matching
        {
            let mut current_windows = self.current_windows.write().await;
            *current_windows = windows.to_vec();
        }

        // No background matching needed - clients are identified immediately when they connect
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

            println!(
                "    Distance to '{}': {:.1}px (pos: {:.1}, width: {:.1})",
                window.name, total_distance, position_distance, width_distance
            );

            // Update best match if this is better
            match best_match {
                None if total_distance <= max_distance => {
                    best_match = Some((window, total_distance));
                    println!("      -> New best match (first within threshold)");
                }
                Some((_, current_best))
                    if total_distance < current_best && total_distance <= max_distance =>
                {
                    best_match = Some((window, total_distance));
                    println!("      -> New best match (better than {:.1})", current_best);
                }
                _ => {
                    if total_distance > max_distance {
                        println!(
                            "      -> Rejected (distance {:.1} > threshold {:.1})",
                            total_distance, max_distance
                        );
                    }
                }
            }
        }

        best_match.map(|(window, _)| window.id.clone())
    }

    // Try to immediately match a client identification request
    pub async fn try_immediate_match(
        &self,
        identification: &ClientIdentification,
    ) -> Option<String> {
        let current_windows = self.current_windows.read().await;

        println!(
            "🔍 Matching client at ({}, {}) width: {} against {} windows:",
            identification.bottom_left_x,
            identification.bottom_left_y,
            identification.window_width,
            current_windows.len()
        );

        for (i, window) in current_windows.iter().enumerate() {
            let window_bottom_x = window.x;
            let window_bottom_y = window.y + window.h;
            println!(
                "  Window {}: '{}' at ({}, {}) size: {}x{} -> bottom-left: ({}, {})",
                i + 1,
                window.name,
                window.x,
                window.y,
                window.w,
                window.h,
                window_bottom_x,
                window_bottom_y
            );
        }

        let result = self.find_best_match(identification, &current_windows);
        if result.is_some() {
            println!("✅ Match found!");
        } else {
            println!("❌ No match within threshold");
        }
        result
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
    let session_id = uuid::Uuid::new_v4().to_string(); // Temporary session ID until window is identified

    println!("🔗 Client session started: {}", get_short_id(&session_id));

    let mut current_window_id: Option<String> = None;

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(e) = handle_client_message(&text, &session_id, &mut current_window_id, &ws_state, &mut socket).await {
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
        let mut clients = ws_state.clients.write().await;
        clients.remove(&window_id);
        println!("🔌 Client disconnected: window {}", window_id);
    } else {
        println!(
            "🔌 Unidentified client session ended: {}",
            get_short_id(&session_id)
        );
    }
}

async fn handle_client_message(
    message: &str,
    session_id: &str,
    current_window_id: &mut Option<String>,
    ws_state: &WebSocketState,
    socket: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(identification) = serde_json::from_str::<ClientIdentification>(message) {
        println!(
            "🎯 Session {} requesting identification at ({}, {}) width: {}px",
            get_short_id(session_id),
            identification.bottom_left_x,
            identification.bottom_left_y,
            identification.window_width
        );

        // Try to match immediately with current windows
        let window_id = ws_state.try_immediate_match(&identification).await;

        if let Some(ref wid) = window_id {
            // Store the window ID for this session
            *current_window_id = Some(wid.clone());

            // Track this client by window ID
            let mut clients = ws_state.clients.write().await;
            clients.insert(
                wid.clone(),
                ConnectedClient {
                    last_coordinates: (
                        identification.bottom_left_x,
                        identification.bottom_left_y,
                        identification.window_width,
                    ),
                },
            );

            println!(
                "✅ Session {} matched to window {}",
                get_short_id(session_id),
                wid
            );
        } else {
            println!("❌ No match found for session {}", get_short_id(session_id));
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

    Ok(())
}
