//! RPC Dispatch for AXIO
//!
//! Provides a simple JSON-RPC style dispatch function that maps
//! method names to API calls. This enables any transport layer
//! (WebSocket, HTTP, IPC) to call AXIO functions.
//!
//! # Protocol
//!
//! Request: `{ "id": "...", "method": "element_at", "args": { "x": 100, "y": 200 } }`
//! Response: `{ "id": "...", "result": {...} }` or `{ "id": "...", "error": "..." }`

use crate::types::ElementId;
use serde_json::{json, Value};

/// Dispatch an RPC call to the appropriate API function
///
/// # Arguments
/// * `method` - The method name (e.g., "element_at", "write", "watch")
/// * `args` - JSON object with method arguments
///
/// # Returns
/// A JSON value with either `{ "result": ... }` or `{ "error": "..." }`
///
/// # Example
/// ```ignore
/// let response = axio::rpc::dispatch("element_at", &json!({ "x": 100.0, "y": 200.0 }));
/// // Returns: { "result": { "id": "...", "role": "button", ... } }
/// ```
pub fn dispatch(method: &str, args: &Value) -> Value {
    let result = dispatch_inner(method, args);
    match result {
        Ok(v) => json!({ "result": v }),
        Err(e) => json!({ "error": e }),
    }
}

/// Inner dispatch that returns Result for cleaner code
fn dispatch_inner(method: &str, args: &Value) -> Result<Value, String> {
    match method {
        "element_at" => {
            let x = args["x"].as_f64().ok_or("x (f64) required")?;
            let y = args["y"].as_f64().ok_or("y (f64) required")?;
            let node = crate::api::element_at(x, y).map_err(|e| e.to_string())?;
            serde_json::to_value(node).map_err(|e| e.to_string())
        }

        "tree" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            let max_depth = args["max_depth"].as_u64().unwrap_or(50) as usize;
            let max_children = args["max_children_per_level"].as_u64().unwrap_or(2000) as usize;
            let children = crate::api::tree(&ElementId::new(element_id.to_string()), max_depth, max_children)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(children).map_err(|e| e.to_string())
        }

        "write" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            let text = args["text"].as_str().ok_or("text required")?;
            crate::api::write(&ElementId::new(element_id.to_string()), text).map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        "watch" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::watch(&ElementId::new(element_id.to_string())).map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        "unwatch" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::unwatch(&ElementId::new(element_id.to_string()));
            Ok(json!(null))
        }

        "click" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::click(&ElementId::new(element_id.to_string())).map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        _ => Err(format!("unknown method: {}", method)),
    }
}

/// Handle a full JSON-RPC request string
///
/// Parses the request, dispatches, and returns a response string.
/// This is a convenience for WebSocket handlers.
///
/// # Request format
/// ```json
/// { "id": "abc", "method": "element_at", "args": { "x": 100, "y": 200 } }
/// ```
///
/// # Response format
/// ```json
/// { "id": "abc", "result": {...} }
/// // or
/// { "id": "abc", "error": "..." }
/// ```
pub fn handle_request(request: &str) -> String {
    let parsed: Result<Value, _> = serde_json::from_str(request);

    let response = match parsed {
        Ok(req) => {
            let id = req.get("id").cloned().unwrap_or(Value::Null);
            let method = req["method"].as_str().unwrap_or("");
            let args = req.get("args").unwrap_or(&Value::Null);

            let mut response = dispatch(method, args);
            response["id"] = id;
            response
        }
        Err(e) => json!({ "error": format!("Invalid JSON: {}", e) }),
    };

    serde_json::to_string(&response)
        .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_method() {
        let response = dispatch("unknown_method", &json!({}));
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("unknown method"));
    }

    #[test]
    fn test_missing_args() {
        let response = dispatch("element_at", &json!({}));
        assert!(response["error"].as_str().unwrap().contains("required"));
    }
}
