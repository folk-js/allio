//! Server side of `allio.shader()`.
//!
//! The client owns shader state and pushes its complete desired set; we make the live set match
//! it. Everything a client declared lives exactly as long as its connection.
//!
//! Some uniforms are bound by the host instead of the client: a shader that declares
//! `windows: vec4f[N]` gets the on-screen windows' `(x, y, w, h)` in screen points, frontmost
//! first, zero-padded; one that declares `focused: f32` gets the index of the focused window in
//! that list, or -1; one with a window source `NAME` that declares `NAME_rect: vec4f` gets that
//! window's rect (zero while it isn't on screen). They are updated here as windows change, with no
//! round trip to the page, and window sources are told when their window is resized.

use allio_shader::{Hide, Region, Shader, ShaderSpec, Source, UniformType};
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
  fn set_hide(&self, hide: &Hide) -> Result<(), String>;
  fn set_behind(&self, hide: &Hide) -> Result<(), String>;
  fn probe(&self, x: f64, y: f64) -> Result<[f32; 4], String>;
  fn window_resized(&self, window: u32, w: f64, h: f64);
  /// How many captures it is running.
  fn captures(&self) -> usize {
    0
  }
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
  fn set_hide(&self, hide: &Hide) -> Result<(), String> {
    Shader::set_hide(self, hide)
  }
  fn set_behind(&self, hide: &Hide) -> Result<(), String> {
    Shader::set_behind(self, hide)
  }
  fn probe(&self, x: f64, y: f64) -> Result<[f32; 4], String> {
    Shader::probe(self, x, y)
  }
  fn window_resized(&self, window: u32, w: f64, h: f64) {
    Shader::window_resized(self, window, w, h);
  }
  fn captures(&self) -> usize {
    Shader::captures(self)
  }
}

/// The on-screen windows, front to back.
#[derive(Default)]
pub struct Windows {
  /// `(x, y, w, h)` in screen points.
  pub rects: Vec<[f32; 4]>,
  /// The windows' ids, in the same order.
  pub ids: Vec<u32>,
  /// Index into `rects` of the focused window.
  pub focused: Option<usize>,
}

/// Reads the current windows.
pub type WindowSource = Box<dyn Fn() -> Windows + Send + Sync>;

/// Which host-bound uniforms a shader declared.
#[derive(Default, PartialEq, Debug)]
struct Binding {
  /// Length of its `windows` array.
  windows: Option<usize>,
  focused: bool,
  /// `NAME_rect` uniforms to keep at a window's rect: (uniform, window).
  rects: Vec<(String, u32)>,
  /// Every window shown by a window source.
  sources: Vec<u32>,
}

impl Binding {
  fn of(spec: &ShaderSpec) -> Self {
    Self {
      windows: match spec.uniforms.get("windows") {
        Some(UniformType::Vec4Array(n)) => Some(*n),
        _ => None,
      },
      focused: spec.uniforms.get("focused") == Some(&UniformType::F32),
      rects: spec
        .sources
        .iter()
        .filter_map(|(name, source)| match source {
          Some(Source::Window { window }) => Some((format!("{name}_rect"), *window)),
          _ => None,
        })
        .filter(|(uniform, _)| spec.uniforms.get(uniform) == Some(&UniformType::Vec4))
        .collect(),
      sources: spec
        .sources
        .values()
        .filter_map(|source| match source {
          Some(Source::Window { window }) => Some(*window),
          _ => None,
        })
        .collect(),
    }
  }

  /// The values to set for the given windows.
  fn values(&self, windows: &Windows) -> BTreeMap<String, Vec<f32>> {
    let mut values = BTreeMap::new();
    if let Some(n) = self.windows {
      let mut flat = vec![0.0; n * 4];
      for (slot, rect) in flat.chunks_mut(4).zip(&windows.rects) {
        slot.copy_from_slice(rect);
      }
      values.insert("windows".to_string(), flat);
    }
    if self.focused {
      // Index -1 when nothing is focused, or the focused window doesn't fit in the array.
      let index = windows
        .focused
        .filter(|&i| self.windows.is_none_or(|n| i < n));
      values.insert(
        "focused".to_string(),
        vec![index.map_or(-1.0, |i| i as f32)],
      );
    }
    for (uniform, window) in &self.rects {
      let rect = windows
        .ids
        .iter()
        .position(|id| id == window)
        .and_then(|i| windows.rects.get(i))
        .copied()
        .unwrap_or_default();
      values.insert(uniform.clone(), rect.to_vec());
    }
    values
  }
}

struct Entry<T> {
  shader: T,
  binding: Binding,
  /// The last size each of its source windows was seen at, to tell the shader when it changes.
  sizes: BTreeMap<u32, [f32; 2]>,
}

struct State<T> {
  owner: Option<ConnId>,
  shaders: BTreeMap<String, Entry<T>>,
}

/// The live shaders.
pub struct Reconciler<T: Live = Shader> {
  state: Mutex<State<T>>,
  windows: WindowSource,
}

pub type Shaders = Reconciler<Shader>;

/// A partial update from the hot path.
#[derive(Deserialize)]
struct Patch {
  id: String,
  #[serde(default)]
  values: BTreeMap<String, Vec<f32>>,
  region: Option<Region>,
  hide: Option<Hide>,
  behind: Option<Hide>,
}

/// Where to read a shader's state.
#[derive(Deserialize)]
struct Probe {
  id: String,
  x: f64,
  y: f64,
}

impl<T: Live> Reconciler<T> {
  pub fn new(windows: WindowSource) -> Self {
    Self {
      state: Mutex::new(State {
        owner: None,
        shaders: BTreeMap::new(),
      }),
      windows,
    }
  }

  /// Handles `shaders_set`, `shader_patch` and `shader_probe`; anything else falls through.
  pub fn handle(&self, conn: ConnId, method: &str, args: &Value) -> Option<Value> {
    match method {
      "shaders_set" => Some(self.set(conn, args)),
      "shader_patch" => Some(self.patch(args)),
      "shader_probe" => Some(self.probe(args)),
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
    let mut live = self.state.lock().unwrap();
    live.owner = Some(conn);
    live.shaders.retain(|id, _| want.contains_key(id));

    let mut errors = BTreeMap::new();
    for (id, spec) in &want {
      let result = match live.shaders.get_mut(id) {
        Some(entry) => {
          entry.binding = Binding::of(spec);
          entry.shader.update(spec)
        }
        None => T::start(spec).map(|shader| {
          let binding = Binding::of(spec);
          live.shaders.insert(
            id.clone(),
            Entry {
              shader,
              binding,
              sizes: BTreeMap::new(),
            },
          );
        }),
      };
      if let Err(e) = result {
        errors.insert(id, e);
      }
    }
    bind(&mut live, &(self.windows)());
    json!({ "result": { "errors": errors } })
  }

  /// Hot path: overwrite one shader's uniform values, region and/or hidden windows.
  fn patch(&self, args: &Value) -> Value {
    let patch: Patch = match serde_json::from_value(args.clone()) {
      Ok(p) => p,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let live = self.state.lock().unwrap();
    let Some(entry) = live.shaders.get(&patch.id) else {
      return json!({ "error": format!("no shader '{}'", patch.id) });
    };
    if let Some(region) = patch.region {
      entry.shader.set_region(region);
    }
    let hidden = patch
      .hide
      .map_or(Ok(()), |hide| entry.shader.set_hide(&hide));
    let behind = patch
      .behind
      .map_or(Ok(()), |hide| entry.shader.set_behind(&hide));
    match entry
      .shader
      .set_values(&patch.values)
      .and(hidden)
      .and(behind)
    {
      Ok(()) => json!({ "result": null }),
      Err(e) => json!({ "error": e }),
    }
  }

  /// Reads one cell of a shader's simulation state at a screen point.
  fn probe(&self, args: &Value) -> Value {
    let probe: Probe = match serde_json::from_value(args.clone()) {
      Ok(p) => p,
      Err(e) => return json!({ "error": e.to_string() }),
    };
    let live = self.state.lock().unwrap();
    match live.shaders.get(&probe.id) {
      None => json!({ "error": format!("no shader '{}'", probe.id) }),
      Some(entry) => match entry.shader.probe(probe.x, probe.y) {
        Ok(cell) => json!({ "result": cell }),
        Err(e) => json!({ "error": e }),
      },
    }
  }

  /// How many shaders are live, and how many captures they run between them.
  pub fn summary(&self) -> (usize, usize) {
    let state = self.state.lock().unwrap();
    let captures = state.shaders.values().map(|e| e.shader.captures()).sum();
    (state.shaders.len(), captures)
  }

  /// Re-reads the windows and pushes them to the shaders that bound them.
  pub fn windows_changed(&self) {
    bind(&mut self.state.lock().unwrap(), &(self.windows)());
  }

  /// Drops everything the closed connection declared.
  pub fn disconnected(&self, conn: ConnId) {
    let mut live = self.state.lock().unwrap();
    if live.owner == Some(conn) {
      live.owner = None;
      live.shaders.clear();
    }
  }
}

/// Sets the host-bound uniforms of every shader that declared them.
/// Sets the host-bound uniforms of every shader that declared them, and tells window sources
/// their window's new size.
fn bind<T: Live>(live: &mut State<T>, windows: &Windows) {
  for entry in live.shaders.values_mut() {
    let values = entry.binding.values(windows);
    if !values.is_empty() {
      // Can only fail if the declaration changed under us; the next `set` fixes that.
      let _ = entry.shader.set_values(&values);
    }
    for window in &entry.binding.sources {
      let Some([.., w, h]) = windows
        .ids
        .iter()
        .position(|id| id == window)
        .and_then(|i| windows.rects.get(i))
        .copied()
      else {
        continue;
      };
      if entry.sizes.insert(*window, [w, h]) != Some([w, h]) {
        entry.shader.window_resized(*window, f64::from(w), f64::from(h));
      }
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use std::sync::{Arc, Mutex as StdMutex};

  type Log = Arc<StdMutex<Vec<String>>>;

  /// Records what the reconciler did to it.
  struct Fake {
    log: Log,
    id: String,
  }

  thread_local! {
    static LOG: Log = Arc::default();
  }

  fn log() -> Log {
    LOG.with(Arc::clone)
  }

  fn drain() -> Vec<String> {
    std::mem::take(&mut *log().lock().unwrap())
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
      self
        .log
        .lock()
        .unwrap()
        .push(format!("values {} {values:?}", self.id));
      Ok(())
    }
    fn set_region(&self, region: Region) {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("region {} {}", self.id, region.x));
    }
    fn set_hide(&self, hide: &Hide) -> Result<(), String> {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("hide {} {hide:?}", self.id));
      Ok(())
    }
    fn set_behind(&self, hide: &Hide) -> Result<(), String> {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("behind {} {hide:?}", self.id));
      Ok(())
    }
    fn probe(&self, x: f64, y: f64) -> Result<[f32; 4], String> {
      Ok([x as f32, y as f32, 0.0, 1.0])
    }
    fn window_resized(&self, window: u32, w: f64, h: f64) {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("resized {} {window} {w}x{h}", self.id));
    }
  }

  impl Drop for Fake {
    fn drop(&mut self) {
      self.log.lock().unwrap().push(format!("drop {}", self.id));
    }
  }

  fn reconciler() -> Reconciler<Fake> {
    Reconciler::new(Box::new(Windows::default))
  }

  fn spec(wgsl: &str) -> Value {
    json!({ "wgsl": wgsl, "uniforms": {}, "values": {}, "region": { "x": 0, "y": 0, "w": 1, "h": 1 } })
  }

  fn set(r: &Reconciler<Fake>, conn: ConnId, shaders: Value) -> Value {
    r.handle(conn, "shaders_set", &json!({ "shaders": shaders }))
      .unwrap()
  }

  fn live(r: &Reconciler<Fake>) -> bool {
    !r.state.lock().unwrap().shaders.is_empty()
  }

  #[test]
  fn set_starts_updates_and_removes() {
    drain();
    let r = reconciler();
    set(&r, 1, json!({ "a": spec("A"), "b": spec("B") }));
    assert_eq!(drain(), ["start A", "start B"]);

    // The same state again only updates; dropping "a" removes it.
    set(&r, 1, json!({ "b": spec("B") }));
    assert_eq!(drain(), ["drop A", "update B"]);
  }

  #[test]
  fn errors_are_reported_per_shader() {
    drain();
    let r = reconciler();
    let reply = set(&r, 1, json!({ "a": spec("A"), "b": spec("bad") }));
    assert_eq!(reply["result"]["errors"], json!({ "b": "bad shader" }));

    // A failed edit leaves the shader in place.
    let reply = set(&r, 1, json!({ "a": spec("bad") }));
    assert_eq!(reply["result"]["errors"], json!({ "a": "bad edit" }));
    assert!(live(&r));
  }

  #[test]
  fn shaders_die_with_the_connection_that_declared_them() {
    drain();
    let r = reconciler();
    set(&r, 1, json!({ "a": spec("A") }));

    // A different connection closing changes nothing.
    r.disconnected(2);
    assert!(drain().iter().all(|e| !e.starts_with("drop")));

    // A new connection takes over; the old one closing afterwards must not tear it down.
    set(&r, 3, json!({ "a": spec("A") }));
    r.disconnected(1);
    assert!(live(&r));

    r.disconnected(3);
    assert!(!live(&r));
    assert!(drain().contains(&"drop A".to_string()));
  }

  #[test]
  fn patch_applies_values_region_and_hide() {
    drain();
    let r = reconciler();
    set(&r, 1, json!({ "a": spec("A") }));
    drain();

    r.handle(
      1,
      "shader_patch",
      &json!({ "id": "a", "values": { "x": [1.0] }, "region": { "x": 5, "y": 0, "w": 1, "h": 1 }, "hide": { "windows": [9] }, "behind": "all" }),
    );
    let log = drain();
    assert_eq!(log[0], "region A 5");
    assert_eq!(log[1], "hide A Windows([9])");
    assert_eq!(log[2], "behind A All");
    assert!(log[3].starts_with("values A"));

    let reply = r
      .handle(1, "shader_patch", &json!({ "id": "missing" }))
      .unwrap();
    assert!(reply["error"].as_str().unwrap().contains("no shader"));
  }

  #[test]
  fn probe_reads_a_shaders_state() {
    drain();
    let r = reconciler();
    set(&r, 1, json!({ "a": spec("A") }));
    let reply = r
      .handle(1, "shader_probe", &json!({ "id": "a", "x": 3.0, "y": 4.0 }))
      .unwrap();
    assert_eq!(reply["result"], json!([3.0, 4.0, 0.0, 1.0]));
    let reply = r
      .handle(1, "shader_probe", &json!({ "id": "nope", "x": 0, "y": 0 }))
      .unwrap();
    assert!(reply["error"].as_str().unwrap().contains("no shader"));
  }

  #[test]
  fn unknown_methods_fall_through() {
    assert!(reconciler()
      .handle(1, "set_passthrough", &json!({}))
      .is_none());
  }

  #[test]
  fn declared_window_uniforms_are_bound_by_the_host() {
    drain();
    let source = Arc::new(StdMutex::new(Windows {
      rects: vec![[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]],
      ids: vec![10, 11],
      focused: Some(1),
    }));
    let shared = source.clone();
    let r = Reconciler::<Fake>::new(Box::new(move || {
      let w = shared.lock().unwrap();
      Windows {
        rects: w.rects.clone(),
        ids: w.ids.clone(),
        focused: w.focused,
      }
    }));

    let mut with = spec("A");
    with["uniforms"] = json!({ "windows": "vec4f[3]", "focused": "f32" });
    set(&r, 1, json!({ "a": with, "b": spec("B") }));

    // Only the shader that declared them gets them: padded to its array length.
    let log = drain();
    let values: Vec<_> = log.iter().filter(|e| e.starts_with("values")).collect();
    assert_eq!(values.len(), 1, "{log:?}");
    assert!(
      values[0].contains("[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.0, 0.0, 0.0, 0.0]"),
      "{}",
      values[0]
    );
    assert!(values[0].contains("\"focused\": [1.0]"), "{}", values[0]);

    // They follow window changes.
    *source.lock().unwrap() = Windows {
      rects: vec![[9.0, 9.0, 9.0, 9.0]],
      ids: vec![12],
      focused: None,
    };
    r.windows_changed();
    let log = drain();
    assert!(
      log[0].contains("[9.0, 9.0, 9.0, 9.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]"),
      "{log:?}"
    );
    assert!(log[0].contains("\"focused\": [-1.0]"), "{log:?}");
  }

  #[test]
  fn more_windows_than_slots_are_cut_off() {
    let windows = Windows {
      rects: vec![[1.0; 4], [2.0; 4], [3.0; 4]],
      ids: vec![1, 2, 3],
      focused: Some(2),
    };
    let binding = Binding {
      windows: Some(2),
      focused: true,
      rects: Vec::new(),
      sources: Vec::new(),
    };
    let values = binding.values(&windows);
    assert_eq!(values["windows"], [1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]);
    assert_eq!(values["focused"], [-1.0], "the focused window doesn't fit");
  }

  #[test]
  fn window_sources_get_their_rect_and_are_told_of_resizes() {
    drain();
    let source = Arc::new(StdMutex::new(Windows {
      rects: vec![[1.0, 2.0, 30.0, 40.0]],
      ids: vec![7],
      focused: None,
    }));
    let shared = source.clone();
    let r = Reconciler::<Fake>::new(Box::new(move || {
      let w = shared.lock().unwrap();
      Windows { rects: w.rects.clone(), ids: w.ids.clone(), focused: None }
    }));
    let mut with = spec("A");
    with["uniforms"] = json!({ "win_rect": "vec4f" });
    with["sources"] = json!({ "win": { "window": 7 }, "gone": { "window": 8 }, "spare": null });
    set(&r, 1, json!({ "a": with }));
    let log = drain();
    assert!(log.iter().any(|e| e.contains("\"win_rect\": [1.0, 2.0, 30.0, 40.0]")), "{log:?}");
    assert!(log.contains(&"resized A 7 30x40".to_string()), "{log:?}");

    // Moving doesn't resize; resizing does.
    source.lock().unwrap().rects = vec![[50.0, 2.0, 30.0, 40.0]];
    r.windows_changed();
    assert!(!drain().iter().any(|e| e.starts_with("resized")));
    source.lock().unwrap().rects = vec![[50.0, 2.0, 60.0, 40.0]];
    r.windows_changed();
    assert!(drain().contains(&"resized A 7 60x40".to_string()));
  }
}
