use axio::{elements, windows, AXElement, ElementId, WindowId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/// RPC request - deserialize from `{ method, args }` format
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "args", rename_all = "snake_case")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum RpcRequest {
  /// Get deepest element at screen coordinates
  ElementAt { x: f64, y: f64 },
  /// Get cached element by ID
  Get { element_id: ElementId },
  /// Get root element for a window
  WindowRoot { window_id: WindowId },
  /// Discover children of element
  Children {
    element_id: ElementId,
    #[serde(default = "default_max_children")]
    max_children: usize,
  },
  /// Refresh element attributes from macOS
  Refresh { element_id: ElementId },
  /// Write text to element
  Write { element_id: ElementId, text: String },
  /// Click element
  Click { element_id: ElementId },
  /// Watch element for changes
  Watch { element_id: ElementId },
  /// Stop watching element
  Unwatch { element_id: ElementId },
}

fn default_max_children() -> usize {
  1000
}

/// RPC response variants
#[derive(Debug, Serialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum RpcResponse {
  /// Single element (boxed to reduce enum size - AXElement is 288 bytes)
  Element(Box<AXElement>),
  Elements(Vec<AXElement>),
  Null,
}

/// Dispatch a raw JSON request
pub fn dispatch_json(method: &str, args: &Value) -> Value {
  // Reconstruct tagged format for serde
  let request_value = json!({ "method": method, "args": args });

  match serde_json::from_value::<RpcRequest>(request_value) {
    Ok(request) => match dispatch(request) {
      Ok(response) => json!({ "result": response }),
      Err(e) => json!({ "error": e }),
    },
    Err(e) => json!({ "error": format!("Invalid request: {}", e) }),
  }
}

/// Typed dispatch - compiler ensures all cases handled correctly
pub fn dispatch(request: RpcRequest) -> Result<RpcResponse, String> {
  match request {
    RpcRequest::ElementAt { x, y } => {
      let element = elements::at(x, y).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Get { element_id } => {
      let element = elements::get(&element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::WindowRoot { window_id } => {
      let element = windows::root(&window_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Children {
      element_id,
      max_children,
    } => {
      let children = elements::children(&element_id, max_children).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Elements(children))
    }

    RpcRequest::Refresh { element_id } => {
      let element = elements::refresh(&element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Write { element_id, text } => {
      elements::write(&element_id, &text).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Click { element_id } => {
      elements::click(&element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Watch { element_id } => {
      elements::watch(&element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Unwatch { element_id } => {
      elements::unwatch(&element_id);
      Ok(RpcResponse::Null)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn export_bindings_rpcrequest() {
    RpcRequest::export().expect("export RpcRequest");
  }

  #[test]
  fn export_bindings_rpcresponse() {
    RpcResponse::export().expect("export RpcResponse");
  }
}
