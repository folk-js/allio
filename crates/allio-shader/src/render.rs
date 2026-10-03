#![allow(unsafe_code)]

//! Draws the latest captured frame through a user pipeline into a `CAMetalLayer`.
//!
//! Everything a client can change (uniform values, the pipeline, the region) is plain state
//! behind one mutex: setting it overwrites, nothing is queued. Each vsync draws whatever the
//! state is at that moment.

#![allow(
  clippy::cast_possible_truncation,
  clippy::cast_precision_loss,
  clippy::cast_sign_loss
)] // GPU uniforms are f32; sizes are positive

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
  MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder,
  MTLCommandQueue, MTLDevice, MTLFunction, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat,
  MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
  MTLRenderPipelineState, MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerDescriptor,
  MTLSamplerMinMagFilter, MTLSamplerState, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture,
  MTLTextureDescriptor, MTLTextureUsage,
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
  behind: bool,
}

impl Key {
  fn of(spec: &ShaderSpec) -> Self {
    Self {
      wgsl: spec.wgsl.clone(),
      uniforms: spec.uniforms.clone(),
      behind: spec.behind.is_some(),
    }
  }
}

/// Which capture a frame belongs to.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Source {
  /// `screen`.
  Screen,
  /// `behind`.
  Behind,
}

/// A shader built into Metal objects, with its packed uniform bytes.
pub(crate) struct Pipeline {
  key: Key,
  compiled: Compiled,
  pso: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
  /// Present if the shader defines `sim`.
  sim: Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
  uniforms: Vec<u8>,
}

/// Format of the `state` texture: enough range and precision for simulation values.
const STATE_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA32Float;

/// Bytes in one texel of the state texture.
const TEXEL_BYTES: usize = 16;

/// Default size of a `state` cell in screen points.
const DEFAULT_CELL: f32 = 4.0;
/// Largest state texture side, in cells.
const MAX_CELLS: usize = 4096;

/// The simulation state: two textures, one read while the other is written, then swapped.
struct Sim {
  /// The latest state.
  current: Retained<ProtocolObject<dyn MTLTexture>>,
  /// Written by the next step, then swapped with `current`.
  next: Retained<ProtocolObject<dyn MTLTexture>>,
  /// Size in cells.
  size: (usize, usize),
}

struct State {
  pipeline: Pipeline,
  sim: Option<Sim>,
  /// Size in screen points of one `state` cell.
  cell: f32,
  /// Draws so far, exposed as `u.frame`.
  frames: u64,
  /// Private copy of the latest frame, so the capture surface is released immediately and the
  /// next draw can redraw a static screen.
  latest: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
  /// The same for the `behind` capture, if the shader has one.
  behind: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
  /// Simulation steps per frame.
  steps: u32,
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

  let make = |fragment: &ProtocolObject<dyn MTLFunction>, format: MTLPixelFormat| {
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vertex));
    desc.setFragmentFunction(Some(fragment));
    unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(format);
    device
      .newRenderPipelineStateWithDescriptor_error(&desc)
      .map_err(|e| e.localizedDescription().to_string())
  };
  let pso = make(&fragment, MTLPixelFormat::BGRA8Unorm)?;
  let sim = if compiled.stateful {
    let function = library
      .newFunctionWithName(&ns(pipeline::SIM_ENTRY))
      .ok_or("missing sim function")?;
    Some(make(&function, STATE_FORMAT)?)
  } else {
    None
  };

  let uniforms = vec![0; compiled.layout.size];
  let mut pipeline = Pipeline {
    key: Key::of(spec),
    compiled,
    pso,
    sim,
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

#[cfg(test)]
thread_local! {
  /// Lets tests place the cursor.
  static MOUSE: std::cell::Cell<Option<[f32; 2]>> = const { std::cell::Cell::new(None) };
}

/// Cursor position in screen points, top-left origin.
fn mouse() -> [f32; 2] {
  #[cfg(test)]
  if let Some(m) = MOUSE.with(std::cell::Cell::get) {
    return m;
  }
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
    cell: Option<f32>,
    steps: u32,
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
      sim: None,
      cell: cell.unwrap_or(DEFAULT_CELL).max(0.5),
      frames: 0,
      latest: None,
      behind: None,
      steps,
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
    st.cell = spec.cell.unwrap_or(DEFAULT_CELL).max(0.5);
    st.steps = spec.steps();
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

  /// Reads one cell of the simulation state at a screen point: `[r, g, b, a]` as the `sim`
  /// function last wrote it. Blocks until the GPU has caught up.
  pub(crate) fn probe(&self, x: f64, y: f64) -> Result<[f32; 4], String> {
    let i = &*self.0;
    let st = i.state.lock();
    let sim = st.sim.as_ref().ok_or("the shader has no state to probe")?;
    let cell = |point: f64, origin: f64, count: usize| {
      ((point - origin) / f64::from(st.cell))
        .floor()
        .clamp(0.0, (count - 1) as f64) as usize
    };
    let at = MTLOrigin {
      x: cell(x, st.region.x, sim.size.0),
      y: cell(y, st.region.y, sim.size.1),
      z: 0,
    };

    // One RGBA32F texel is 16 bytes.
    let buffer = i
      .device
      .newBufferWithLength_options(TEXEL_BYTES, MTLResourceOptions::StorageModeShared)
      .ok_or("no buffer")?;
    let cb = i.queue.commandBuffer().ok_or("no command buffer")?;
    let blit = cb.blitCommandEncoder().ok_or("no blit encoder")?;
    unsafe {
      blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
        &sim.current, 0, 0, at, MTLSize { width: 1, height: 1, depth: 1 }, &buffer, 0, TEXEL_BYTES, TEXEL_BYTES,
      );
    }
    blit.endEncoding();
    cb.commit();
    cb.waitUntilCompleted();

    let bytes =
      unsafe { std::slice::from_raw_parts(buffer.contents().as_ptr().cast::<u8>(), TEXEL_BYTES) };
    let mut cell = [0.0; 4];
    for (value, word) in cell.iter_mut().zip(bytes.as_chunks::<4>().0) {
      *value = f32::from_le_bytes(*word);
    }
    Ok(cell)
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
  pub(crate) fn present(&self, source: Source, frame: Frame) {
    let i = &*self.0;
    let mut st = i.state.lock();
    let slot = match source {
      Source::Screen => &mut st.latest,
      Source::Behind => &mut st.behind,
    };
    let stale = slot
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
      *slot = i.device.newTextureWithDescriptor(&d);
    }
    let Some(dst) = slot.as_ref() else {
      return warn_once("render: frame texture", "could not allocate");
    };
    let Some(surface) = wrap(&i.device, &frame) else {
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
        &surface, 0, 0, origin, MTLSize { width: frame.width, height: frame.height, depth: 1 }, dst, 0, 0, origin,
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
    if let Some(cb) = self.encode(&drawable.texture()) {
      cb.presentDrawable(ProtocolObject::from_ref(drawable));
      cb.commit();
    }
  }

  /// Encodes a simulation step (if the shader has one) and a draw into `target`.
  fn encode(
    &self,
    target: &ProtocolObject<dyn MTLTexture>,
  ) -> Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
    let i = &*self.0;
    let mut st = i.state.lock();
    let src = st.latest.clone()?;
    let Some(cb) = i.queue.commandBuffer() else {
      warn_once("render: draw", "no command buffer");
      return None;
    };

    let State {
      pipeline: p,
      sim,
      cell,
      frames,
      started,
      region,
      behind,
      steps,
      ..
    } = &mut *st;
    // Until the second capture has had a frame, `behind` is the screen.
    let behind = behind.as_ref().unwrap_or(&src);

    // Size of the state texture, in cells, and (re)allocate it when that changes.
    let cells = |points: f64| {
      (points / f64::from(*cell))
        .ceil()
        .clamp(1.0, MAX_CELLS as f64) as usize
    };
    let size = (cells(region.w), cells(region.h));
    if p.sim.is_some() && sim.as_ref().is_none_or(|s| s.size != size) {
      *sim = Sim::new(&i.device, &cb, size);
    }
    if p.sim.is_none() {
      *sim = None;
    }

    *frames += 1;
    let res = [target.width() as f32, target.height() as f32];
    let r = [
      region.x as f32,
      region.y as f32,
      region.w as f32,
      region.h as f32,
    ];
    let state_size = [size.0 as f32, size.1 as f32];
    for (name, vals) in [
      ("resolution", &res[..]),
      ("time", &[started.elapsed().as_secs_f32()][..]),
      ("mouse", &mouse()[..]),
      ("region", &r[..]),
      ("frame", &[*frames as f32][..]),
      ("state_size", &state_size[..]),
    ] {
      if let Some(f) = p.compiled.layout.field(name) {
        put(&mut p.uniforms, f.offset, vals);
      }
    }

    // Advance the simulation: each step reads `current`, writes `next`, and swaps.
    if let (Some(step), Some(sim)) = (&p.sim, sim.as_mut()) {
      for _ in 0..*steps {
        let pass = Pass {
          pso: step,
          uniforms: &p.uniforms,
          sampler: &i.sampler,
          screen: &src,
          state: &sim.current,
          behind,
        };
        if !pass.encode(&cb, &sim.next) {
          warn_once("render: sim", "no render encoder");
          return None;
        }
        std::mem::swap(&mut sim.current, &mut sim.next);
      }
    }

    let latest_state = sim.as_ref().map(|s| &s.current);
    let pass = Pass {
      pso: &p.pso,
      uniforms: &p.uniforms,
      sampler: &i.sampler,
      screen: &src,
      // A stateless shader never reads `state`, but the slot still needs a texture.
      state: latest_state.unwrap_or(&src),
      behind,
    };
    if !pass.encode(&cb, target) {
      warn_once("render: draw", "no render encoder");
      return None;
    }
    Some(cb)
  }
}

impl Sim {
  /// Allocates both textures and clears them to zero (private memory is not zeroed).
  fn new(
    device: &ProtocolObject<dyn MTLDevice>,
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    size: (usize, usize),
  ) -> Option<Self> {
    let make = || {
      let d = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
          STATE_FORMAT,
          size.0,
          size.1,
          false,
        )
      };
      d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
      d.setStorageMode(MTLStorageMode::Private);
      device.newTextureWithDescriptor(&d)
    };
    let (current, next) = (make()?, make()?);
    for texture in [&current, &next] {
      let pass = MTLRenderPassDescriptor::renderPassDescriptor();
      let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
      color.setTexture(Some(texture));
      color.setLoadAction(MTLLoadAction::Clear);
      color.setStoreAction(MTLStoreAction::Store);
      color.setClearColor(MTLClearColor {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
      });
      cb.renderCommandEncoderWithDescriptor(&pass)?.endEncoding();
    }
    Some(Self {
      current,
      next,
      size,
    })
  }
}

/// One full-screen draw of a fragment function.
struct Pass<'a> {
  pso: &'a ProtocolObject<dyn MTLRenderPipelineState>,
  uniforms: &'a [u8],
  sampler: &'a ProtocolObject<dyn MTLSamplerState>,
  screen: &'a ProtocolObject<dyn MTLTexture>,
  state: &'a ProtocolObject<dyn MTLTexture>,
  behind: &'a ProtocolObject<dyn MTLTexture>,
}

impl Pass<'_> {
  /// Draws into `target`, starting from transparent. Returns false if no encoder was available.
  fn encode(
    &self,
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    target: &ProtocolObject<dyn MTLTexture>,
  ) -> bool {
    let pass = MTLRenderPassDescriptor::renderPassDescriptor();
    let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(target));
    color.setLoadAction(MTLLoadAction::Clear);
    color.setStoreAction(MTLStoreAction::Store);
    color.setClearColor(MTLClearColor {
      red: 0.0,
      green: 0.0,
      blue: 0.0,
      alpha: 0.0,
    });
    let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else {
      return false;
    };
    enc.setRenderPipelineState(self.pso);
    unsafe {
      enc.setFragmentBytes_length_atIndex(
        NonNull::from(self.uniforms).cast(),
        self.uniforms.len(),
        0,
      );
      enc.setFragmentSamplerState_atIndex(Some(self.sampler), 0);
      enc.setFragmentTexture_atIndex(Some(self.screen), 0);
      enc.setFragmentTexture_atIndex(Some(self.state), 1);
      enc.setFragmentTexture_atIndex(Some(self.behind), 2);
      enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
    }
    enc.endEncoding();
    true
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
  use super::*;
  use crate::spec::Hide;
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
      hide: Hide::None,
      cell: None,
      steps: None,
      behind: None,
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
    Some(
      Renderer::new(
        device,
        layer,
        2.0,
        pipeline,
        spec.region,
        spec.cell,
        spec.steps(),
      )
      .unwrap(),
    )
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

  /// Renders `r` into a 1x1 texture and returns its red channel as 0..=255.
  fn render_red(r: &Renderer, target: &ProtocolObject<dyn MTLTexture>) -> u8 {
    let cb = r.encode(target).unwrap();
    cb.commit();
    cb.waitUntilCompleted();
    let mut px = [0u8; 4];
    let region = objc2_metal::MTLRegion {
      origin: MTLOrigin { x: 0, y: 0, z: 0 },
      size: MTLSize {
        width: 1,
        height: 1,
        depth: 1,
      },
    };
    unsafe {
      target.getBytes_bytesPerRow_fromRegion_mipmapLevel(
        NonNull::from(&mut px).cast(),
        4,
        region,
        0,
      );
    }
    px[2] // BGRA
  }

  fn offscreen(device: &ProtocolObject<dyn MTLDevice>) -> Retained<ProtocolObject<dyn MTLTexture>> {
    let d = unsafe {
      MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
        MTLPixelFormat::BGRA8Unorm,
        1,
        1,
        false,
      )
    };
    d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    d.setStorageMode(MTLStorageMode::Shared);
    device.newTextureWithDescriptor(&d).unwrap()
  }

  /// The simulation runs once per draw, and `state` carries over between draws.
  #[test]
  fn stateful_shaders_accumulate_across_draws() {
    let counter = "
      @fragment fn sim(in: VsOut) -> @location(0) vec4f {
        return vec4f(textureLoad(state, vec2i(0, 0), 0).r + 0.1, u.state_size.x, u.frame, 1.0);
      }
      @fragment fn fs(in: VsOut) -> @location(0) vec4f {
        return vec4f(textureLoad(state, vec2i(0, 0), 0).r, 0.0, 0.0, 1.0);
      }";
    let s = spec(counter, &[]);
    let Some(r) = renderer(&s) else { return };
    let target = offscreen(&r.0.device);
    // The renderer needs one captured frame before it draws; any texture will do.
    r.0.state.lock().latest = Some(offscreen(&r.0.device));

    // 0.1 per step, shown after each draw (the step runs before the display pass).
    let seen: Vec<u8> = (0..3).map(|_| render_red(&r, &target)).collect();
    for (got, want) in seen.iter().zip([26i32, 51, 77]) {
      assert!(
        (i32::from(*got) - want).abs() <= 1,
        "0.1, 0.2, 0.3 of 255, got {seen:?}"
      );
    }
  }

  #[test]
  fn state_size_follows_the_region_and_cell() {
    let mut s = spec(PASSTHROUGH, &[]);
    s.region = Region {
      x: 0.0,
      y: 0.0,
      w: 100.0,
      h: 50.0,
    };
    s.cell = Some(10.0);
    let Some(r) = renderer(&s) else { return };
    let sim = "@fragment fn sim(in: VsOut) -> @location(0) vec4f { return vec4f(0.0); }
      @fragment fn fs(in: VsOut) -> @location(0) vec4f { return vec4f(u.state_size.x / 255.0, u.state_size.y / 255.0, 0.0, 1.0); }";
    let mut stateful = s.clone();
    stateful.wgsl = sim.into();
    r.update(&stateful).unwrap();
    let target = offscreen(&r.0.device);
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    // 100x50 points at 10 points per cell is 10x5 cells; red shows the width.
    assert_eq!(render_red(&r, &target), 10);
    let size = r.0.state.lock().sim.as_ref().map(|s| s.size);
    assert_eq!(size, Some((10, 5)));
  }

  // --- the demo shaders in src-web/shaders, run for real ---

  /// A demo's `name.json`: its uniform declarations and options.
  #[derive(serde::Deserialize)]
  struct Manifest {
    uniforms: BTreeMap<String, UniformType>,
    #[serde(default)]
    hide: Hide,
    cell: Option<f32>,
    steps: Option<u32>,
    behind: Option<Hide>,
  }

  const DEMOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-web/shaders");

  /// The demo as the page would declare it, over a 200x100 point region.
  fn demo_spec(name: &str) -> ShaderSpec {
    let wgsl = std::fs::read_to_string(format!("{DEMOS}/{name}.wgsl")).unwrap();
    let json = std::fs::read_to_string(format!("{DEMOS}/{name}.json")).unwrap();
    let m: Manifest = serde_json::from_str(&json).unwrap();
    ShaderSpec {
      wgsl,
      uniforms: m.uniforms,
      values: BTreeMap::new(),
      region: Region {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
      },
      hide: m.hide,
      cell: m.cell,
      steps: m.steps,
      behind: m.behind,
    }
  }

  #[test]
  fn every_demo_shader_builds() {
    let Some(device) = MTLCreateSystemDefaultDevice() else {
      return;
    };
    let mut names: Vec<_> = std::fs::read_dir(DEMOS)
      .unwrap()
      .filter_map(|e| {
        let path = e.unwrap().path();
        (path.extension()? == "wgsl")
          .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
      })
      .collect();
    names.sort();
    assert!(names.len() >= 4, "{names:?}");
    for name in names {
      build(&device, &demo_spec(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
  }

  /// The simulation state as `(width, height, cells)`, each cell `[r, g, b, a]`.
  fn read_state(r: &Renderer) -> (usize, usize, Vec<[f32; 4]>) {
    let st = r.0.state.lock();
    let sim = st.sim.as_ref().unwrap();
    let (w, h) = sim.size;
    let d = unsafe {
      MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
        STATE_FORMAT,
        w,
        h,
        false,
      )
    };
    d.setStorageMode(MTLStorageMode::Shared);
    let copy = r.0.device.newTextureWithDescriptor(&d).unwrap();
    let cb = r.0.queue.commandBuffer().unwrap();
    let blit = cb.blitCommandEncoder().unwrap();
    unsafe { blit.copyFromTexture_toTexture(&sim.current, &copy) };
    blit.endEncoding();
    cb.commit();
    cb.waitUntilCompleted();

    let mut bytes = vec![0u8; w * h * TEXEL_BYTES];
    let region = objc2_metal::MTLRegion {
      origin: MTLOrigin { x: 0, y: 0, z: 0 },
      size: MTLSize {
        width: w,
        height: h,
        depth: 1,
      },
    };
    unsafe {
      copy.getBytes_bytesPerRow_fromRegion_mipmapLevel(
        NonNull::new(bytes.as_mut_ptr()).unwrap().cast(),
        w * TEXEL_BYTES,
        region,
        0,
      );
    }
    let cells = bytes
      .as_chunks::<4>()
      .0
      .iter()
      .map(|word| f32::from_le_bytes(*word))
      .collect::<Vec<_>>()
      .as_chunks::<4>()
      .0
      .to_vec();
    (w, h, cells)
  }

  /// Renders the renderer's shader once into a `w` x `h` texture, one pixel per point, and
  /// returns the BGRA pixels in rows.
  fn render_image(r: &Renderer, w: usize, h: usize) -> Vec<[u8; 4]> {
    let d = unsafe {
      MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
        MTLPixelFormat::BGRA8Unorm,
        w,
        h,
        false,
      )
    };
    d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    d.setStorageMode(MTLStorageMode::Shared);
    let target = r.0.device.newTextureWithDescriptor(&d).unwrap();
    let cb = r.encode(&target).unwrap();
    cb.commit();
    cb.waitUntilCompleted();

    let mut bytes = vec![0u8; w * h * 4];
    let region = objc2_metal::MTLRegion {
      origin: MTLOrigin { x: 0, y: 0, z: 0 },
      size: MTLSize {
        width: w,
        height: h,
        depth: 1,
      },
    };
    unsafe {
      target.getBytes_bytesPerRow_fromRegion_mipmapLevel(
        NonNull::new(bytes.as_mut_ptr()).unwrap().cast(),
        w * 4,
        region,
        0,
      );
    }
    bytes.as_chunks::<4>().0.to_vec()
  }

  /// A `w` x `h` texture whose pixel at `(x, y)` is `colour(x, y)` (BGRA).
  fn picture(
    device: &ProtocolObject<dyn MTLDevice>,
    w: usize,
    h: usize,
    colour: impl Fn(usize, usize) -> [u8; 4],
  ) -> Retained<ProtocolObject<dyn MTLTexture>> {
    let d = unsafe {
      MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
        MTLPixelFormat::BGRA8Unorm,
        w,
        h,
        false,
      )
    };
    d.setUsage(MTLTextureUsage::ShaderRead);
    d.setStorageMode(MTLStorageMode::Shared);
    let texture = device.newTextureWithDescriptor(&d).unwrap();
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in 0..h {
      for x in 0..w {
        bytes.extend_from_slice(&colour(x, y));
      }
    }
    let region = objc2_metal::MTLRegion {
      origin: MTLOrigin { x: 0, y: 0, z: 0 },
      size: MTLSize {
        width: w,
        height: h,
        depth: 1,
      },
    };
    unsafe {
      texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
        region,
        0,
        NonNull::new(bytes.as_ptr().cast_mut()).unwrap().cast(),
        w * 4,
      );
    }
    texture
  }

  /// A one-colour screen.
  fn solid(
    device: &ProtocolObject<dyn MTLDevice>,
    bgra: [u8; 4],
  ) -> Retained<ProtocolObject<dyn MTLTexture>> {
    picture(device, 1, 1, |_, _| bgra)
  }

  /// Windows as the host would bind them: 24 slots, the first few filled.
  fn windows(rects: &[[f32; 4]]) -> Vec<f32> {
    let mut flat = vec![0.0; 96];
    for (slot, rect) in flat.chunks_mut(4).zip(rects) {
      slot.copy_from_slice(rect);
    }
    flat
  }

  fn set(r: &Renderer, pairs: &[(&str, Vec<f32>)]) {
    let values = pairs
      .iter()
      .map(|(n, v)| ((*n).to_string(), v.clone()))
      .collect();
    r.set_values(&values).unwrap();
  }

  /// Draws `n` frames, so a simulation settles.
  fn run(r: &Renderer, n: usize) {
    let target = offscreen(&r.0.device);
    for _ in 0..n {
      render_red(r, &target);
    }
  }

  // --- lava ---

  /// A platform in the middle of a 200x100 point screen, with a tap above it: lava falls, pools
  /// on the platform without entering it, and eats through. (Four point cells, one step a frame,
  /// so the geometry below is in cells of 4.)
  #[test]
  fn lava_pools_on_a_window_and_burns_through_it() {
    let mut spec = demo_spec("lava");
    spec.cell = Some(4.0);
    spec.steps = None;
    let Some(r) = renderer(&spec) else {
      return;
    };
    let target = offscreen(&r.0.device);
    r.0.state.lock().latest = Some(offscreen(&r.0.device));

    // One window (x 40..160, y 60..80), the rest of the array empty.
    set(
      &r,
      &[
        ("windows", windows(&[[40.0, 60.0, 120.0, 20.0]])),
        ("pour", vec![1.0]),
        ("radius", vec![8.0]),
        ("rate", vec![0.05]),
        ("reset", vec![0.0]),
      ],
    );
    MOUSE.with(|m| m.set(Some([100.0, 20.0])));

    let mut lava_after = Vec::new();
    for step in 0..400 {
      render_red(&r, &target);
      if step == 20 {
        // The stream is still in the air: above the platform there is lava only in its column
        // (cells 22..28 around the tap), not a haze spreading sideways.
        let (w, _, cells) = read_state(&r);
        let stray = (0..14)
          .flat_map(|y| (0..w).map(move |x| (x, y)))
          .filter(|&(x, y)| !(20..30).contains(&x) && cells[y * w + x][0] > 0.01)
          .count();
        assert_eq!(stray, 0, "lava spread sideways through the air");
      }
      if step == 40 {
        set(&r, &[("pour", vec![0.0])]); // stop pouring: what is there is all there will be
      }
      if step % 100 == 99 {
        let (_, _, cells) = read_state(&r);
        lava_after.push(cells.iter().map(|c| c[0]).sum::<f32>());
      }
    }

    let (w, _, cells) = read_state(&r);
    let at = |x: usize, y: usize| cells[y * w + x];
    // Cells of the window: x 10..40, y 15..20.
    let platform = || (10..40).flat_map(|x| (15..20).map(move |y| (x, y)));
    let burnt_through = platform().filter(|&(x, y)| at(x, y)[1] >= 1.0).count();
    let intact_with_lava = platform()
      .filter(|&(x, y)| at(x, y)[1] < 1.0 && at(x, y)[0] > 0.0)
      .count();

    assert!(
      lava_after[0] > 1.0,
      "the tap poured some lava: {lava_after:?}"
    );
    assert_eq!(
      intact_with_lava, 0,
      "lava never sits inside an intact window"
    );
    assert!(
      burnt_through > 0,
      "lava ate into the window; masses {lava_after:?}"
    );
    // Lava only leaves through the bottom edge: nothing is created after the tap is shut.
    assert!(
      lava_after.windows(2).all(|p| p[1] <= p[0] + 0.01),
      "mass never grows once the tap is off: {lava_after:?}"
    );
  }

  /// The same, with the demo's own finer cells and two steps a frame.
  #[test]
  fn lava_works_with_the_demos_own_settings() {
    let Some(r) = renderer(&demo_spec("lava")) else {
      return;
    };
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    set(
      &r,
      &[
        ("windows", windows(&[[40.0, 60.0, 120.0, 20.0]])),
        ("pour", vec![1.0]),
        ("radius", vec![8.0]),
        ("rate", vec![0.05]),
      ],
    );
    MOUSE.with(|m| m.set(Some([100.0, 20.0])));
    run(&r, 120);
    let (_, _, cells) = read_state(&r);
    assert!(cells.iter().map(|c| c[0]).sum::<f32>() > 1.0, "lava poured");
    assert!(
      cells.iter().any(|c| c[1] > 0.0),
      "and began to burn the window"
    );
    assert!(
      cells.iter().all(|c| c[0].is_finite() && c[0] < 2.0),
      "and stayed sane"
    );
  }

  #[test]
  fn reset_wipes_the_simulation() {
    let Some(r) = renderer(&demo_spec("lava")) else {
      return;
    };
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    MOUSE.with(|m| m.set(Some([100.0, 20.0])));
    set(&r, &[("pour", vec![1.0]), ("radius", vec![8.0])]);
    run(&r, 30);
    assert!(read_state(&r).2.iter().any(|c| c[0] > 0.0));
    set(&r, &[("pour", vec![0.0]), ("reset", vec![1.0])]);
    run(&r, 1);
    assert!(read_state(&r).2.iter().all(|c| c[0] == 0.0 && c[1] == 0.0));
  }

  // --- steps and probe ---

  #[test]
  fn steps_run_the_simulation_several_times_a_frame() {
    let counter = "
      @fragment fn sim(in: VsOut) -> @location(0) vec4f {
        return vec4f(textureLoad(state, vec2i(0, 0), 0).r + 0.1, 0.0, 0.0, 1.0);
      }
      @fragment fn fs(in: VsOut) -> @location(0) vec4f {
        return vec4f(textureLoad(state, vec2i(0, 0), 0).r, 0.0, 0.0, 1.0);
      }";
    let mut counting = spec(counter, &[]);
    counting.steps = Some(3);
    let Some(r) = renderer(&counting) else { return };
    let target = offscreen(&r.0.device);
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    let seen: Vec<u8> = (0..2).map(|_| render_red(&r, &target)).collect();
    for (got, want) in seen.iter().zip([77i32, 153]) {
      assert!(
        (i32::from(*got) - want).abs() <= 2,
        "0.3 then 0.6 of 255, got {seen:?}"
      );
    }
  }

  #[test]
  fn probe_reads_the_state_at_a_point() {
    let counter = "
      @fragment fn sim(in: VsOut) -> @location(0) vec4f {
        let c = vec2i(in.pos.xy);
        return vec4f(f32(c.x) / 10.0, f32(c.y) / 10.0, 0.5, 1.0);
      }
      @fragment fn fs(in: VsOut) -> @location(0) vec4f { return vec4f(0.0); }";
    let mut grid = spec(counter, &[]);
    grid.region = Region {
      x: 100.0,
      y: 200.0,
      w: 50.0,
      h: 50.0,
    };
    grid.cell = Some(10.0);
    let Some(r) = renderer(&grid) else { return };
    assert!(
      r.probe(110.0, 210.0).is_err(),
      "nothing to read before the first step"
    );
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    run(&r, 1);

    // Point (125, 235) is in cell (2, 3) of the region's 5x5 cells.
    let [x, y, blue, _] = r.probe(125.0, 235.0).unwrap();
    assert!(
      (x - 0.2).abs() < 0.01 && (y - 0.3).abs() < 0.01 && (blue - 0.5).abs() < 0.01,
      "{x} {y} {blue}"
    );
    // Points outside the region read the nearest cell.
    assert!((r.probe(0.0, 0.0).unwrap()[0] - 0.0).abs() < 0.01);
    assert!((r.probe(1000.0, 1000.0).unwrap()[0] - 0.4).abs() < 0.01);
  }

  #[test]
  fn probing_a_shader_without_state_is_an_error() {
    let Some(r) = renderer(&spec(PASSTHROUGH, &[])) else {
      return;
    };
    r.0.state.lock().latest = Some(offscreen(&r.0.device));
    run(&r, 1);
    assert!(r.probe(1.0, 1.0).unwrap_err().contains("no state"));
  }

  // --- x-ray ---

  /// A window of one colour with a stripe of another, where the warp can show it.
  fn window_pixels(
    device: &ProtocolObject<dyn MTLDevice>,
    stripe: std::ops::Range<usize>,
  ) -> Retained<ProtocolObject<dyn MTLTexture>> {
    picture(device, 200, 100, |x, _| {
      if stripe.contains(&x) {
        [40, 220, 40, 255]
      } else {
        [200, 40, 40, 255]
      }
    })
  }

  /// A hole of radius 20 in a window under the cursor, with a 12 point rim.
  fn xray(stripe: std::ops::Range<usize>) -> Option<Renderer> {
    let r = renderer(&demo_spec("xray"))?;
    {
      let mut st = r.0.state.lock();
      st.latest = Some(window_pixels(&r.0.device, stripe)); // blue window, as it is
      st.behind = Some(solid(&r.0.device, [0, 0, 255, 255])); // red desktop, without it
    }
    set(
      &r,
      &[
        ("windows", windows(&[[20.0, 10.0, 160.0, 80.0]])),
        ("radius", vec![20.0]),
        ("thickness", vec![12.0]),
        ("warp", vec![1.0]),
      ],
    );
    MOUSE.with(|m| m.set(Some([100.0, 50.0])));
    Some(r)
  }

  #[test]
  fn xray_punches_a_lit_rim_through_the_window_under_the_cursor() {
    let Some(r) = xray(0..0) else { return };
    let image = render_image(&r, 200, 100);
    let at = |x: usize, y: usize| image[y * 200 + x]; // BGRA, premultiplied

    // The hole shows what is behind the window, opaque; the rim is opaque too.
    assert_eq!(at(100, 50)[3], 255, "the hole is opaque");
    assert!(
      at(100, 50)[2] > 150 && at(100, 50)[0] < 60,
      "and shows the desktop (red): {:?}",
      at(100, 50)
    );
    assert_eq!(at(100, 26)[3], 255, "the rim is opaque");
    // Outside the outer circle, or outside the window, nothing is drawn.
    assert_eq!(at(140, 50), [0, 0, 0, 0]);
    assert_eq!(at(10, 50), [0, 0, 0, 0]);

    // The rim is the window's own colour (blue), darker on the top and left, lighter on the
    // bottom-right where it faces the light.
    let blue = |x: usize, y: usize| at(x, y)[0];
    assert!(
      blue(100, 26) < 170 && blue(74, 50) < 170,
      "top and left darken: {} {}",
      blue(100, 26),
      blue(74, 50)
    );
    assert!(
      blue(118, 68) > 200,
      "the bottom-right catches the light: {}",
      blue(118, 68)
    );
    assert!(
      at(100, 26)[2] < 100,
      "and it is the window's colour, not the desktop's"
    );

    // The profile: flat at the outer circle, then steeper and steeper toward the hole.
    let shade: Vec<u8> = [31, 28, 25, 22]
      .iter()
      .map(|&d| blue(100, 50 - d))
      .collect();
    assert!(shade[0] > 185, "nearly flat at the outer circle: {shade:?}");
    // Darker toward the hole, and steeper as the surface tips over: the last step is cut short
    // by the grazing highlight where the lip turns edge-on.
    assert!(
      shade.windows(2).all(|w| w[1] < w[0]),
      "darker toward the hole: {shade:?}"
    );
    let drops: Vec<i32> = shade[..3]
      .windows(2)
      .map(|w| i32::from(w[0]) - i32::from(w[1]))
      .collect();
    assert!(drops[1] > drops[0], "and getting steeper: {shade:?}");
  }

  #[test]
  fn xray_warps_the_window_texture_around_the_rim() {
    // A stripe at x 117..120, inside the hole's circle (which spans x 80..120 on this row).
    let Some(r) = xray(117..120) else { return };
    let image = render_image(&r, 200, 100);
    let at = |x: usize, y: usize| image[y * 200 + x];

    // Without the warp, (121, 50) would be plain window colour. At distance 21 from the middle
    // the rim shows the window from about 18 away, where the stripe is.
    let px = at(121, 50);
    assert!(px[1] > px[0], "the stripe is pulled onto the rim: {px:?}");
    // Near the outer circle there is hardly any warp.
    let px = at(131, 50);
    assert!(px[0] > px[1], "the outer edge is unwarped: {px:?}");
  }

  #[test]
  fn xray_thickness_sets_the_width_of_the_rim() {
    let Some(r) = xray(0..0) else { return };
    let rim_at = |r: &Renderer| render_image(r, 200, 100)[50 * 200 + 125][3];
    assert_eq!(
      rim_at(&r),
      255,
      "5 points from the hole is rim when it is 12 wide"
    );
    set(&r, &[("thickness", vec![3.0])]);
    assert_eq!(rim_at(&r), 0, "and nothing when it is 3 wide");
  }

  #[test]
  fn xray_does_nothing_when_the_cursor_is_not_over_a_window() {
    let Some(r) = xray(0..0) else { return };
    MOUSE.with(|m| m.set(Some([10.0, 50.0]))); // outside the window
    assert!(render_image(&r, 200, 100)
      .iter()
      .all(|px| *px == [0, 0, 0, 0]));
  }

  // --- blobs ---

  /// Blobs over a 200x100 point region, the way the demo runs them.
  fn blobs() -> Option<Renderer> {
    let r = renderer(&demo_spec("blobs"))?;
    r.0.state.lock().latest = Some(solid(&r.0.device, [0, 0, 255, 255]));
    set(
      &r,
      &[
        ("goo", vec![60.0]),
        ("size", vec![14.0]),
        ("count", vec![6.0]),
        ("speed", vec![1.0]),
        ("cling", vec![1.0]),
        ("tone", vec![0.1]),
        ("ball", vec![0.0]),
        ("wobble", vec![0.0]),
      ],
    );
    MOUSE.with(|m| m.set(Some([-500.0, -500.0]))); // far away
    Some(r)
  }

  /// Blob positions: texels `0..count` of the first row of the state.
  fn blob_positions(r: &Renderer, count: usize) -> Vec<[f32; 2]> {
    let (_, _, cells) = read_state(r);
    cells[..count].iter().map(|c| [c[0], c[1]]).collect()
  }

  #[test]
  fn blobs_start_scattered_and_wander_without_leaving() {
    let Some(r) = blobs() else { return };
    set(&r, &[("windows", windows(&[]))]);
    run(&r, 2);
    let start = blob_positions(&r, 6);
    assert!(
      start
        .iter()
        .all(|p| (0.0..200.0).contains(&p[0]) && (0.0..100.0).contains(&p[1])),
      "{start:?}"
    );
    assert!(
      start.windows(2).any(|w| (w[0][0] - w[1][0]).abs() > 5.0),
      "scattered, not stacked: {start:?}"
    );

    run(&r, 400);
    let later = blob_positions(&r, 6);
    assert!(
      later
        .iter()
        .all(|p| (-10.0..210.0).contains(&p[0]) && (-10.0..110.0).contains(&p[1])),
      "{later:?}"
    );
    assert!(
      start
        .iter()
        .zip(&later)
        .any(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() > 10.0),
      "they moved"
    );
  }

  #[test]
  fn blobs_are_kept_out_of_windows() {
    let Some(r) = blobs() else { return };
    let rect = [60.0, 20.0, 80.0, 60.0];
    set(&r, &[("windows", windows(&[rect]))]);
    run(&r, 600);
    for [x, y] in blob_positions(&r, 6) {
      let inside = x > rect[0] + 4.0
        && x < rect[0] + rect[2] - 4.0
        && y > rect[1] + 4.0
        && y < rect[1] + rect[3] - 4.0;
      assert!(!inside, "blob at ({x}, {y}) is inside the window");
    }
  }

  /// The blobs are plain monochrome: every drawn pixel is the same grey.
  #[test]
  fn blobs_are_monochrome() {
    let Some(r) = blobs() else { return };
    set(&r, &[("windows", windows(&[])), ("tone", vec![0.5])]);
    run(&r, 3);
    let image = render_image(&r, 200, 100);
    let drawn: Vec<_> = image.iter().filter(|px| px[3] == 255).collect();
    assert!(!drawn.is_empty(), "something is drawn");
    assert!(
      drawn.iter().all(|px| px[0] == px[1] && px[1] == px[2]),
      "grey only"
    );
    assert!(
      drawn.iter().all(|px| (i32::from(px[0]) - 128).abs() <= 1),
      "at the chosen tone"
    );
  }

  #[test]
  fn blobs_leave_windows_to_the_real_thing() {
    let Some(r) = blobs() else { return };
    let rect = [60.0, 20.0, 80.0, 60.0];
    set(&r, &[("windows", windows(&[rect]))]);
    run(&r, 50);
    let image = render_image(&r, 200, 100);
    assert_eq!(
      image[50 * 200 + 100],
      [0, 0, 0, 0],
      "the middle of a window is not drawn on"
    );
  }

  // --- looking at the demos (writes PNGs when ALLIO_SNAPSHOTS names a directory) ---

  /// Pixels per point in snapshots.
  const SCALE: usize = 4;

  /// A stand-in desktop: a gradient with a faint grid.
  fn desktop(x: f32, y: f32) -> [u8; 3] {
    let grid = if (x % 20.0) < 0.5 || (y % 20.0) < 0.5 {
      14.0
    } else {
      0.0
    };
    [
      (40.0 + x * 0.35 + grid) as u8,
      (70.0 + y * 0.7 + grid) as u8,
      (150.0 - x * 0.2 + grid) as u8,
    ]
  }

  /// A window that is just a grid of lines every 10 points, to show how a shader displaces it.
  fn grid_pixel(x: f32, y: f32) -> [u8; 3] {
    if (x % 10.0) < 0.8 || (y % 10.0) < 0.8 {
      [30, 30, 40]
    } else {
      [236, 236, 240]
    }
  }

  /// What is inside a window at `(x, y)` relative to its top-left: a title bar, lines of
  /// stand-in text, and a few coloured blocks.
  fn window_pixel(x: f32, y: f32, w: f32, h: f32) -> [u8; 3] {
    if y < 12.0 {
      return [226, 226, 230];
    }
    if x < 1.0 || y < 13.0 || x > w - 1.0 || y > h - 1.0 {
      return [170, 170, 176];
    }
    let block =
      |bx: f32, by: f32, bw: f32, bh: f32| x >= bx && x < bx + bw && y >= by && y < by + bh;
    if block(8.0, 60.0, 30.0, 14.0) {
      return [40, 110, 230]; // blue: water
    }
    if block(44.0, 60.0, 30.0, 14.0) {
      return [50, 170, 70]; // green: plant
    }
    if block(80.0, 60.0, 30.0, 14.0) {
      return [200, 60, 50]; // red
    }
    if block(116.0, 60.0, 30.0, 14.0) {
      return [30, 30, 36]; // near black: coal
    }
    let line = ((y - 20.0) / 6.0).floor();
    let along = (y - 20.0) % 6.0;
    if y < 54.0
      && along > 1.5
      && along < 4.0
      && x > 8.0
      && x < 20.0 + 120.0 * ((line * 0.37).sin().abs())
    {
      return [60, 60, 66]; // text
    }
    [248, 248, 250]
  }

  thread_local! {
    /// Whether snapshot windows show a grid instead of a UI.
    static GRID: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
  }

  /// The screen: the desktop with `windows` on it (frontmost first).
  fn scene(x: f32, y: f32, windows: &[[f32; 4]]) -> [u8; 3] {
    for r in windows {
      if x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3] {
        return if GRID.with(std::cell::Cell::get) {
          grid_pixel(x, y)
        } else {
          window_pixel(x - r[0], y - r[1], r[2], r[3])
        };
      }
    }
    desktop(x, y)
  }

  /// A texture of the scene, `SCALE` pixels per point.
  fn scene_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    windows: &[[f32; 4]],
  ) -> Retained<ProtocolObject<dyn MTLTexture>> {
    picture(device, 200 * SCALE, 100 * SCALE, |px, py| {
      let [r, g, b] = scene(px as f32 / SCALE as f32, py as f32 / SCALE as f32, windows);
      [b, g, r, 255]
    })
  }

  /// Saves `overlay` (premultiplied BGRA) drawn over the scene, if snapshots are enabled.
  fn snapshot(name: &str, overlay: &[[u8; 4]], windows: &[[f32; 4]]) {
    let Some(dir) = crate::snapshot::dir() else {
      return;
    };
    let (w, h) = (200 * SCALE, 100 * SCALE);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
      for x in 0..w {
        let o = overlay[y * w + x];
        let behind = scene(x as f32 / SCALE as f32, y as f32 / SCALE as f32, windows);
        let keep = 1.0 - f32::from(o[3]) / 255.0;
        let mix =
          |over: u8, under: u8| (f32::from(over) + f32::from(under) * keep).clamp(0.0, 255.0) as u8;
        rgba.extend([
          mix(o[2], behind[0]),
          mix(o[1], behind[1]),
          mix(o[0], behind[2]),
          255,
        ]);
      }
    }
    crate::snapshot::write_png(&dir.join(format!("{name}.png")), w, h, &rgba);
  }

  /// X-ray over the scene, 4 pixels per point.
  fn xray_snapshot(name: &str, radius: f32, thickness: f32, warp: f32) {
    let rect = [20.0, 10.0, 160.0, 80.0];
    let Some(r) = renderer(&demo_spec("xray")) else {
      return;
    };
    {
      let mut st = r.0.state.lock();
      st.latest = Some(scene_texture(&r.0.device, &[rect]));
      st.behind = Some(scene_texture(&r.0.device, &[]));
    }
    set(
      &r,
      &[
        ("windows", windows(&[rect])),
        ("radius", vec![radius]),
        ("thickness", vec![thickness]),
        ("warp", vec![warp]),
      ],
    );
    MOUSE.with(|m| m.set(Some([100.0, 50.0])));
    snapshot(name, &render_image(&r, 200 * SCALE, 100 * SCALE), &[rect]);
  }

  #[test]
  fn snapshots_xray() {
    for (label, warp) in [("flat", 0.0), ("squeeze", 1.0), ("stretch", -1.0)] {
      GRID.with(|g| g.set(true));
      xray_snapshot(&format!("xray-grid-{label}"), 30.0, 20.0, warp);
      xray_snapshot(&format!("xray-grid-thin-{label}"), 30.0, 7.0, warp);
      GRID.with(|g| g.set(false));
      xray_snapshot(&format!("xray-ui-{label}"), 30.0, 20.0, warp);
      xray_snapshot(&format!("xray-ui-thin-{label}"), 30.0, 7.0, warp);
    }
  }

  /// Lava poured onto windows made of paper, metal, water and coal, over the scene's colours.
  #[test]
  fn snapshots_lava() {
    let Some(dir) = crate::snapshot::dir() else {
      return;
    };
    let Some(r) = renderer(&demo_spec("lava")) else {
      return;
    };
    // (x, y, w, h, colour): paper, metal, water, coal side by side, taps above each.
    let boxes: [([f32; 4], [u8; 3]); 4] = [
      ([6.0, 40.0, 40.0, 40.0], [240, 238, 230]),
      ([54.0, 40.0, 40.0, 40.0], [150, 150, 158]),
      ([102.0, 40.0, 40.0, 40.0], [60, 110, 220]),
      ([150.0, 40.0, 40.0, 40.0], [30, 30, 34]),
    ];
    let rects = boxes.map(|b| b.0);
    let colour = |x: f32, y: f32| {
      boxes
        .iter()
        .find(|(b, _)| x >= b[0] && x < b[0] + b[2] && y >= b[1] && y < b[1] + b[3])
        .map_or([70, 120, 90], |(_, c)| *c)
    };
    let (w, h) = (200 * SCALE, 100 * SCALE);
    let texture = picture(&r.0.device, w, h, |px, py| {
      let [red, green, blue] = colour(px as f32 / SCALE as f32, py as f32 / SCALE as f32);
      [blue, green, red, 255]
    });
    {
      let mut state = r.0.state.lock();
      state.latest = Some(texture);
      state.behind = Some(solid(&r.0.device, [90, 120, 70, 255]));
    }
    set(
      &r,
      &[
        ("windows", windows(&rects)),
        ("radius", vec![5.0]),
        ("rate", vec![0.03]),
      ],
    );
    for (label, frames) in [("a", 100), ("b", 400)] {
      for x in [26.0, 74.0, 122.0, 170.0] {
        set(&r, &[("pour", vec![1.0])]);
        MOUSE.with(|m| m.set(Some([x, 25.0])));
        run(&r, frames / 4);
        set(&r, &[("pour", vec![0.0])]);
        run(&r, 1);
      }
      let overlay = render_image(&r, w, h);
      let mut rgba = Vec::with_capacity(w * h * 4);
      for y in 0..h {
        for x in 0..w {
          let o = overlay[y * w + x];
          let under = colour(x as f32 / SCALE as f32, y as f32 / SCALE as f32);
          let keep = 1.0 - f32::from(o[3]) / 255.0;
          let mix = |over: u8, below: u8| {
            (f32::from(over) + f32::from(below) * keep).clamp(0.0, 255.0) as u8
          };
          rgba.extend([
            mix(o[2], under[0]),
            mix(o[1], under[1]),
            mix(o[0], under[2]),
            255,
          ]);
        }
      }
      crate::snapshot::write_png(&dir.join(format!("lava-{label}.png")), w, h, &rgba);
    }
  }

  #[test]
  fn snapshots_blobs() {
    let Some(r) = blobs() else { return };
    let rects = [[8.0, 14.0, 78.0, 62.0], [118.0, 30.0, 74.0, 60.0]];
    r.0.state.lock().latest = Some(scene_texture(&r.0.device, &rects));
    set(
      &r,
      &[
        ("windows", windows(&rects)),
        ("goo", vec![22.0]),
        ("size", vec![9.0]),
        ("count", vec![8.0]),
        ("ball", vec![8.0]),
        ("wobble", vec![3.0]),
        ("tone", vec![0.08]),
      ],
    );
    MOUSE.with(|m| m.set(Some([100.0, 50.0])));
    for (label, frames) in [("a", 240), ("b", 200), ("c", 200)] {
      run(&r, frames);
      snapshot(
        &format!("blobs-{label}"),
        &render_image(&r, 200 * SCALE, 100 * SCALE),
        &rects,
      );
    }
  }
}
