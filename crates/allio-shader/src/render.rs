#![allow(unsafe_code)]

//! Draws the latest captured frame through a user pipeline into a `CAMetalLayer`.
//!
//! Everything a client can change (uniform values, the pipeline, the region) is plain state
//! behind one mutex: setting it overwrites, nothing is queued. Each vsync draws whatever the
//! state is at that moment.

#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // GPU uniforms are f32

use crate::capture::Frame;
use crate::diag::warn_once;
use crate::pipeline::{self, Compiled};
use crate::spec::{Region, ShaderSpec};
use crate::uniforms::UniformType;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::CGSize;
use objc2_core_graphics::CGEvent;
use objc2_foundation::NSString;
use objc2_metal::{
  MTLBlitCommandEncoder, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
  MTLDevice, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
  MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
  MTLRenderPipelineState, MTLSamplerAddressMode, MTLSamplerDescriptor, MTLSamplerMinMagFilter,
  MTLSamplerState, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
  MTLTextureUsage,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;

/// What a built pipeline was made from; a new spec with the same key needs no rebuild.
#[derive(PartialEq)]
struct Key {
  wgsl: String,
  uniforms: BTreeMap<String, UniformType>,
}

impl Key {
  fn of(spec: &ShaderSpec) -> Self {
    Self {
      wgsl: spec.wgsl.clone(),
      uniforms: spec.uniforms.clone(),
    }
  }
}

/// A shader built into Metal objects, with its packed uniform bytes.
pub(crate) struct Pipeline {
  key: Key,
  compiled: Compiled,
  pso: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
  uniforms: Vec<u8>,
}

struct State {
  pipeline: Pipeline,
  /// Private copy of the latest frame, so the capture surface is released immediately and the
  /// next draw can redraw a static screen.
  latest: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
  started: Instant,
  region: Region,
}

struct Inner {
  device: Retained<ProtocolObject<dyn MTLDevice>>,
  queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
  layer: Retained<CAMetalLayer>,
  sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
  scale: f64,
  state: Mutex<State>,
}

// Metal devices, queues, pipelines and CAMetalLayer.nextDrawable are thread-safe; all mutable
// state is behind `state`.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

/// Cheap to clone; usable from any thread.
#[derive(Clone)]
pub(crate) struct Renderer(Arc<Inner>);

fn ns(s: &str) -> Retained<NSString> {
  NSString::from_str(s)
}

/// Compiles a spec and builds its Metal pipeline, with the spec's initial values applied.
/// Needs no window, so invalid specs are rejected before anything visible exists.
pub(crate) fn build(
  device: &ProtocolObject<dyn MTLDevice>,
  spec: &ShaderSpec,
) -> Result<Pipeline, String> {
  let compiled = pipeline::compile(spec)?;
  let library = device
    .newLibraryWithSource_options_error(&ns(&compiled.msl), None)
    .map_err(|e| {
      format!(
        "Metal rejected generated shader: {}",
        e.localizedDescription()
      )
    })?;
  let vertex = library
    .newFunctionWithName(&ns(pipeline::VERTEX_ENTRY))
    .ok_or("missing vertex function")?;
  let fragment = library
    .newFunctionWithName(&ns(pipeline::FRAGMENT_ENTRY))
    .ok_or("missing fragment function")?;

  let desc = MTLRenderPipelineDescriptor::new();
  desc.setVertexFunction(Some(&vertex));
  desc.setFragmentFunction(Some(&fragment));
  unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }
    .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
  let pso = device
    .newRenderPipelineStateWithDescriptor_error(&desc)
    .map_err(|e| e.localizedDescription().to_string())?;

  let uniforms = vec![0; compiled.layout.size];
  let mut pipeline = Pipeline {
    key: Key::of(spec),
    compiled,
    pso,
    uniforms,
  };
  apply(&mut pipeline, &spec.values)?;
  Ok(pipeline)
}

/// Wraps a capture surface as a texture without copying.
fn wrap(
  device: &ProtocolObject<dyn MTLDevice>,
  frame: &Frame,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
  let desc = unsafe {
    MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
      MTLPixelFormat::BGRA8Unorm,
      frame.width,
      frame.height,
      false,
    )
  };
  desc.setUsage(MTLTextureUsage::ShaderRead);
  device.newTextureWithDescriptor_iosurface_plane(&desc, &frame.surface, 0)
}

/// Cursor position in screen points, top-left origin.
fn mouse() -> [f32; 2] {
  CGEvent::new(None).map_or([0.0; 2], |e| {
    let p = CGEvent::location(Some(&e));
    [p.x as f32, p.y as f32]
  })
}

fn put(buf: &mut [u8], offset: usize, vals: &[f32]) {
  for (i, v) in vals.iter().enumerate() {
    let at = offset + i * 4;
    if let Some(dst) = buf.get_mut(at..at + 4) {
      dst.copy_from_slice(&v.to_le_bytes());
    }
  }
}

/// Writes named values into the packed buffer. Applies every valid entry and reports the first bad one.
fn apply(pipeline: &mut Pipeline, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
  let mut first_err = None;
  for (name, vals) in values {
    match pipeline.compiled.layout.field(name) {
      None => first_err = first_err.or(Some(format!("unknown uniform '{name}'"))),
      Some(f) if f.ty.floats() != vals.len() => {
        first_err = first_err.or(Some(format!(
          "uniform '{name}' expects {} floats, got {}",
          f.ty.floats(),
          vals.len()
        )));
      }
      Some(f) => put(&mut pipeline.uniforms, f.offset, vals),
    }
  }
  first_err.map_or(Ok(()), Err)
}

impl Renderer {
  pub(crate) fn new(
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    layer: Retained<CAMetalLayer>,
    scale: f64,
    pipeline: Pipeline,
    region: Region,
  ) -> Result<Self, String> {
    let queue = device.newCommandQueue().ok_or("no command queue")?;
    let sd = MTLSamplerDescriptor::new();
    sd.setMinFilter(MTLSamplerMinMagFilter::Linear);
    sd.setMagFilter(MTLSamplerMinMagFilter::Linear);
    sd.setSAddressMode(MTLSamplerAddressMode::ClampToEdge);
    sd.setTAddressMode(MTLSamplerAddressMode::ClampToEdge);
    let sampler = device
      .newSamplerStateWithDescriptor(&sd)
      .ok_or("no sampler")?;

    let state = State {
      pipeline,
      latest: None,
      started: Instant::now(),
      region,
    };
    Ok(Self(Arc::new(Inner {
      device,
      queue,
      layer,
      sampler,
      scale,
      state: Mutex::new(state),
    })))
  }

  /// Applies a changed spec's pipeline and values. The pipeline is rebuilt only if the WGSL or
  /// declarations changed; on failure the previous one keeps running.
  pub(crate) fn update(&self, spec: &ShaderSpec) -> Result<(), String> {
    let mut st = self.0.state.lock();
    if st.pipeline.key == Key::of(spec) {
      apply(&mut st.pipeline, &spec.values)
    } else {
      build(&self.0.device, spec).map(|next| st.pipeline = next)
    }
  }

  /// The hot path: overwrite uniform values. Latest wins.
  pub(crate) fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
    apply(&mut self.0.state.lock().pipeline, values)
  }

  /// Resizes the output for a new region. Returns whether the region changed.
  pub(crate) fn set_region(&self, region: Region) -> bool {
    let mut st = self.0.state.lock();
    if st.region == region {
      return false;
    }
    st.region = region;
    let scale = self.0.scale;
    self.0.layer.setDrawableSize(CGSize::new(
      (region.w * scale).round(),
      (region.h * scale).round(),
    ));
    true
  }

  /// Copies a new frame into the private texture. The capture surface is released as soon as
  /// the copy completes.
  pub(crate) fn present(&self, frame: Frame) {
    let i = &*self.0;
    let mut st = i.state.lock();
    let stale = st
      .latest
      .as_ref()
      .is_none_or(|t| t.width() != frame.width || t.height() != frame.height);
    if stale {
      let d = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
          MTLPixelFormat::BGRA8Unorm,
          frame.width,
          frame.height,
          false,
        )
      };
      d.setUsage(MTLTextureUsage::ShaderRead);
      d.setStorageMode(MTLStorageMode::Private);
      st.latest = i.device.newTextureWithDescriptor(&d);
    }
    let Some(dst) = st.latest.as_ref() else {
      return warn_once("render: frame texture", "could not allocate");
    };
    let Some(source) = wrap(&i.device, &frame) else {
      return warn_once("render: frame", "Metal could not wrap the IOSurface");
    };
    let Some(cb) = i.queue.commandBuffer() else {
      return warn_once("render: frame copy", "no command buffer");
    };
    let Some(blit) = cb.blitCommandEncoder() else {
      return warn_once("render: frame copy", "no blit encoder");
    };
    let origin = MTLOrigin { x: 0, y: 0, z: 0 };
    unsafe {
      blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
        &source, 0, 0, origin, MTLSize { width: frame.width, height: frame.height, depth: 1 }, dst, 0, 0, origin,
      );
    }
    blit.endEncoding();

    // Hold the capture surface until the GPU has finished copying it.
    let slot = Mutex::new(Some(frame));
    let done = RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
      drop(slot.lock().take());
    });
    unsafe { cb.addCompletedHandler(RcBlock::as_ptr(&done)) };
    cb.commit();
  }

  /// Draws the current state into `drawable`. Called on every vsync.
  pub(crate) fn draw(&self, drawable: &ProtocolObject<dyn CAMetalDrawable>) {
    let i = &*self.0;
    let mut st = i.state.lock();
    let Some(src) = st.latest.clone() else { return };
    let Some(cb) = i.queue.commandBuffer() else {
      return warn_once("render: draw", "no command buffer");
    };

    let dst = drawable.texture();
    let (time, region, m) = (st.started.elapsed().as_secs_f32(), st.region, mouse());
    let p = &mut st.pipeline;
    let res = [dst.width() as f32, dst.height() as f32];
    let r = [
      region.x as f32,
      region.y as f32,
      region.w as f32,
      region.h as f32,
    ];
    for (name, vals) in [
      ("resolution", &res[..]),
      ("time", &[time][..]),
      ("mouse", &m[..]),
      ("region", &r[..]),
    ] {
      if let Some(f) = p.compiled.layout.field(name) {
        put(&mut p.uniforms, f.offset, vals);
      }
    }

    let pass = MTLRenderPassDescriptor::renderPassDescriptor();
    let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(&dst));
    color.setLoadAction(MTLLoadAction::Clear);
    color.setStoreAction(MTLStoreAction::Store);
    color.setClearColor(MTLClearColor {
      red: 0.0,
      green: 0.0,
      blue: 0.0,
      alpha: 0.0,
    });
    let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else {
      return warn_once("render: draw", "no render encoder");
    };
    enc.setRenderPipelineState(&p.pso);
    unsafe {
      enc.setFragmentBytes_length_atIndex(
        NonNull::from(p.uniforms.as_slice()).cast(),
        p.uniforms.len(),
        0,
      );
      enc.setFragmentSamplerState_atIndex(Some(&i.sampler), 0);
      enc.setFragmentTexture_atIndex(Some(&src), 0);
      enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
    }
    enc.endEncoding();
    cb.presentDrawable(ProtocolObject::from_ref(drawable));
    cb.commit();
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
  use super::*;
  use objc2_metal::MTLCreateSystemDefaultDevice;

  const PASSTHROUGH: &str =
    "@fragment fn fs(in: VsOut) -> @location(0) vec4f { return textureSample(screen, samp, in.uv); }";
  const WITH_FADE: &str = "@fragment fn fs(in: VsOut) -> @location(0) vec4f {
    return textureSample(screen, samp, in.uv) * u.fade;
  }";

  fn spec(wgsl: &str, uniforms: &[(&str, UniformType)]) -> ShaderSpec {
    ShaderSpec {
      wgsl: wgsl.into(),
      uniforms: uniforms.iter().map(|&(n, t)| (n.to_string(), t)).collect(),
      values: BTreeMap::new(),
      region: Region {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
      },
    }
  }

  fn with_values(mut s: ShaderSpec, values: &[(&str, &[f32])]) -> ShaderSpec {
    s.values = values
      .iter()
      .map(|&(n, v)| (n.to_string(), v.to_vec()))
      .collect();
    s
  }

  /// A renderer without a window or capture stream: needs Metal, not Screen Recording.
  fn renderer(spec: &ShaderSpec) -> Option<Renderer> {
    let device = MTLCreateSystemDefaultDevice()?;
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    let pipeline = build(&device, spec).unwrap();
    Some(Renderer::new(device, layer, 2.0, pipeline, spec.region).unwrap())
  }

  fn read(r: &Renderer, name: &str) -> Vec<f32> {
    let st = r.0.state.lock();
    let f = st.pipeline.compiled.layout.field(name).unwrap();
    (0..f.ty.floats())
      .map(|i| {
        let at = f.offset + i * 4;
        f32::from_le_bytes(st.pipeline.uniforms[at..at + 4].try_into().unwrap())
      })
      .collect()
  }

  #[test]
  fn generated_shaders_build_in_metal() {
    let Some(device) = MTLCreateSystemDefaultDevice() else {
      return;
    };
    build(&device, &spec(PASSTHROUGH, &[])).unwrap();
    let rich = spec(
      "@fragment fn fs(in: VsOut) -> @location(0) vec4f {
        let c = textureSample(screen, samp, in.uv);
        return vec4f(1.0 - c.rgb, 1.0) * u.fade + u.rects * u.mouse.x + vec4f(u.resolution, u.time, 0.0);
      }",
      &[("fade", UniformType::F32), ("rects", UniformType::Vec4)],
    );
    build(&device, &rich).unwrap();
  }

  #[test]
  fn bad_initial_values_are_rejected() {
    let Some(device) = MTLCreateSystemDefaultDevice() else {
      return;
    };
    let s = with_values(
      spec(WITH_FADE, &[("fade", UniformType::F32)]),
      &[("fade", &[1.0, 2.0])],
    );
    assert!(build(&device, &s)
      .err()
      .unwrap()
      .contains("expects 1 floats"));
    let s = with_values(spec(PASSTHROUGH, &[]), &[("nope", &[1.0])]);
    assert!(build(&device, &s)
      .err()
      .unwrap()
      .contains("unknown uniform"));
  }

  #[test]
  fn values_are_validated_and_latest_wins() {
    let base = spec(WITH_FADE, &[("fade", UniformType::F32)]);
    let Some(r) = renderer(&base) else { return };
    let set =
      |name: &str, v: &[f32]| r.set_values(&BTreeMap::from([(name.to_string(), v.to_vec())]));
    set("fade", &[0.25]).unwrap();
    set("fade", &[0.75]).unwrap();
    assert_eq!(read(&r, "fade"), [0.75]);
    assert!(set("fade", &[1.0, 2.0]).is_err());
    assert!(set("nope", &[1.0]).is_err());
    assert_eq!(read(&r, "fade"), [0.75], "rejected values change nothing");
  }

  #[test]
  fn update_rebuilds_only_when_the_shader_changes() {
    let base = spec(WITH_FADE, &[("fade", UniformType::F32)]);
    let Some(r) = renderer(&base) else { return };

    // Same shader, new value: no rebuild.
    r.update(&with_values(base.clone(), &[("fade", &[0.5])]))
      .unwrap();
    assert_eq!(read(&r, "fade"), [0.5]);

    // Edited shader: rebuilt, with the new spec's values.
    let edited = with_values(
      spec(
        &WITH_FADE.replace("* u.fade", "* u.fade * 0.5"),
        &[("fade", UniformType::F32)],
      ),
      &[("fade", &[0.9])],
    );
    r.update(&edited).unwrap();
    assert!(r.0.state.lock().pipeline.key.wgsl.contains("* 0.5"));
    assert_eq!(read(&r, "fade"), [0.9]);
  }

  #[test]
  fn failed_rebuild_keeps_the_running_shader() {
    let base = spec(WITH_FADE, &[("fade", UniformType::F32)]);
    let Some(r) = renderer(&with_values(base.clone(), &[("fade", &[0.5])])) else {
      return;
    };

    let mut broken = base.clone();
    broken.wgsl = "@fragment fn fs(in: VsOut) -> @location(0) vec4f { return nope; }".into();
    let err = r.update(&broken).unwrap_err();
    assert!(err.contains("nope"), "{err}");
    assert_eq!(
      r.0.state.lock().pipeline.key.wgsl,
      WITH_FADE,
      "old pipeline still runs"
    );
    assert_eq!(read(&r, "fade"), [0.5]);
  }

  #[test]
  fn set_region_reports_changes_and_resizes_the_output() {
    let Some(r) = renderer(&spec(PASSTHROUGH, &[])) else {
      return;
    };
    let moved = Region {
      x: 5.0,
      y: 5.0,
      w: 20.0,
      h: 10.0,
    };
    assert!(r.set_region(moved));
    assert!(!r.set_region(moved));
    let size = r.0.layer.drawableSize();
    assert_eq!(
      (size.width, size.height),
      (40.0, 20.0),
      "points times the 2x scale"
    );
  }
}
