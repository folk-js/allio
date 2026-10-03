//! Stand-in so dependents compile on platforms without an implementation.

use crate::spec::{Region, ShaderSpec};
use std::collections::BTreeMap;

/// Screen shaders are not implemented on this platform, so no value of this type exists.
#[derive(Debug)]
#[allow(missing_copy_implementations)] // there are no values to copy
pub enum Shader {}

impl Shader {
  /// Always fails on this platform.
  pub fn new(_spec: &ShaderSpec) -> Result<Self, String> {
    Err("screen shaders are only implemented on macOS".into())
  }

  /// Unreachable: no instance can exist.
  pub fn update(&self, _spec: &ShaderSpec) -> Result<(), String> {
    match *self {}
  }

  /// Unreachable: no instance can exist.
  pub fn set_values(&self, _values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
    match *self {}
  }

  /// Unreachable: no instance can exist.
  pub fn set_region(&self, _region: Region) {
    match *self {}
  }
}
