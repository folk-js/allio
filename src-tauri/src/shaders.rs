//! Server side of `allio.shader()`.
//!
//! The client owns shader state and pushes its complete desired set; we make the live set match
//! it. Everything a client declared lives exactly as long as its connection.

use allio_shader::{Region, Shader, ShaderSpec};
use allio_ws::ConnId;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What the reconciler needs from a shader. Exists so the lifecycle rules can be tested without
/// a screen.
pub trait Live: Sized {
  fn start(spec: &ShaderSpec) -> Result<Self, String>;
  fn update(&self, spec: &ShaderSpec) -> Result<(), String>;
  fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String>;
  fn set_region(&self, region: Region);
}

impl Live for Shader {
  fn start(spec: &ShaderSpec) -> Result<Self, String> {
    Shader::new(spec)
  }
  fn update(&self, spec: &ShaderSpec) -> Result<(), String> {
    Shader::update(self, spec)
  }
  fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
    Shader::set_values(self, values)
  }
  fn set_region(&self, region: Region) {
    Shader::set_region(self, region);
  }
}

struct State<T> {
  owner: Option<ConnId>,
  shaders: BTreeMap<String, T>,
}

/// The live shaders.
pub struct Reconciler<T: Live = Shader>(Mutex<State<T>>);

impl<T: Live> Default for Reconciler<T> {
  fn default() -> Self {
    Self(Mutex::new(State {
      owner: None,
      shaders: BTreeMap::new(),
    }))
  }
}

pub type Shaders = Reconciler<Shader>;

/// A partial update from the hot path.
#[derive(Deserialize)]
struct Patch {
  id: String,
  #[serde(default)]
  values: BTreeMap<String, Vec<f32>>,
  region: Option<Region>,
}

impl<T: Live> Reconciler<T> {
  /// Handles `shaders_set` and `shader_patch`; anything else falls through.
  pub fn handle(&self, conn: ConnId, method: &str, args: &Value) -> Option<Value> {
    match method {
      "shaders_set" => Some(self.set(conn, args)),
      "shader_patch" => Some(self.patch(args)),
      _ => None,
    }
  }

  /// Makes the live set equal `args.shaders` (id to spec). Specs that didn't change are no-ops.
  /// Replies with per-shader errors, e.g. WGSL diagnostics.
  fn set(&self, conn: ConnId, args: &Value) -> Value {
    let want: BTreeMap<String, ShaderSpec> = match serde_json::from_value(args["shaders"].clone()) {
      Ok(w) => w,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let mut live = self.0.lock().unwrap();
    live.owner = Some(conn);
    live.shaders.retain(|id, _| want.contains_key(id));

    let mut errors = BTreeMap::new();
    for (id, spec) in &want {
      let result = match live.shaders.get(id) {
        Some(shader) => shader.update(spec),
        None => T::start(spec).map(|shader| {
          live.shaders.insert(id.clone(), shader);
        }),
      };
      if let Err(e) = result {
        errors.insert(id, e);
      }
    }
    json!({ "result": { "errors": errors } })
  }

  /// Hot path: overwrite one shader's uniform values and/or region.
  fn patch(&self, args: &Value) -> Value {
    let patch: Patch = match serde_json::from_value(args.clone()) {
      Ok(p) => p,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let live = self.0.lock().unwrap();
    let Some(shader) = live.shaders.get(&patch.id) else {
      return json!({ "error": format!("no shader '{}'", patch.id) });
    };
    if let Some(region) = patch.region {
      shader.set_region(region);
    }
    match shader.set_values(&patch.values) {
      Ok(()) => json!({ "result": null }),
      Err(e) => json!({ "error": e }),
    }
  }

  /// Drops everything the closed connection declared.
  pub fn disconnected(&self, conn: ConnId) {
    let mut live = self.0.lock().unwrap();
    if live.owner == Some(conn) {
      live.owner = None;
      live.shaders.clear();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::{Arc, Mutex as StdMutex};

  /// Records what the reconciler did to it.
  struct Fake {
    log: Arc<StdMutex<Vec<String>>>,
    id: String,
  }

  thread_local! {
    static LOG: Arc<StdMutex<Vec<String>>> = Arc::default();
  }

  fn log() -> Arc<StdMutex<Vec<String>>> {
    LOG.with(Arc::clone)
  }

  impl Live for Fake {
    fn start(spec: &ShaderSpec) -> Result<Self, String> {
      if spec.wgsl == "bad" {
        return Err("bad shader".into());
      }
      log().lock().unwrap().push(format!("start {}", spec.wgsl));
      Ok(Self {
        log: log(),
        id: spec.wgsl.clone(),
      })
    }
    fn update(&self, spec: &ShaderSpec) -> Result<(), String> {
      self.log.lock().unwrap().push(format!("update {}", self.id));
      if spec.wgsl == "bad" {
        Err("bad edit".into())
      } else {
        Ok(())
      }
    }
    fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
      self.log.lock().unwrap().push(format!(
        "values {} {:?}",
        self.id,
        values.keys().collect::<Vec<_>>()
      ));
      Ok(())
    }
    fn set_region(&self, region: Region) {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("region {} {}", self.id, region.x));
    }
  }

  impl Drop for Fake {
    fn drop(&mut self) {
      self.log.lock().unwrap().push(format!("drop {}", self.id));
    }
  }

  fn spec(wgsl: &str) -> Value {
    json!({ "wgsl": wgsl, "uniforms": {}, "values": {}, "region": { "x": 0, "y": 0, "w": 1, "h": 1 } })
  }

  fn drain() -> Vec<String> {
    std::mem::take(&mut *log().lock().unwrap())
  }

  fn set(r: &Reconciler<Fake>, conn: ConnId, shaders: Value) -> Value {
    r.handle(conn, "shaders_set", &json!({ "shaders": shaders }))
      .unwrap()
  }

  #[test]
  fn set_starts_updates_and_removes() {
    drain();
    let r = Reconciler::<Fake>::default();
    set(&r, 1, json!({ "a": spec("A"), "b": spec("B") }));
    assert_eq!(drain(), ["start A", "start B"]);

    // The same state again only updates; dropping "a" removes it.
    set(&r, 1, json!({ "b": spec("B") }));
    assert_eq!(drain(), ["drop A", "update B"]);
  }

  #[test]
  fn errors_are_reported_per_shader() {
    drain();
    let r = Reconciler::<Fake>::default();
    let reply = set(&r, 1, json!({ "a": spec("A"), "b": spec("bad") }));
    assert_eq!(reply["result"]["errors"], json!({ "b": "bad shader" }));

    // A failed edit leaves the shader in place.
    let reply = set(&r, 1, json!({ "a": spec("bad") }));
    assert_eq!(reply["result"]["errors"], json!({ "a": "bad edit" }));
    assert!(r.0.lock().unwrap().shaders.contains_key("a"));
  }

  #[test]
  fn shaders_die_with_the_connection_that_declared_them() {
    drain();
    let r = Reconciler::<Fake>::default();
    set(&r, 1, json!({ "a": spec("A") }));

    // A different connection closing changes nothing.
    r.disconnected(2);
    assert!(drain().iter().all(|e| !e.starts_with("drop")));

    // A new connection takes over; the old one closing afterwards must not tear it down.
    set(&r, 3, json!({ "a": spec("A") }));
    r.disconnected(1);
    assert!(r.0.lock().unwrap().shaders.contains_key("a"));

    r.disconnected(3);
    assert!(r.0.lock().unwrap().shaders.is_empty());
    assert!(drain().contains(&"drop A".to_string()));
  }

  #[test]
  fn patch_applies_values_and_region() {
    drain();
    let r = Reconciler::<Fake>::default();
    set(&r, 1, json!({ "a": spec("A") }));
    drain();

    r.handle(
      1,
      "shader_patch",
      &json!({ "id": "a", "values": { "x": [1.0] }, "region": { "x": 5, "y": 0, "w": 1, "h": 1 } }),
    );
    assert_eq!(drain(), ["region A 5", "values A [\"x\"]"]);

    let reply = r
      .handle(1, "shader_patch", &json!({ "id": "missing" }))
      .unwrap();
    assert!(reply["error"].as_str().unwrap().contains("no shader"));
  }

  #[test]
  fn unknown_methods_fall_through() {
    let r = Reconciler::<Fake>::default();
    assert!(r.handle(1, "set_passthrough", &json!({})).is_none());
  }
}
