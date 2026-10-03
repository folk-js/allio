//! Uniform types and the packed buffer layout they imply.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The most uniform data a shader can have: Metal passes it inline, which is limited to 4 KB.
const MAX_UNIFORM_BYTES: usize = 4096;

/// A uniform's type: a float, a float vector, or an array of `vec4f` (e.g. a list of rectangles,
/// given as 4N flat numbers). Serialized as `f32`, `vec2f`, `vec3f`, `vec4f` or `vec4f[N]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniformType {
  /// `f32`
  F32,
  /// `vec2f`
  Vec2,
  /// `vec3f`
  Vec3,
  /// `vec4f`
  Vec4,
  /// `array<vec4f, N>`
  Vec4Array(usize),
}

impl fmt::Display for UniformType {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::F32 => f.write_str("f32"),
      Self::Vec2 => f.write_str("vec2f"),
      Self::Vec3 => f.write_str("vec3f"),
      Self::Vec4 => f.write_str("vec4f"),
      Self::Vec4Array(n) => write!(f, "vec4f[{n}]"),
    }
  }
}

impl Serialize for UniformType {
  fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for UniformType {
  fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
    let s = String::deserialize(d)?;
    Ok(match s.as_str() {
      "f32" => Self::F32,
      "vec2f" => Self::Vec2,
      "vec3f" => Self::Vec3,
      "vec4f" => Self::Vec4,
      other => other
        .strip_prefix("vec4f[")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|n| n.parse().ok())
        .filter(|&n| n > 0)
        .map(Self::Vec4Array)
        .ok_or_else(|| {
          serde::de::Error::custom(format!(
            "unknown uniform type '{other}' (f32, vec2f, vec3f, vec4f or vec4f[N])"
          ))
        })?,
    })
  }
}

impl UniformType {
  /// The WGSL type.
  pub(crate) fn wgsl(self) -> String {
    match self {
      Self::F32 | Self::Vec2 | Self::Vec3 | Self::Vec4 => self.to_string(),
      Self::Vec4Array(n) => format!("array<vec4f, {n}>"),
    }
  }

  /// Number of `f32` values a client supplies.
  pub(crate) const fn floats(self) -> usize {
    match self {
      Self::F32 => 1,
      Self::Vec2 => 2,
      Self::Vec3 => 3,
      Self::Vec4 => 4,
      Self::Vec4Array(n) => 4 * n,
    }
  }

  /// Alignment in bytes under WGSL uniform layout rules (a `vec3f` aligns like a `vec4f`).
  const fn align(self) -> usize {
    match self {
      Self::F32 => 4,
      Self::Vec2 => 8,
      Self::Vec3 | Self::Vec4 | Self::Vec4Array(_) => 16,
    }
  }

  /// Size in bytes. A `vec3f` takes 12 and an array of `vec4f` has no padding between elements.
  const fn size(self) -> usize {
    self.floats() * 4
  }
}

/// Uniforms the renderer fills in itself, in struct order.
pub(crate) const BUILTINS: [(&str, UniformType); 6] = [
  ("resolution", UniformType::Vec2),
  ("time", UniformType::F32),
  ("mouse", UniformType::Vec2),
  ("region", UniformType::Vec4),
  ("frame", UniformType::F32),
  ("state_size", UniformType::Vec2),
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
          "duplicate uniform '{name}' (built-ins: resolution, time, mouse, region, frame, state_size)"
        ));
      }
      cursor = usize::next_multiple_of(cursor, ty.align());
      fields.push(Field {
        name: name.to_string(),
        ty,
        offset: cursor,
      });
      cursor += ty.size();
    }
    let size = cursor.next_multiple_of(16);
    if size > MAX_UNIFORM_BYTES {
      return Err(format!(
        "uniforms take {size} bytes; the limit is {MAX_UNIFORM_BYTES}"
      ));
    }
    Ok(Self { fields, size })
  }

  pub(crate) fn field(&self, name: &str) -> Option<&Field> {
    self.fields.iter().find(|f| f.name == name)
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
  use super::*;

  fn declared(items: &[(&str, UniformType)]) -> BTreeMap<String, UniformType> {
    items.iter().map(|&(n, t)| (n.to_string(), t)).collect()
  }

  #[test]
  fn arrays_pack_as_vec4s() {
    let layout = Layout::new(&declared(&[
      ("a", UniformType::F32),
      ("rects", UniformType::Vec4Array(3)),
    ]))
    .unwrap();
    let a = layout.field("a").unwrap();
    let rects = layout.field("rects").unwrap();
    assert_eq!(rects.offset % 16, 0, "arrays are 16-byte aligned");
    assert!(rects.offset > a.offset);
    assert_eq!(rects.ty.floats(), 12);
    assert!(layout.size >= rects.offset + 48);
  }

  #[test]
  fn oversized_uniforms_are_rejected() {
    let e = Layout::new(&declared(&[("big", UniformType::Vec4Array(300))])).unwrap_err();
    assert!(e.contains("limit is 4096"), "{e}");
  }

  #[test]
  fn types_round_trip_through_serde() {
    for (text, ty) in [
      ("f32", UniformType::F32),
      ("vec3f", UniformType::Vec3),
      ("vec4f[24]", UniformType::Vec4Array(24)),
    ] {
      let json = format!("\"{text}\"");
      assert_eq!(serde_json::from_str::<UniformType>(&json).unwrap(), ty);
      assert_eq!(serde_json::to_string(&ty).unwrap(), json);
    }
    for bad in ["vec4f[0]", "vec4f[x]", "mat4", "vec4f[3"] {
      assert!(
        serde_json::from_str::<UniformType>(&format!("\"{bad}\"")).is_err(),
        "{bad}"
      );
    }
  }
}
