/*! Display-synchronized callbacks using CVDisplayLink. */

#![allow(unsafe_code)]
#![allow(clippy::unwrap_used)] // NonNull::new on stack pointers - never null

use objc2_core_video::{kCVReturnSuccess, CVDisplayLink, CVOptionFlags, CVReturn, CVTimeStamp};
use std::ffi::c_void;
use std::ptr::NonNull;

/// Handle to a running display link. Stops on drop.
pub(crate) struct DisplayLinkHandle {
  link: NonNull<CVDisplayLink>,
  _callback: Box<Box<dyn Fn() + Send + Sync>>,
}

unsafe impl Send for DisplayLinkHandle {}
unsafe impl Sync for DisplayLinkHandle {}

impl DisplayLinkHandle {
  #[allow(deprecated)]
  pub(crate) fn stop(&self) {
    unsafe {
      self.link.as_ref().stop();
    }
  }
}

impl Drop for DisplayLinkHandle {
  #[allow(deprecated)]
  fn drop(&mut self) {
    unsafe {
      self.link.as_ref().stop();
    }
  }
}

unsafe extern "C-unwind" fn display_link_callback(
  _display_link: NonNull<CVDisplayLink>,
  _in_now: NonNull<CVTimeStamp>,
  _in_output_time: NonNull<CVTimeStamp>,
  _flags_in: CVOptionFlags,
  _flags_out: NonNull<CVOptionFlags>,
  user_data: *mut c_void,
) -> CVReturn {
  if !user_data.is_null() {
    let callback = &*(user_data as *const Box<dyn Fn() + Send + Sync>);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      callback();
    }));

    if result.is_err() {
      log::error!("DisplayLink callback panicked");
    }
  }
  kCVReturnSuccess
}

/// Start a display link that calls the callback on each vsync.
#[allow(deprecated)]
pub(crate) fn start_display_link<F>(callback: F) -> Result<DisplayLinkHandle, &'static str>
where
  F: Fn() + Send + Sync + 'static,
{
  let mut link: *mut CVDisplayLink = std::ptr::null_mut();
  let result =
    unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::new(&raw mut link).unwrap()) };

  if result != kCVReturnSuccess || link.is_null() {
    return Err("Failed to create CVDisplayLink");
  }

  let link = unsafe { NonNull::new_unchecked(link) };

  let callback: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(callback));
  let callback_ptr = &raw const *callback as *mut c_void;

  let result = unsafe {
    link
      .as_ref()
      .set_output_callback(Some(display_link_callback), callback_ptr)
  };

  if result != kCVReturnSuccess {
    return Err("Failed to set CVDisplayLink callback");
  }

  let result = unsafe { link.as_ref().start() };
  if result != kCVReturnSuccess {
    return Err("Failed to start CVDisplayLink");
  }

  log::debug!("DisplayLink started successfully");

  Ok(DisplayLinkHandle {
    link,
    _callback: callback,
  })
}
