//! Server side of `allio.pointer()`.
//!
//! The client pushes its complete set of pointer fields; we merge them into one and keep the
//! event tap matching it. No fields, or fields that change nothing, means no tap at all. A
//! client's fields die with its connection, so a crashed or reloaded page can never leave the
//! cursor reshaped.

use allio_pointer::{Pointer, PointerSpec, PointerState};
use allio_ws::ConnId;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What the reconciler needs from the tap. Exists so the lifecycle can be tested without moving
/// the cursor.
pub trait Live: Sized {
  fn start(spec: PointerSpec) -> Result<Self, String>;
  fn set(&self, spec: PointerSpec);
  fn state(&self) -> PointerState;
}

impl Live for Pointer {
  fn start(spec: PointerSpec) -> Result<Self, String> {
    Pointer::new(spec)
  }
  fn set(&self, spec: PointerSpec) {
    Pointer::set(self, spec);
  }
  fn state(&self) -> PointerState {
    Pointer::state(self)
  }
}

struct State<T> {
  owner: Option<ConnId>,
  live: Option<T>,
}

/// The live pointer field.
pub struct Reconciler<T: Live = Pointer> {
  state: Mutex<State<T>>,
}

pub type Pointers = Reconciler<Pointer>;

impl<T: Live> Reconciler<T> {
  pub const fn new() -> Self {
    Self {
      state: Mutex::new(State {
        owner: None,
        live: None,
      }),
    }
  }

  /// Handles `pointer_set` and `pointer_state`; anything else falls through.
  pub fn handle(&self, conn: ConnId, method: &str, args: &Value) -> Option<Value> {
    match method {
      "pointer_set" => Some(self.set(conn, args)),
      "pointer_state" => Some(self.state()),
      _ => None,
    }
  }

  /// Where the pointer appears and whether the page must draw it; null without a live field.
  fn state(&self) -> Value {
    let state = self.state.lock().unwrap();
    json!({ "result": state.live.as_ref().map(Live::state) })
  }

  /// Makes the live field equal the merge of `args.fields` (id to spec).
  fn set(&self, conn: ConnId, args: &Value) -> Value {
    let fields: BTreeMap<String, PointerSpec> = match serde_json::from_value(args["fields"].clone()) {
      Ok(f) => f,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let spec = PointerSpec::merge(fields.values());
    let mut state = self.state.lock().unwrap();
    state.owner = Some(conn);

    if spec.is_identity() {
      state.live = None;
      return json!({ "result": null });
    }
    match &state.live {
      Some(live) => live.set(spec),
      None => match T::start(spec) {
        Ok(live) => state.live = Some(live),
        Err(e) => return json!({ "error": e }),
      },
    }
    json!({ "result": null })
  }

  /// Lets go of the pointer until the client next changes its fields. An escape hatch.
  pub fn release(&self) {
    self.state.lock().unwrap().live = None;
  }

  /// Drops the field if the closed connection declared it.
  pub fn disconnected(&self, conn: ConnId) {
    let mut state = self.state.lock().unwrap();
    if state.owner == Some(conn) {
      state.owner = None;
      state.live = None;
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use std::cell::RefCell;

  thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
  }

  fn drain() -> Vec<String> {
    LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
  }

  fn record(entry: String) {
    LOG.with(|l| l.borrow_mut().push(entry));
  }

  struct Fake;

  impl Live for Fake {
    fn start(spec: PointerSpec) -> Result<Self, String> {
      record(format!("start {}", spec.gain()));
      Ok(Self)
    }
    fn set(&self, spec: PointerSpec) {
      record(format!("set {}", spec.gain()));
    }
    fn state(&self) -> PointerState {
      PointerState {
        x: 1.0,
        y: 2.0,
        hidden: true,
      }
    }
  }

  impl Drop for Fake {
    fn drop(&mut self) {
      record("stop".into());
    }
  }

  fn set(r: &Reconciler<Fake>, conn: ConnId, fields: Value) {
    r.handle(conn, "pointer_set", &json!({ "fields": fields }));
  }

  #[test]
  fn fields_merge_into_one_tap_that_exists_only_when_needed() {
    drain();
    let r = Reconciler::<Fake>::new();
    set(&r, 1, json!({ "a": {} }));
    assert!(drain().is_empty(), "an identity field installs nothing");

    set(&r, 1, json!({ "a": { "gain": 0.5 }, "b": { "gain": 0.5 } }));
    assert_eq!(drain(), ["start 0.25"]);
    set(&r, 1, json!({ "a": { "gain": 0.5 } }));
    assert_eq!(drain(), ["set 0.5"]);
    set(&r, 1, json!({}));
    assert_eq!(drain(), ["stop"]);
  }

  #[test]
  fn fields_die_with_their_connection() {
    drain();
    let r = Reconciler::<Fake>::new();
    set(&r, 1, json!({ "a": { "gain": 2 } }));
    r.disconnected(2);
    assert_eq!(drain(), ["start 2"]);
    r.disconnected(1);
    assert_eq!(drain(), ["stop"]);
  }

  #[test]
  fn state_reads_the_live_field() {
    let r = Reconciler::<Fake>::new();
    let state = |r: &Reconciler<Fake>| r.handle(1, "pointer_state", &json!({})).unwrap();
    assert_eq!(state(&r)["result"], Value::Null);
    set(&r, 1, json!({ "a": { "gain": 2 } }));
    assert_eq!(state(&r)["result"], json!({ "x": 1.0, "y": 2.0, "hidden": true }));
    drain();
  }

  #[test]
  fn unknown_methods_fall_through() {
    assert!(Reconciler::<Fake>::new()
      .handle(1, "shaders_set", &json!({}))
      .is_none());
  }
}
