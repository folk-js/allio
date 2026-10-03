//! Uniform types and the packed buffer layout they imply.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A uniform's type: a float or a float vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformType {
  /// `f32`
  #[serde(rename = "f32")]
  F32,
  /// `vec2f`
  #[serde(rename = "vec2f")]
  Vec2,
  /// `vec3f`
  #[serde(rename = "vec3f")]
  Vec3,
  /// `vec4f`
  #[serde(rename = "vec4f")]
  Vec4,
}

impl UniformType {
  /// The WGSL type name.
  pub(crate) const fn wgsl(self) -> &'static str {
    match self {
      Self::F32 => "f32",
      Self::Vec2 => "vec2f",
      Self::Vec3 => "vec3f",
      Self::Vec4 => "vec4f",
    }
  }

  /// Number of `f32` values a client supplies.
  pub(crate) const fn floats(self) -> usize {
    match self {
      Self::F32 => 1,
      Self::Vec2 => 2,
      Self::Vec3 => 3,
      Self::Vec4 => 4,
    }
  }

  /// Alignment in bytes under WGSL uniform layout rules (a `vec3f` aligns like a `vec4f`).
  const fn align(self) -> usize {
    match self {
      Self::F32 => 4,
      Self::Vec2 => 8,
      Self::Vec3 | Self::Vec4 => 16,
    }
  }
}

/// Uniforms the renderer fills in itself, in struct order.
pub(crate) const BUILTINS: [(&str, UniformType); 4] = [
  ("resolution", UniformType::Vec2),
  ("time", UniformType::F32),
  ("mouse", UniformType::Vec2),
  ("region", UniformType::Vec4),
];

/// A uniform's place in the packed buffer.
#[derive(Debug, Clone)]
pub(crate) struct Field {
  pub(crate) name: String,
  pub(crate) ty: UniformType,
  /// Byte offset in the uniform buffer.
  pub(crate) offset: usize,
}

/// Packed uniform buffer layout: built-ins first, then the declared uniforms by name.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
  pub(crate) fields: Vec<Field>,
  /// Total byte size, padded to 16.
  pub(crate) size: usize,
}

fn is_identifier(s: &str) -> bool {
  let mut chars = s.chars();
  chars
    .next()
    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Layout {
  pub(crate) fn new(declared: &BTreeMap<String, UniformType>) -> Result<Self, String> {
    let builtins = BUILTINS.iter().map(|&(name, ty)| (name, ty));
    let declared = declared.iter().map(|(name, &ty)| (name.as_str(), ty));
    let mut fields = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cursor = 0;
    for (name, ty) in builtins.chain(declared) {
      if !is_identifier(name) {
        return Err(format!("invalid uniform name '{name}'"));
      }
      if !seen.insert(name) {
        return Err(format!(
          "duplicate uniform '{name}' (built-ins: resolution, time, mouse, region)"
        ));
      }
      cursor = usize::next_multiple_of(cursor, ty.align());
      fields.push(Field {
        name: name.to_string(),
        ty,
        offset: cursor,
      });
      cursor += ty.floats() * 4;
    }
    Ok(Self {
      fields,
      size: cursor.next_multiple_of(16),
    })
  }

  pub(crate) fn field(&self, name: &str) -> Option<&Field> {
    self.fields.iter().find(|f| f.name == name)
  }
}
