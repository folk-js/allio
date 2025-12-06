#![deny(unused_imports)]

use super::result::Result;
use super::x_win_struct::window_info::WindowInfo;

pub trait Api {
  /**
   * Return information of current active Window
   */
  fn get_active_window(&self) -> Result<WindowInfo>;

  /**
   * Return Array of open windows information
   */
  fn get_open_windows(&self) -> Result<Vec<WindowInfo>>;
}

pub fn empty_entity() -> WindowInfo {
  WindowInfo {
    id: 0,
    title: String::from(""),
    process_id: 0,
    process_name: String::from(""),
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  }
}
