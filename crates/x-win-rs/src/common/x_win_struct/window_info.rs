#![deny(unused_imports)]

/**
 * Struct to store all informations of the window
 */
#[derive(Debug, Clone)]
pub struct WindowInfo {
  pub id: u32,
  pub x: i32,
  pub y: i32,
  pub width: i32,
  pub height: i32,
  pub title: String,
  pub process_id: u32,
  pub process_name: String,
}

impl WindowInfo {
  pub fn new(
    id: u32,
    title: String,
    process_id: u32,
    process_name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
  ) -> Self {
    Self {
      id,
      title,
      x,
      y,
      width,
      height,
      process_id,
      process_name,
    }
  }
}
