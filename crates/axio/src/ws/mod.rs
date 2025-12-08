/*!
WebSocket server for AXIO.

Provides JSON-RPC over WebSocket for accessibility API access.
Enable with the `ws` feature.

# Example

```ignore
use axio::ws::{WebSocketState, start_server};

// Initialize events first
let _ = axio::init_events();

// Create and start server
let state = WebSocketState::new();
start_server(state).await;
```
*/

mod rpc;
mod server;

pub use rpc::{dispatch, dispatch_json, RpcRequest, RpcResponse};
pub use server::{start_server, CustomRpcHandler, WebSocketState};

