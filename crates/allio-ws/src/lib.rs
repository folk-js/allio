/*! Allio JSON-RPC over WebSocket. */

mod rpc;
mod server;

pub use rpc::{dispatch, dispatch_json, RpcRequest, RpcResponse};
pub use server::{start_server, CustomRpcHandler, WebSocketState, DEFAULT_WS_PORT};

