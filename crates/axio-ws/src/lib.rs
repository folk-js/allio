//! AXIO WebSocket Server
//!
//! Provides a WebSocket server for the AXIO accessibility system.
//! This crate bridges axio-core with network clients.
//!
//! # Example
//!
//! ```ignore
//! use axio_ws::{WebSocketState, start_ws_server};
//!
//! let (sender, _) = tokio::sync::broadcast::channel(1000);
//! let state = WebSocketState::new(Arc::new(sender));
//! tokio::spawn(start_ws_server(state));
//! ```

pub mod protocol;
pub mod websocket;

// Re-export main types
pub use protocol::{ClientMessage, ServerMessage};
pub use websocket::{start_ws_server, ClickthroughCallback, WebSocketState};

// Re-export ElementUpdate from axio-core for convenience
pub use axio::ElementUpdate;
