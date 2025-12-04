//! JSON-RPC dispatch for AXIO.
//!
//! Request: `{ "id": "...", "method": "element_at", "args": { "x": 100, "y": 200 } }`
//! Response: `{ "id": "...", "result": {...} }` or `{ "id": "...", "error": "..." }`

use crate::types::ElementId;
use serde_json::{json, Value};

pub fn dispatch(method: &str, args: &Value) -> Value {
    match dispatch_inner(method, args) {
        Ok(v) => json!({ "result": v }),
        Err(e) => json!({ "error": e }),
    }
}

fn dispatch_inner(method: &str, args: &Value) -> Result<Value, String> {
    match method {
        "element_at" => {
            let x = args["x"].as_f64().ok_or("x (f64) required")?;
            let y = args["y"].as_f64().ok_or("y (f64) required")?;
            let element = crate::api::element_at(x, y).map_err(|e| e.to_string())?;
            serde_json::to_value(element).map_err(|e| e.to_string())
        }

        "get" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            let element = crate::api::get(&ElementId::new(element_id.to_string()))
                .map_err(|e| e.to_string())?;
            serde_json::to_value(element).map_err(|e| e.to_string())
        }

        "children" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            let max_children = args["max_children"].as_u64().unwrap_or(1000) as usize;
            let children = crate::api::children(
                &ElementId::new(element_id.to_string()),
                max_children,
            ).map_err(|e| e.to_string())?;
            serde_json::to_value(children).map_err(|e| e.to_string())
        }

        "refresh" => {
            let element_ids: Vec<ElementId> = args["element_ids"]
                .as_array()
                .ok_or("element_ids (array) required")?
                .iter()
                .filter_map(|v| v.as_str().map(|s| ElementId::new(s.to_string())))
                .collect();
            let elements = crate::api::refresh(&element_ids).map_err(|e| e.to_string())?;
            serde_json::to_value(elements).map_err(|e| e.to_string())
        }

        "write" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            let text = args["text"].as_str().ok_or("text required")?;
            crate::api::write(&ElementId::new(element_id.to_string()), text)
                .map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        "click" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::click(&ElementId::new(element_id.to_string()))
                .map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        "watch" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::watch(&ElementId::new(element_id.to_string()))
                .map_err(|e| e.to_string())?;
            Ok(json!(null))
        }

        "unwatch" => {
            let element_id = args["element_id"].as_str().ok_or("element_id required")?;
            crate::api::unwatch(&ElementId::new(element_id.to_string()));
            Ok(json!(null))
        }

        _ => Err(format!("unknown method: {}", method)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_method() {
        let response = dispatch("unknown_method", &json!({}));
        assert!(response["error"].as_str().unwrap().contains("unknown method"));
    }

    #[test]
    fn test_missing_args() {
        let response = dispatch("element_at", &json!({}));
        assert!(response["error"].as_str().unwrap().contains("required"));
    }
}
