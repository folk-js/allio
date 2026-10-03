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
  /// Declared uniforms: name to type (`f32`, `vec2f`, `vec3f` or `vec4f`).
  #[ts(type = "Record<string, \"f32\" | \"vec2f\" | \"vec3f\" | \"vec4f\">")]
  pub uniforms: BTreeMap<String, UniformType>,
  /// Current uniform values as flat floats. Hot updates use a separate, cheaper message.
  pub values: BTreeMap<String, Vec<f32>>,
  /// Screen region to capture and draw over.
  pub region: Region,
}
