//! Runs a lens shader over a screen region, standalone (no Tauri).
//! Usage: native [x y w h] [seconds]   (defaults: 100 100 800 500, 10s)
#![allow(clippy::expect_used)]

use allio_shader::{Region, Shader, ShaderSpec};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use std::collections::BTreeMap;
use std::time::Duration;

const WGSL: &str = r"
@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let d = (u.region.xy + in.uv * u.region.zw) - u.mouse;
  let k = 1.0 - smoothstep(0.0, 160.0, length(d));
  let uv = in.uv - (d / u.region.zw) * 0.3 * k * k;
  return textureSample(screen, samp, uv);
}";

fn main() {
  let a: Vec<f64> = std::env::args()
    .skip(1)
    .filter_map(|s| s.parse().ok())
    .collect();
  let region = Region {
    x: a.first().copied().unwrap_or(100.0),
    y: a.get(1).copied().unwrap_or(100.0),
    w: a.get(2).copied().unwrap_or(800.0),
    h: a.get(3).copied().unwrap_or(500.0),
  };
  let secs = a.get(4).copied().unwrap_or(10.0);

  let mtm = MainThreadMarker::new().expect("main thread");
  let app = NSApplication::sharedApplication(mtm);
  app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

  let shader = Shader::new(&ShaderSpec {
    wgsl: WGSL.into(),
    uniforms: BTreeMap::new(),
    values: BTreeMap::new(),
    region,
  })
  .expect("shader");
  std::thread::spawn(move || {
    std::thread::sleep(Duration::from_secs_f64(secs));
    drop(shader);
    std::thread::sleep(Duration::from_millis(200));
    std::process::exit(0);
  });
  app.run();
}
