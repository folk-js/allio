//! Captures a screen region and redraws it through a WGSL fragment shader on the GPU.
//!
//! A [`Shader`] is described by a [`ShaderSpec`]. Spec types, uniform layout and WGSL to MSL
//! translation are portable. The rest is macOS: `ScreenCaptureKit` frames are `IOSurface`s,
//! wrapped as Metal textures without copying, and drawn to a click-through window on every
//! vsync. On other platforms `Shader::new` returns an error.

// Translation is portable, but only the macOS renderer uses it so far.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod pipeline;
mod spec;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod uniforms;

#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
mod diag;
#[cfg(target_os = "macos")]
mod light;
#[cfg(target_os = "macos")]
mod render;
#[cfg(target_os = "macos")]
mod shader;
#[cfg(all(test, target_os = "macos"))]
mod snapshot;
#[cfg(target_os = "macos")]
mod ticker;
#[cfg(not(target_os = "macos"))]
mod unsupported;

pub use spec::{Hide, Region, ShaderSpec, Source, MAX_SOURCES};
pub use uniforms::UniformType;

#[cfg(target_os = "macos")]
pub use shader::Shader;
#[cfg(not(target_os = "macos"))]
pub use unsupported::Shader;
