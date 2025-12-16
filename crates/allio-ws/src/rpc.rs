/*!
RPC request/response types and dispatch.
*/

#![allow(missing_docs)]

use allio::a11y::{Action, Value as AXValue};
use allio::{Allio, Element, ElementId, Snapshot, WindowId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use ts_rs::TS;

/// Recency for RPC requests (serializable subset of allio::Recency).
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Recency {
  /// Use cached value, might be stale.
  Any,
  /// Always fetch from OS.
  Current,
  /// Value must be at most this old (milliseconds).
  MaxAgeMs(u32),
}

impl From<Recency> for allio::Recency {
  fn from(r: Recency) -> Self {
    match r {
      Recency::Any => allio::Recency::Any,
      Recency::Current => allio::Recency::Current,
      Recency::MaxAgeMs(ms) => allio::Recency::max_age_ms(ms),
    }
  }
}

/// RPC request.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "args", rename_all = "snake_case")]
#[ts(export)]
pub enum RpcRequest {
  /// Get a snapshot of current state.
  Snapshot,
  /// Get deepest element at screen coordinates.
  ElementAt { x: f64, y: f64 },
  /// Get element by ID with optional recency control.
  Get {
    element_id: ElementId,
    #[serde(default)]
    recency: Option<Recency>,
  },
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
  /// Set value on element.
  Set {
    element_id: ElementId,
    value: AXValue,
  },
  /// Perform an action on element.
  Perform {
    element_id: ElementId,
    action: Action,
  },
  /// Watch element for changes.
  Watch { element_id: ElementId },
  /// Stop watching element.
  Unwatch { element_id: ElementId },
  /// Observe a subtree for changes.
  Observe {
    element_id: ElementId,
    #[serde(default)]
    depth: Option<usize>,
    /// Wait time between sweeps in milliseconds.
    #[serde(default)]
    wait_between_ms: Option<u64>,
  },
  /// Stop observing a subtree.
  Unobserve { element_id: ElementId },
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

pub fn dispatch_json(allio: &Allio, method: &str, args: &JsonValue) -> JsonValue {
  let request_value = json!({ "method": method, "args": args });

  match serde_json::from_value::<RpcRequest>(request_value) {
    Ok(request) => match dispatch(allio, request) {
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

pub fn dispatch(allio: &Allio, request: RpcRequest) -> Result<RpcResponse, String> {
  match request {
    RpcRequest::Snapshot => {
      let snapshot = allio.snapshot();
      Ok(RpcResponse::Snapshot(Box::new(snapshot)))
    }

    RpcRequest::ElementAt { x, y } => {
      let element = allio.element_at(x, y).map_err(|e| e.to_string())?;
      Ok(RpcResponse::OptionalElement(element.map(Box::new)))
    }

    RpcRequest::Get {
      element_id,
      recency,
    } => {
      let recency = recency.map(Into::into).unwrap_or(allio::Recency::Any);
      let element = allio.get(element_id, recency).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::WindowRoot { window_id } => {
      let element = allio
        .window_root(window_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Window not found or has no accessibility: {window_id}"))?;
      Ok(RpcResponse::Element(Box::new(element)))
    }

    RpcRequest::Children {
      element_id,
      max_children: _max_children,
    } => {
      let children = allio
        .children(element_id, allio::Recency::Current)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Elements(children))
    }

    RpcRequest::Parent { element_id } => {
      let parent = allio
        .parent(element_id, allio::Recency::Current)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::OptionalElement(parent.map(Box::new)))
    }

    RpcRequest::Set { element_id, value } => {
      allio
        .set_value(element_id, &value)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Perform { element_id, action } => {
      allio
        .perform_action(element_id, action)
        .map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Watch { element_id } => {
      allio.watch(element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Unwatch { element_id } => {
      allio.unwatch(element_id).map_err(|e| e.to_string())?;
      Ok(RpcResponse::Null)
    }

    RpcRequest::Observe {
      element_id,
      depth,
      wait_between_ms,
    } => {
      let config = allio::ObserveConfig {
        depth,
        wait_between: wait_between_ms.map(std::time::Duration::from_millis),
      };
      // Note: We don't return the handle - the observation stays active until Unobserve is called.
      // This is a simplification for the RPC interface. The handle's Drop won't clean up
      // because we std::mem::forget it.
      let handle = allio
        .observe(element_id, config)
        .map_err(|e| e.to_string())?;
      std::mem::forget(handle);
      Ok(RpcResponse::Null)
    }

    RpcRequest::Unobserve { element_id } => {
      allio.unobserve(element_id);
      Ok(RpcResponse::Null)
    }
  }
}
