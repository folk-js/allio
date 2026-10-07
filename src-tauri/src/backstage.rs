//! Server side of `allio.backstage()`: a virtual display nobody looks at, for windows that should
//! keep rendering (and stay uncovered) out of sight. A client's backstage lives as long as its
//! connection; when it goes, macOS moves the windows on it back onto real displays.

use allio_display::Backstage;
use allio_ws::ConnId;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Mutex;

/// Default size, in points (it renders at 2x).
const SIZE: (u32, u32) = (2400, 1600);

#[derive(Deserialize)]
struct Request {
  on: bool,
  w: Option<u32>,
  h: Option<u32>,
}

#[derive(Default)]
pub struct Backstages {
  state: Mutex<Option<(ConnId, Backstage)>>,
}

impl Backstages {
  /// Handles `backstage_set`; anything else falls through. Replies with the display's frame in
  /// global screen points, or null when off.
  pub fn handle(&self, conn: ConnId, method: &str, args: &Value) -> Option<Value> {
    (method == "backstage_set").then(|| self.set(conn, args))
  }

  fn set(&self, conn: ConnId, args: &Value) -> Value {
    let request: Request = match serde_json::from_value(args.clone()) {
      Ok(r) => r,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let mut state = self.state.lock().unwrap();
    if !request.on {
      *state = None;
      return json!({ "result": null });
    }
    if state.is_none() {
      let (w, h) = (request.w.unwrap_or(SIZE.0), request.h.unwrap_or(SIZE.1));
      match Backstage::new(w, h) {
        Ok(backstage) => *state = Some((conn, backstage)),
        Err(e) => return json!({ "error": e }),
      }
    }
    let Some((_, backstage)) = state.as_ref() else {
      return json!({ "result": null });
    };
    let f = backstage.frame();
    json!({ "result": { "x": f.x, "y": f.y, "w": f.w, "h": f.h } })
  }

  /// Removes the display if the closed connection made it.
  pub fn disconnected(&self, conn: ConnId) {
    let mut state = self.state.lock().unwrap();
    if state.as_ref().is_some_and(|(owner, _)| *owner == conn) {
      *state = None;
    }
  }
}
