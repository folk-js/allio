/*!
RPC request/response types and dispatch.
*/

#![allow(missing_docs)]

use axio::accessibility::{Action, Value as AXValue};
use axio::{Axio, Element, ElementId, Snapshot, WindowId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use ts_rs::TS;

/// RPC request.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "args", rename_all = "snake_case")]
#[ts(export)]
pub enum RpcRequest {
  /// Get a snapshot of current state.
  Snapshot,
  /// Get deepest element at screen coordinates.
  ElementAt { x: f64, y: f64 },
  /// Get cached element by ID.
  Get { element_id: ElementId },
  /// Get root element for a window.
  WindowRoot { window_id: WindowId },
  /// Discover children of element.
  Children {
    element_id: ElementId,
    #[serde(default = "default_max_children")]
    max_children: usize,
  },
  /// Discover parent of element.
  Parent { element_id: ElementId },
  /// Refresh element from OS.
  Refresh { element_id: ElementId },
  /// Write value to element.
  Write {
    element_id: ElementId,
    value: AXValue,
  },
  /// Perform an action on element.
  Action {
    element_id: ElementId,
    action: Action,
  },
  /// Watch element for changes.
  Watch { element_id: ElementId },
  /// Stop watching element.
  Unwatch { element_id: ElementId },
}

const fn default_max_children() -> usize {
  1000
}

/// RPC response.
#[derive(Debug, Serialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum RpcResponse {
  /// Full state snapshot.
  Snapshot(Box<Snapshot>),
  /// Single element.
  Element(Box<Element>),
  /// Optional element.
  OptionalElement(Option<Box<Element>>),
  /// List of elements.
  Elements(Vec<Element>),
  /// No data.
  Null,
}

pub fn dispatch_json(axio: &Axio, method: &str, args: &JsonValue) -> JsonValue {
  let request_value = json!({ "method": method, "args": args });

  match serde_json::from_value::<RpcRequest>(request_value) {
    Ok(request) => match dispatch(axio, request) {
      Ok(response) => json!({ "result": response }),
      Err(e) => {
        log::warn!("[rpc] {method} failed: {e}");
        json!({ "error": e })
      }
    },
    Err(e) => {
      log::warn!("[rpc] Invalid request for {method}: {e}");
      json!({ "error": format!("Invalid request: {}", e) })
    }
  }
}

pub fn dispatch(axio: &Axio, request: RpcRequest) -> Result<RpcResponse, String> {
  match request {
    RpcRequest::Snapshot => {
      let snapshot = axio.snapshot();
      Ok(RpcResponse::Snapshot(Box::new(snapshot)))
    }

    RpcRequest::ElementAt { x, y } => {
      let element = axio.element_at(x, y).map_err(|e| e.to_string())?;
      Ok(RpcResponse::OptionalElement(element.map(Box::new)))
    }

    RpcRequest::Get { element_id } => {
      let element = axio
        .get(element_id, axio::Recency::Any)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::WindowRoot { window_id } => {
      let element = axio
        .window_root(window_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Window not found or has no accessibility: {window_id}"))?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Children {
      element_id,
      max_children: _max_children,
    } => {
      let children = axio
        .children(element_id, axio::Recency::Current)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Elements(children))
    }

    RpcRequest::Parent { element_id } => {
      let parent = axio
        .parent(element_id, axio::Recency::Current)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::OptionalElement(parent.map(Box::new)))
    }

    RpcRequest::Refresh { element_id } => {
      let element = axio
        .get(element_id, axio::Recency::Current)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Write { element_id, value } => {
      axio
        .set_value(element_id, &value)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Action { element_id, action } => {
      axio
        .perform_action(element_id, action)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Watch { element_id } => {
      axio.watch(element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Unwatch { element_id } => {
      axio.unwatch(element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }
  }
}
