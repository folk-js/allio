//! AXIO WebSocket Server
//!
//! Provides a thin WebSocket layer over the AXIO accessibility system.
//! Uses JSON-RPC style messages with events for push notifications.
//!
//! # Protocol
//!
//! ## Request (Client → Server)
//! ```json
//! { "id": "123", "method": "element_at", "args": { "x": 100, "y": 200 } }
//! ```
//!
//! ## Response (Server → Client)
//! ```json
//! { "id": "123", "result": { ... } }
//! // or
//! { "id": "123", "error": "..." }
//! ```
//!
//! ## Event (Server → Client, pushed)
//! ```json
//! { "event": "window_update", "data": [...] }
//! { "event": "element_update", "data": { ... } }
//! ```
//!
//! # Example
//!
//! ```ignore
//! use axio_ws::{WebSocketState, WsEventSink, start_ws_server};
//! use std::sync::Arc;
//!
//! // Create broadcast channel
//! let (sender, _) = tokio::sync::broadcast::channel(1000);
//! let sender = Arc::new(sender);
//!
//! // Set up event sink to broadcast to WebSocket clients
//! axio::set_event_sink(WsEventSink::new(sender.clone()));
//!
//! // Initialize axio
//! axio::api::initialize();
//!
//! // Create WebSocket state and start server
//! let state = WebSocketState::new(sender);
//! tokio::spawn(start_ws_server(state));
//! ```

mod websocket;

// Re-export main types
pub use websocket::{start_ws_server, ClickthroughCallback, WebSocketState, WsEventSink};

// Re-export axio types for convenience
pub use axio::{AXNode, ElementUpdate, WindowInfo};
