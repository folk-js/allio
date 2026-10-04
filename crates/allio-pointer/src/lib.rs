//! Pointer fields: decide how real mouse motion moves the pointer, and where it acts.
//!
//! A [`PointerSpec`] says how: the hand moves a visual pointer (scaled by a gain and sticky
//! [`Target`]s), and the real cursor goes where the screen drawn under it really is (through
//! [`Cut`]s and [`Lens`]es), so shader-drawn worlds agree with the pointer. The field and the
//! tracker are portable and pure. On macOS a session event tap applies them to every real mouse
//! move. On other platforms `Pointer::new` returns an error.

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod field;
mod spec;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod tracker;

#[cfg(target_os = "macos")]
mod shape;
#[cfg(target_os = "macos")]
mod tap;
#[cfg(not(target_os = "macos"))]
mod unsupported;

pub use spec::{Cut, Lens, PointerSpec, PointerState, Rect, Target, LENS_FLAT};

#[cfg(target_os = "macos")]
pub use shape::Shape;
#[cfg(target_os = "macos")]
pub use tap::Pointer;
#[cfg(not(target_os = "macos"))]
pub use unsupported::{Pointer, Shape};
