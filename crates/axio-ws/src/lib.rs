//! AXIO WebSocket Server - thin layer over axio with JSON-RPC + events.
//!
//! Request:  `{ "id": "123", "method": "element_at", "args": { "x": 100, "y": 200 } }`
//! Response: `{ "id": "123", "result": {...} }` or `{ "id": "123", "error": "..." }`
//! Event:    `{ "event": "window_update", "data": [...] }`

mod websocket;

pub use axio::{AXNode, AXWindow, ElementUpdate};
pub use websocket::{start_ws_server, CustomRpcHandler, WebSocketState};
