#![deny(unused_imports)]

/// Window information
#[derive(Debug, Clone)]
pub struct WindowInfo {
  pub id: String,
  pub title: String,
  pub app_name: String,
  pub x: f64,
  pub y: f64,
  pub w: f64,
  pub h: f64,
  pub focused: bool,
  pub process_id: u32,
  /// Z-order index (0 = frontmost)
  pub z_index: u32,
}
