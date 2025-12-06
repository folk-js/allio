#![deny(unused_imports)]

mod api;

use crate::common::api::Api;
use api::LinuxAPI;

pub fn init_platform_api() -> impl Api {
  LinuxAPI {}
}
