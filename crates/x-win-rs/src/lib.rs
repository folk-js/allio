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

/// Retrieve information the about currently active window.
/// Return `WindowInfo` containing details about a specific active window.
pub fn get_active_window() -> Result<WindowInfo> {
  let api = init_platform_api();
  let active_window = api.get_active_window()?;
  Ok(active_window)
}

/// Retrieve information about the currently open windows.
/// Return `Vec<WindowInfo>` each containing details about a specific open window.
pub fn get_open_windows() -> Result<Vec<WindowInfo>> {
  let api = init_platform_api();
  let open_windows = api.get_open_windows()?;
  Ok(open_windows)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_empty_entity() -> Result<()> {
    let window_info = empty_entity();
    assert_eq!(window_info.id, 0);
    assert_eq!(window_info.title, String::from(""));
    Ok(())
  }

  #[cfg(all(feature = "macos_permission", target_os = "macos"))]
  #[test]
  #[ignore = "Not working on ci/cd"]
  fn test_check_screen_record_permission() -> Result<()> {
    use macos::permission::check_screen_record_permission;
    let value = check_screen_record_permission();
    assert_eq!(value, true);
    Ok(())
  }

  #[cfg(all(feature = "macos_permission", target_os = "macos"))]
  #[test]
  #[ignore = "Not working on ci/cd"]
  fn test_request_screen_record_permission() -> Result<()> {
    use macos::permission::request_screen_record_permission;
    let value = request_screen_record_permission();
    assert_eq!(value, true);
    Ok(())
  }
}
