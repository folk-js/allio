//! The portable description of a shader: what a client declares.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

use crate::uniforms::UniformType;

/// A rectangle in screen points (top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Region {
  /// Left edge.
  pub x: f64,
  /// Top edge.
  pub y: f64,
  /// Width.
  pub w: f64,
  /// Height.
  pub h: f64,
}

/// Which windows to leave out of the captured `screen` texture, so a shader can see what is
/// behind them. Our own windows are always left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Hide {
  /// Capture everything except our own windows.
  #[default]
  None,
  /// Capture the desktop, the Dock and the menu bar: every application's ordinary windows are
  /// left out.
  All,
  /// Leave out these windows (system window ids, as in allio's `Window.id`).
  Windows(Vec<u32>),
}

/// Another texture a shader reads, by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum Source {
  /// A window's own pixels (without its shadow), even while it is covered or on another Space.
  /// Declare a uniform `NAME_rect: vec4f` to have the host keep it at the window's (x, y, w, h)
  /// on screen, in points.
  Window {
    /// The window (system window id, as in allio's `Window.id`).
    window: u32,
  },
  /// Part of the screen, like `screen`.
  Display {
    /// What to capture, in screen points.
    region: Region,
    /// Windows to leave out.
    #[serde(default)]
    #[ts(optional)]
    hide: Option<Hide>,
  },
}

/// Most named sources a shader can declare.
pub const MAX_SOURCES: usize = 8;

/// Everything that defines a shader. A client sends its complete desired set of these;
/// applying the same spec twice changes nothing.
///
/// The fragment function sees the captured `region` as the texture `screen`; see
/// `docs/SHADERS.md` for the generated prelude and built-in uniforms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShaderSpec {
  /// WGSL source containing `@fragment fn fs(in: VsOut) -> @location(0) vec4f`.
  pub wgsl: String,
  /// Declared uniforms: name to type (`f32`, `vec2f`, `vec3f`, `vec4f` or `vec4f[N]`).
  #[ts(type = "Record<string, \"f32\" | \"vec2f\" | \"vec3f\" | \"vec4f\" | `vec4f[${number}]`>")]
  pub uniforms: BTreeMap<String, UniformType>,
  /// Current uniform values as flat floats. Hot updates use a separate, cheaper message.
  pub values: BTreeMap<String, Vec<f32>>,
  /// Screen region to capture and draw over.
  pub region: Region,
  /// Windows to leave out of `screen`.
  #[serde(default)]
  pub hide: Hide,
  /// Size in screen points of one cell of the `state` texture, for shaders that define `sim`.
  /// Defaults to 4.
  #[serde(default)]
  #[ts(optional)]
  pub cell: Option<f32>,
  /// Simulation steps run per drawn frame (1 to 8). Lets a finer `cell` keep the same speed.
  /// Defaults to 1.
  #[serde(default)]
  #[ts(optional)]
  pub steps: Option<u32>,
  /// A second capture of the screen with its own windows left out, read as the texture `behind`.
  /// With `screen` it lets a shader see a window and what is behind it at once. Default: none.
  #[serde(default)]
  #[ts(optional)]
  pub behind: Option<Hide>,
  /// More textures, by name: each is read in WGSL as a `texture_2d<f32>` of that name. `null`
  /// declares the name with nothing in it yet (it reads as transparent), so a page can keep a
  /// fixed set of names and fill them as it goes. Changing which names exist rebuilds the
  /// pipeline; changing what a name shows doesn't.
  #[serde(default)]
  #[ts(optional, as = "Option<BTreeMap<String, Option<Source>>>")]
  pub sources: BTreeMap<String, Option<Source>>,
  /// Whether to draw every frame. By default a shader draws only when something it reads
  /// changed: a captured frame, a value, the region, or the pointer if it reads `u.mouse`. One
  /// that reads `u.time` or `u.frame`, or defines `sim`, draws every frame unless this is false.
  #[serde(default)]
  #[ts(optional)]
  pub animate: Option<bool>,
}

impl ShaderSpec {
  /// Simulation steps per frame.
  pub fn steps(&self) -> u32 {
    self.steps.unwrap_or(1).clamp(1, 8)
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
  use super::*;

  #[test]
  fn hide_uses_plain_json() {
    assert_eq!(serde_json::to_string(&Hide::None).unwrap(), "\"none\"");
    assert_eq!(serde_json::to_string(&Hide::All).unwrap(), "\"all\"");
    assert_eq!(
      serde_json::to_string(&Hide::Windows(vec![4, 9])).unwrap(),
      "{\"windows\":[4,9]}"
    );
    assert_eq!(
      serde_json::from_str::<Hide>("{\"windows\":[7]}").unwrap(),
      Hide::Windows(vec![7])
    );
  }

  #[test]
  fn hide_and_cell_are_optional_in_a_spec() {
    let json = r#"{"wgsl":"x","uniforms":{},"values":{},"region":{"x":0,"y":0,"w":1,"h":1}}"#;
    let spec: ShaderSpec = serde_json::from_str(json).unwrap();
    assert_eq!(spec.hide, Hide::None);
    assert_eq!(spec.cell, None);
    assert_eq!(spec.steps(), 1);
    assert_eq!(spec.behind, None);
  }

  #[test]
  fn sources_are_windows_displays_or_empty() {
    let json = r#"{"wgsl":"x","uniforms":{},"values":{},"region":{"x":0,"y":0,"w":1,"h":1},
      "sources":{"a":{"window":7},"b":{"region":{"x":1,"y":2,"w":3,"h":4}},"c":null}}"#;
    let spec: ShaderSpec = serde_json::from_str(json).unwrap();
    assert_eq!(spec.sources["a"], Some(Source::Window { window: 7 }));
    assert!(matches!(
      spec.sources["b"],
      Some(Source::Display { hide: None, .. })
    ));
    assert_eq!(spec.sources["c"], None);
  }

  #[test]
  fn steps_are_clamped() {
    let json =
      r#"{"wgsl":"x","uniforms":{},"values":{},"region":{"x":0,"y":0,"w":1,"h":1},"steps":99}"#;
    assert_eq!(serde_json::from_str::<ShaderSpec>(json).unwrap().steps(), 8);
  }
}
