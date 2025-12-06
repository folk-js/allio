#![deny(unused_imports)]

use super::result::Result;
use super::x_win_struct::window_info::WindowInfo;

pub trait Api {
  /// Return list of open windows with focus state
  fn get_open_windows(&self) -> Result<Vec<WindowInfo>>;
}

pub fn empty_entity() -> WindowInfo {
  WindowInfo {
    id: String::new(),
    title: String::new(),
    app_name: String::new(),
    x: 0.0,
    y: 0.0,
    w: 0.0,
    h: 0.0,
    focused: false,
    process_id: 0,
    z_index: 0,
  }
}
