//! WGSL to MSL translation.
//!
//! From a spec's uniform declarations we generate a WGSL prelude (uniform struct, bindings,
//! fullscreen vertex stage), so authors only write the fragment function. See
//! `docs/SHADERS.md` for what the shader can read.

use crate::spec::{ShaderSpec, MAX_SOURCES};
use crate::uniforms::Layout;
use naga::back::msl;
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Entry point names: the generated vertex stage, the display function every shader writes, and
/// the optional simulation step that makes a shader stateful.
pub(crate) const VERTEX_ENTRY: &str = "vs";
pub(crate) const FRAGMENT_ENTRY: &str = "fs";
pub(crate) const SIM_ENTRY: &str = "sim";

/// A translated shader, ready to build Metal pipelines from.
#[derive(Debug, Clone)]
pub(crate) struct Compiled {
  pub(crate) msl: String,
  pub(crate) layout: Layout,
  /// Whether the WGSL defines `sim`.
  pub(crate) stateful: bool,
  /// The named sources, in binding order.
  pub(crate) sources: Vec<String>,
  /// The textures the shader actually reads (`screen`, `behind`, named sources): only these need
  /// capturing.
  pub(crate) reads: BTreeSet<String>,
  /// Whether it changes by itself (`sim`, `u.time`, `u.frame`), so needs drawing every frame.
  pub(crate) animated: bool,
  /// Whether it reads `u.mouse`, so needs drawing when the pointer moves.
  pub(crate) reads_mouse: bool,
}

/// Binding of the first named source; Metal texture slot is this minus 2.
const FIRST_SOURCE_BINDING: u32 = 5;
/// Names the prelude already uses.
const RESERVED: [&str; 5] = ["u", "samp", "screen", "state", "behind"];

fn check_source_name(name: &str) -> Result<(), String> {
  let mut chars = name.chars();
  let ident = chars
    .next()
    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
  if !ident || RESERVED.contains(&name) {
    return Err(format!("'{name}' can't be a source name"));
  }
  Ok(())
}

/// Whether `wgsl` mentions `name` as a whole identifier path (so `u.time` but not `u.timeout`).
fn mentions(wgsl: &str, name: &str) -> bool {
  wgsl.match_indices(name).any(|(at, _)| {
    let before = wgsl[..at].chars().next_back();
    let after = wgsl[at + name.len()..].chars().next();
    !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
      && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
  })
}

const VERTEX_STAGE: &str = "
struct VsOut { @builtin(position) pos: vec4f, @location(0) uv: vec2f }
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
  var o: VsOut;
  o.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
  o.uv = vec2f(p.x, 1.0 - p.y);
  return o;
}
";

/// The WGSL prepended to user source. `behind` adds the second capture; `sources` are named
/// textures after it.
pub(crate) fn prelude(layout: &Layout, behind: bool, sources: &[String]) -> String {
  let mut s = String::from("struct Uniforms {\n");
  for f in &layout.fields {
    let _ = writeln!(s, "  {}: {},", f.name, f.ty.wgsl());
  }
  s.push_str("}\n@group(0) @binding(0) var<uniform> u: Uniforms;\n");
  s.push_str("@group(0) @binding(1) var samp: sampler;\n@group(0) @binding(2) var screen: texture_2d<f32>;\n");
  s.push_str("@group(0) @binding(3) var state: texture_2d<f32>;\n");
  if behind {
    s.push_str("@group(0) @binding(4) var behind: texture_2d<f32>;\n");
  }
  for (binding, name) in (FIRST_SOURCE_BINDING..).zip(sources) {
    let _ = writeln!(
      s,
      "@group(0) @binding({binding}) var {name}: texture_2d<f32>;"
    );
  }
  s.push_str(VERTEX_STAGE);
  s
}

/// Validates and translates a spec. Errors are human-readable WGSL diagnostics.
pub(crate) fn compile(spec: &ShaderSpec) -> Result<Compiled, String> {
  let layout = Layout::new(&spec.uniforms)?;
  if spec.sources.len() > MAX_SOURCES {
    return Err(format!("at most {MAX_SOURCES} sources"));
  }
  let sources: Vec<String> = spec.sources.keys().cloned().collect();
  for name in &sources {
    check_source_name(name)?;
  }
  let source = format!(
    "{}{}",
    prelude(&layout, spec.behind.is_some(), &sources),
    spec.wgsl
  );
  let module = naga::front::wgsl::parse_str(&source).map_err(|e| e.emit_to_string(&source))?;
  let info = naga::valid::Validator::new(
    naga::valid::ValidationFlags::all(),
    naga::valid::Capabilities::empty(),
  )
  .validate(&module)
  .map_err(|e| e.emit_to_string(&source))?;
  let defines = |name: &str| {
    module
      .entry_points
      .iter()
      .any(|ep| ep.stage == naga::ShaderStage::Fragment && ep.name == name)
  };
  if !defines(FRAGMENT_ENTRY) {
    return Err(format!(
      "WGSL must define `@fragment fn {FRAGMENT_ENTRY}(in: VsOut) -> @location(0) vec4f`"
    ));
  }

  let fragment_resources = || fragment_resources(sources.len());
  let mut options = msl::Options {
    lang_version: (2, 4),
    ..Default::default()
  };
  options
    .per_entry_point_map
    .insert(FRAGMENT_ENTRY.into(), fragment_resources());
  options
    .per_entry_point_map
    .insert(SIM_ENTRY.into(), fragment_resources());
  options
    .per_entry_point_map
    .insert(VERTEX_ENTRY.into(), msl::EntryPointResources::default());

  let (msl, _) = msl::write_string(&module, &info, &options, &msl::PipelineOptions::default())
    .map_err(|e| e.to_string())?;
  Ok(Compiled {
    msl,
    layout,
    stateful: defines(SIM_ENTRY),
    reads: textures_read(&module, &info),
    animated: defines(SIM_ENTRY)
      || mentions(&spec.wgsl, "u.time")
      || mentions(&spec.wgsl, "u.frame"),
    reads_mouse: mentions(&spec.wgsl, "u.mouse"),
    sources,
  })
}

/// Where fragment stages find things in Metal: the uniform buffer, the sampler, then `screen`,
/// `state`, `behind` and the `sources` named sources at texture slots 0, 1, 2, 3...
fn fragment_resources(sources: usize) -> msl::EntryPointResources {
  let mut resources = msl::EntryPointResources::default();
  let mut bind = |binding: u32, target: msl::BindTarget| {
    resources
      .resources
      .insert(naga::ResourceBinding { group: 0, binding }, target);
  };
  bind(
    0,
    msl::BindTarget {
      buffer: Some(0),
      ..Default::default()
    },
  );
  bind(
    1,
    msl::BindTarget {
      sampler: Some(msl::BindSamplerTarget::Resource(0)),
      ..Default::default()
    },
  );
  for (binding, slot) in [(2, 0), (3, 1), (4, 2)] {
    bind(
      binding,
      msl::BindTarget {
        texture: Some(slot),
        ..Default::default()
      },
    );
  }
  for (binding, slot) in (FIRST_SOURCE_BINDING..).zip(3u8..).take(sources) {
    bind(
      binding,
      msl::BindTarget {
        texture: Some(slot),
        ..Default::default()
      },
    );
  }
  resources
}

/// The texture globals that the fragment entry points (or anything they call) read.
fn textures_read(module: &naga::Module, info: &naga::valid::ModuleInfo) -> BTreeSet<String> {
  let mut reads = BTreeSet::new();
  for (index, ep) in module.entry_points.iter().enumerate() {
    if ep.stage != naga::ShaderStage::Fragment {
      continue;
    }
    let uses = info.get_entry_point(index);
    for (handle, global) in module.global_variables.iter() {
      let is_texture = matches!(module.types[global.ty].inner, naga::TypeInner::Image { .. });
      if is_texture && !uses[handle].is_empty() {
        if let Some(name) = &global.name {
          reads.insert(name.clone());
        }
      }
    }
  }
  reads
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
  use super::*;
  use crate::spec::{Hide, Region};
  use crate::uniforms::UniformType;
  use std::collections::BTreeMap;

  fn spec(wgsl: &str, uniforms: &[(&str, UniformType)]) -> ShaderSpec {
    ShaderSpec {
      wgsl: wgsl.into(),
      uniforms: uniforms
        .iter()
        .map(|(n, t)| ((*n).to_string(), *t))
        .collect(),
      values: BTreeMap::new(),
      region: Region {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
      },
      hide: Hide::None,
      cell: None,
      steps: None,
      behind: None,
      sources: BTreeMap::new(),
      animate: None,
    }
  }

  const PASSTHROUGH: &str =
    "@fragment fn fs(in: VsOut) -> @location(0) vec4f { return textureSample(screen, samp, in.uv); }";

  fn rich() -> ShaderSpec {
    spec(
      "@fragment fn fs(in: VsOut) -> @location(0) vec4f {
        return textureSample(screen, samp, in.uv) * u.fade + vec4f(u.tint, u.after) * u.rects.x * u.aim.x * u.mouse.y;
      }",
      &[
        ("fade", UniformType::F32),
        ("aim", UniformType::Vec2),
        ("tint", UniformType::Vec3),
        ("rects", UniformType::Vec4),
        ("after", UniformType::F32),
      ],
    )
  }

  #[test]
  fn passthrough_compiles() {
    let c = compile(&spec(PASSTHROUGH, &[])).unwrap();
    assert!(c.msl.contains("fragment"), "{}", c.msl);
  }

  #[test]
  fn layout_matches_naga() {
    let s = rich();
    let c = compile(&s).unwrap();
    let source = format!("{}{}", prelude(&c.layout, false, &[]), s.wgsl);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let (_, ty) = module
      .types
      .iter()
      .find(|(_, t)| t.name.as_deref() == Some("Uniforms"))
      .unwrap();
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
      panic!("not a struct")
    };
    assert_eq!(members.len(), c.layout.fields.len());
    for (m, f) in members.iter().zip(&c.layout.fields) {
      assert_eq!(m.offset as usize, f.offset, "offset of {}", f.name);
    }
    assert!(*span as usize <= c.layout.size);
  }

  #[test]
  fn shader_errors_are_readable() {
    let e = compile(&spec(
      "@fragment fn fs(in: VsOut) -> @location(0) vec4f { return nope; }",
      &[],
    ))
    .unwrap_err();
    assert!(e.contains("nope"), "{e}");
  }

  #[test]
  fn bad_and_duplicate_uniform_names_rejected() {
    assert!(compile(&spec(PASSTHROUGH, &[("bad name", UniformType::F32)])).is_err());
    assert!(compile(&spec(PASSTHROUGH, &[("mouse", UniformType::Vec2)])).is_err());
  }

  #[test]
  fn behind_exists_only_when_asked_for() {
    let wgsl = "@fragment fn fs(in: VsOut) -> @location(0) vec4f { return textureSampleLevel(behind, samp, in.uv, 0.0); }";
    let mut s = spec(wgsl, &[]);
    assert!(
      compile(&s).unwrap_err().contains("behind"),
      "unknown without the option"
    );
    s.behind = Some(Hide::None);
    assert!(compile(&s).is_ok());
  }

  #[test]
  fn named_sources_are_textures_and_only_what_is_read_is_reported() {
    use crate::spec::Source;
    let wgsl = "fn pick() -> vec4f { return textureSampleLevel(win, samp, vec2f(0.0), 0.0); }
      @fragment fn fs(in: VsOut) -> @location(0) vec4f { return pick() * u.time; }";
    let mut s = spec(wgsl, &[]);
    s.sources
      .insert("win".into(), Some(Source::Window { window: 1 }));
    s.sources.insert("spare".into(), None);
    let c = compile(&s).unwrap();
    assert_eq!(c.sources, ["spare", "win"]);
    assert_eq!(
      c.reads.iter().collect::<Vec<_>>(),
      ["win"],
      "read through a helper; screen unread"
    );
    assert!(c.animated && !c.reads_mouse);

    s.sources.insert("screen".into(), None);
    assert!(compile(&s).unwrap_err().contains("source name"));
  }

  #[test]
  fn still_shaders_are_not_animated() {
    let c = compile(&spec(PASSTHROUGH, &[("timeout", UniformType::F32)])).unwrap();
    assert!(!c.animated && !c.reads_mouse);
    assert!(c.reads.contains("screen"));
    assert!(mentions("x * u.mouse.x", "u.mouse") && !mentions("u.timeout", "u.time"));
  }

  #[test]
  fn missing_fragment_function_is_explained() {
    let e = compile(&spec("fn helper() {}", &[])).unwrap_err();
    assert!(e.contains("@fragment fn fs"), "{e}");
  }

  #[test]
  fn spec_round_trips_through_json() {
    let json = r#"{"wgsl":"x","uniforms":{"a":"f32","b":"vec3f"},"values":{"a":[1.0]},"region":{"x":1,"y":2,"w":3,"h":4}}"#;
    let s: ShaderSpec = serde_json::from_str(json).unwrap();
    assert_eq!(s.uniforms["b"], UniformType::Vec3);
    assert!(serde_json::from_str::<ShaderSpec>(&json.replace("vec3f", "mat4")).is_err());
    assert_eq!(
      s.region,
      Region {
        x: 1.0,
        y: 2.0,
        w: 3.0,
        h: 4.0
      }
    );
    let back: ShaderSpec = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
  }
}
