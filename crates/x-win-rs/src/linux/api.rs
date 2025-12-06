#![deny(unused_imports)]

use crate::common::{api::Api, result::Result, x_win_struct::window_info::WindowInfo};

pub struct LinuxAPI {}

impl Api for LinuxAPI {
  fn get_open_windows(&self) -> Result<Vec<WindowInfo>> {
    // TODO: Linux support not implemented
    Ok(vec![])
  }
}
