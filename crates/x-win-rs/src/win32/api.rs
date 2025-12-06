#![deny(unused_imports)]

use crate::common::{
  api::{empty_entity, Api},
  result::Result,
  x_win_struct::window_info::WindowInfo,
};

pub struct WindowsAPI {}

impl Api for WindowsAPI {
  fn get_active_window(&self) -> Result<WindowInfo> {
    // TODO: Windows support not implemented
    Ok(empty_entity())
  }

  fn get_open_windows(&self) -> Result<Vec<WindowInfo>> {
    // TODO: Windows support not implemented
    Ok(vec![])
  }
}
