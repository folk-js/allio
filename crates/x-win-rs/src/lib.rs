#![deny(unsafe_op_in_unsafe_fn)]

mod common;

#[cfg(target_os = "windows")]
mod win32;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
use win32::init_platform_api;

#[cfg(target_os = "linux")]
use linux::init_platform_api;

#[cfg(target_os = "macos")]
use macos::init_platform_api;

#[cfg(all(feature = "macos_permission", target_os = "macos"))]
pub use macos::permission;

pub use common::{
  api::{empty_entity, Api},
  result::Result,
  x_win_struct::window_info::WindowInfo,
};

/// Retrieve information about open windows.
/// Each `WindowInfo` includes a `focused` field indicating if it's the active window.
pub fn get_open_windows() -> Result<Vec<WindowInfo>> {
  let api = init_platform_api();
  api.get_open_windows()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_empty_entity() -> Result<()> {
    let w = empty_entity();
    assert!(w.id.is_empty());
    assert!(w.title.is_empty());
    assert!(w.app_name.is_empty());
    assert_eq!(w.x, 0.0);
    assert_eq!(w.y, 0.0);
    assert_eq!(w.w, 0.0);
    assert_eq!(w.h, 0.0);
    assert!(!w.focused);
    assert_eq!(w.process_id, 0);
    assert_eq!(w.z_index, 0);
    Ok(())
  }
}
