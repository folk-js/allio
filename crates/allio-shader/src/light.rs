#![allow(unsafe_code)]

//! Global illumination for shaders that define `scene`: holographic radiance cascades, solved
//! on the grid of cells `scene` wrote, into the `light` texture. The kernels are in
//! `light.wgsl`, translated to Metal by naga like every other shader here.

#![allow(clippy::cast_possible_truncation)] // grid sizes and levels fit in u32

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString};
use objc2_metal::{
  MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder,
  MTLComputeCommandEncoder, MTLComputePipelineState, MTLDevice, MTLLibrary, MTLLoadAction,
  MTLPixelFormat, MTLRenderPassDescriptor, MTLResourceOptions, MTLSize, MTLStoreAction,
  MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use std::ptr::NonNull;

const SOURCE: &str = include_str!("light.wgsl");

/// Format of the cell textures: `emit`, `matter`, bounce and `light`.
pub(crate) const FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA16Float;
/// Threads per threadgroup side; the kernels declare the same.
const GROUP: usize = 16;
/// Buffer slot naga's runtime-array sizes go in (unused: the kernels do their own bounds checks).
const SIZES_SLOT: u8 = 30;

type Pso = Retained<ProtocolObject<dyn MTLComputePipelineState>>;
type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;

/// The compiled kernels. Built once per renderer.
pub(crate) struct Kernels {
  seed: Pso,
  extend: Pso,
  merge: Pso,
  blur: Pso,
  bounce: Pso,
}

/// Every binding is its own Metal slot, in whichever namespace (buffer or texture) it needs.
fn resources(module: &naga::Module) -> naga::back::msl::EntryPointResources {
  use naga::back::msl::{BindTarget, EntryPointResources};
  let mut resources = EntryPointResources {
    sizes_buffer: Some(SIZES_SLOT),
    ..Default::default()
  };
  for (_, global) in module.global_variables.iter() {
    let Some(binding) = global.binding else {
      continue;
    };
    let slot = binding.binding as u8;
    let texture = matches!(module.types[global.ty].inner, naga::TypeInner::Image { .. });
    let target = if texture {
      BindTarget {
        texture: Some(slot),
        ..Default::default()
      }
    } else {
      BindTarget {
        buffer: Some(slot),
        mutable: true,
        ..Default::default()
      }
    };
    resources.resources.insert(binding, target);
  }
  resources
}

impl Kernels {
  pub(crate) fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self, String> {
    let module = naga::front::wgsl::parse_str(SOURCE).map_err(|e| e.emit_to_string(SOURCE))?;
    let info = naga::valid::Validator::new(
      naga::valid::ValidationFlags::all(),
      // pack2x16float, for rays stored as half floats; Metal has it.
      naga::valid::Capabilities::SHADER_FLOAT16_IN_FLOAT32,
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(SOURCE))?;
    let mut options = naga::back::msl::Options {
      lang_version: (2, 4),
      ..Default::default()
    };
    for ep in &module.entry_points {
      options
        .per_entry_point_map
        .insert(ep.name.clone(), resources(&module));
    }
    let (msl, translation) = naga::back::msl::write_string(
      &module,
      &info,
      &options,
      &naga::back::msl::PipelineOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    let library = device
      .newLibraryWithSource_options_error(&NSString::from_str(&msl), None)
      .map_err(|e| format!("Metal rejected the light kernels: {}", e.localizedDescription()))?;
    // naga may rename entry points that clash with Metal's reserved words.
    let kernel = |name: &str| -> Result<Pso, String> {
      let index = module
        .entry_points
        .iter()
        .position(|ep| ep.name == name)
        .ok_or_else(|| format!("no kernel {name}"))?;
      let msl_name = translation
        .entry_point_names
        .get(index)
        .and_then(|n| n.as_ref().ok())
        .ok_or_else(|| format!("kernel {name} didn't translate"))?;
      let function = library
        .newFunctionWithName(&NSString::from_str(msl_name))
        .ok_or_else(|| format!("no Metal function for {name}"))?;
      device
        .newComputePipelineStateWithFunction_error(&function)
        .map_err(|e| e.localizedDescription().to_string())
    };
    Ok(Self {
      seed: kernel("seed")?,
      extend: kernel("extend")?,
      merge: kernel("merge")?,
      blur: kernel("blur")?,
      bounce: kernel("bounce")?,
    })
  }
}

const fn ceil_log2(n: usize) -> u32 {
  n.next_power_of_two().trailing_zeros()
}

type Encoder = ProtocolObject<dyn MTLComputeCommandEncoder>;

/// Runs `pso` over `threads` (x, y), with `params` (a uniform struct of 32-bit fields) at
/// buffer `slot`.
fn dispatch(enc: &Encoder, pso: &Pso, threads: (usize, usize), params: &[u32], slot: usize) {
  enc.setComputePipelineState(pso);
  if !params.is_empty() {
    // Uniform structs are padded to 16 bytes.
    let mut padded = params.to_vec();
    padded.resize(params.len().div_ceil(4) * 4, 0);
    unsafe {
      enc.setBytes_length_atIndex(
        NonNull::from(padded.as_slice()).cast(),
        padded.len() * 4,
        slot,
      );
    }
  }
  enc.dispatchThreadgroups_threadsPerThreadgroup(
    MTLSize {
      width: threads.0.div_ceil(GROUP),
      height: threads.1.div_ceil(GROUP),
      depth: 1,
    },
    MTLSize {
      width: GROUP,
      height: GROUP,
      depth: 1,
    },
  );
}

/// The lighting of one grid of cells: what `scene` wrote, and the buffers the cascades use.
pub(crate) struct Light {
  /// Size in cells.
  pub(crate) size: (usize, usize),
  /// Written by `scene`: light given off (rgb).
  pub(crate) emit: Texture,
  /// Written by `scene`: albedo (rgb) and opacity per point (a).
  pub(crate) matter: Texture,
  /// The result, read as `light`: mean radiance arriving at each cell.
  pub(crate) texture: Texture,
  /// What last frame's light makes each cell give off.
  bounce: Texture,
  /// Rays of each level, as packed half floats.
  rays: Vec<Buffer>,
  /// Merge results, ping-ponged from the top level down.
  merges: [Buffer; 2],
  /// Fluence of all four directions, added up (RGB9E5).
  fluence: Buffer,
  /// Zeros for naga's buffer-sizes argument.
  sizes: Buffer,
}

impl Light {
  /// Allocates everything for a grid of `size` cells, with no light yet.
  pub(crate) fn new(
    device: &ProtocolObject<dyn MTLDevice>,
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    size: (usize, usize),
  ) -> Option<Self> {
    let texture = |usage: MTLTextureUsage| {
      let d = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
          FORMAT, size.0, size.1, false,
        )
      };
      d.setUsage(usage | MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
      d.setStorageMode(MTLStorageMode::Private);
      device.newTextureWithDescriptor(&d)
    };
    let buffer = |bytes: usize| {
      device.newBufferWithLength_options(bytes.max(16), MTLResourceOptions::StorageModePrivate)
    };
    let (w, h) = size;
    let longest = w.max(h);
    let levels = ceil_log2(longest) as usize;
    let rays = (0..levels.max(1))
      .map(|i| {
        let width = if i == 0 {
          longest
        } else {
          longest.div_ceil(1 << i) * ((1 << i) + 1)
        };
        buffer(width * longest * 8)
      })
      .collect::<Option<Vec<_>>>()?;
    let merge_bytes = longest.next_power_of_two() * longest * 4;
    let light = Self {
      size,
      emit: texture(MTLTextureUsage::empty())?,
      matter: texture(MTLTextureUsage::empty())?,
      texture: texture(MTLTextureUsage::ShaderWrite)?,
      bounce: texture(MTLTextureUsage::ShaderWrite)?,
      rays,
      merges: [buffer(merge_bytes)?, buffer(merge_bytes)?],
      fluence: buffer(w * h * 4)?,
      sizes: device.newBufferWithLength_options(256, MTLResourceOptions::StorageModeShared)?,
    };
    // Private memory starts as garbage: no light and no bounce yet.
    for t in [&light.texture, &light.bounce] {
      let pass = MTLRenderPassDescriptor::renderPassDescriptor();
      let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
      color.setTexture(Some(t));
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
    Some(light)
  }

  /// Solves the light for what `scene` just wrote into `emit` and `matter`. `cell` is the size
  /// of a cell in points, so opacity is per point whatever the grid.
  pub(crate) fn encode(
    &self,
    kernels: &Kernels,
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    cell: f32,
  ) -> Option<()> {
    let (w, h) = self.size;
    let blit = cb.blitCommandEncoder()?;
    blit.fillBuffer_range_value(&self.fluence, NSRange::new(0, self.fluence.length()), 0);
    blit.endEncoding();

    // Dispatches in one (serial) compute encoder run in order, each seeing the last's writes.
    let enc = cb.computeCommandEncoder()?;
    unsafe {
      enc.setTexture_atIndex(Some(&self.emit), 0);
      enc.setTexture_atIndex(Some(&self.matter), 1);
      enc.setBuffer_offset_atIndex(Some(&self.sizes), 0, SIZES_SLOT.into());
      // Bounce from last frame's light (zero on the first).
      enc.setTexture_atIndex(Some(&self.texture), 16);
      enc.setTexture_atIndex(Some(&self.bounce), 17);
    }
    dispatch(&enc, &kernels.bounce, (w, h), &[], 0);
    unsafe { enc.setTexture_atIndex(Some(&self.bounce), 2) };

    for dir in 0..4 {
      self.cascade(&enc, kernels, dir, cell)?;
    }

    unsafe {
      enc.setBuffer_offset_atIndex(Some(&self.fluence), 0, 13);
      enc.setTexture_atIndex(Some(&self.texture), 14);
    }
    dispatch(&enc, &kernels.blur, (w, h), &[w as u32, h as u32], 15);
    enc.endEncoding();
    Some(())
  }

  /// Adds the fluence of light travelling in direction `dir` (east, north, west, south) into
  /// `fluence`: seed, extend, merge.
  fn cascade(&self, enc: &Encoder, kernels: &Kernels, dir: u32, cell: f32) -> Option<()> {
    let (w, h) = self.size;
    // Probes run along the direction; slices across it.
    let (pc, sc) = if dir & 1 == 1 { (h, w) } else { (w, h) };
    let levels = ceil_log2(pc).max(1);
    let (pc32, sc32) = (pc as u32, sc as u32);
    let ray = |level: u32| self.rays.get(level as usize);

    unsafe { enc.setBuffer_offset_atIndex(Some(ray(0)?), 0, 3) };
    dispatch(
      enc,
      &kernels.seed,
      (pc, sc),
      &[pc32, sc32, w as u32, h as u32, dir, cell.to_bits()],
      4,
    );

    let ray_width = |level: u32| pc.div_ceil(1 << level) * ((1 << level) + 1);
    for level in 1..levels {
      unsafe {
        enc.setBuffer_offset_atIndex(Some(ray(level - 1)?), 0, 5);
        enc.setBuffer_offset_atIndex(Some(ray(level)?), 0, 6);
      }
      let prev = if level == 1 { pc } else { ray_width(level - 1) };
      dispatch(
        enc,
        &kernels.extend,
        (ray_width(level), sc),
        &[pc32, level, prev as u32, ray_width(level) as u32, sc32],
        7,
      );
    }

    unsafe { enc.setBuffer_offset_atIndex(Some(&self.fluence), 0, 12) };
    let stride = pc.next_power_of_two() as u32;
    let [mut read, mut write] = [&self.merges[1], &self.merges[0]];
    for level in (0..levels).rev() {
      let level_probes = pc.div_ceil(1 << level);
      let top = level == levels - 1;
      let merge_in = if top {
        0
      } else {
        pc.div_ceil(1 << (level + 1)) * (1 << (level + 1))
      };
      unsafe {
        enc.setBuffer_offset_atIndex(Some(ray(level)?), 0, 8);
        enc.setBuffer_offset_atIndex(Some(read), 0, 9);
        enc.setBuffer_offset_atIndex(Some(write), 0, 10);
      }
      dispatch(
        enc,
        &kernels.merge,
        (level_probes << level, sc),
        &[
          pc32,
          level_probes as u32,
          (1 << level) + 1,
          u32::from(top),
          w as u32,
          h as u32,
          level,
          sc32,
          stride,
          merge_in as u32,
          dir,
        ],
        11,
      );
      std::mem::swap(&mut read, &mut write);
    }
    Some(())
  }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
  #[test]
  fn kernels_build() {
    let Some(device) = objc2_metal::MTLCreateSystemDefaultDevice() else {
      return;
    };
    if let Err(e) = super::Kernels::new(&device) {
      panic!("{e}");
    }
  }
}
