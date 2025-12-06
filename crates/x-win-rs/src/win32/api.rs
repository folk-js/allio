#![deny(unused_imports)]

use crate::common::{api::Api, result::Result, x_win_struct::window_info::WindowInfo};

pub struct WindowsAPI {}

impl Api for WindowsAPI {
  fn get_open_windows(&self) -> Result<Vec<WindowInfo>> {
    // TODO: Windows support not implemented
    Ok(vec![])
  }
}
