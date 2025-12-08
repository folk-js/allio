//! Display-synchronized callbacks using CVDisplayLink.
//!
//! CVDisplayLink fires a callback synchronized to the display's actual refresh rate.
//! This ensures:
//! - No drift (tied to hardware vsync)
//! - Auto-adapts to display rate (60Hz, 120Hz, throttled, etc.)
//! - Perfect frame alignment

use objc2_core_video::{kCVReturnSuccess, CVDisplayLink, CVOptionFlags, CVReturn, CVTimeStamp};
use std::ffi::c_void;
use std::ptr::NonNull;

/// Handle to a running display link.
/// Stops the link when dropped.
pub struct DisplayLinkHandle {
  link: NonNull<CVDisplayLink>,
  // Keep the callback alive - double-boxed for stable pointer
  _callback: Box<Box<dyn Fn() + Send + Sync>>,
}

// SAFETY: CVDisplayLink is thread-safe according to Apple docs
unsafe impl Send for DisplayLinkHandle {}
unsafe impl Sync for DisplayLinkHandle {}

impl DisplayLinkHandle {
  /// Stop the display link (it will also stop on drop).
  #[allow(deprecated)] // CVDisplayLink deprecated in macOS 15, but still works
  pub fn stop(&self) {
    unsafe {
      self.link.as_ref().stop();
    }
  }

  /// Check if the display link is running.
  #[allow(deprecated)]
  pub fn is_running(&self) -> bool {
    unsafe { self.link.as_ref().is_running() }
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

/// The actual callback that dispatches to our Rust closure.
/// Called on a high-priority display link thread.
unsafe extern "C-unwind" fn display_link_callback(
  _display_link: NonNull<CVDisplayLink>,
  _in_now: NonNull<CVTimeStamp>,
  _in_output_time: NonNull<CVTimeStamp>,
  _flags_in: CVOptionFlags,
  _flags_out: NonNull<CVOptionFlags>,
  user_data: *mut c_void,
) -> CVReturn {
  if !user_data.is_null() {
    // SAFETY: We control what we put in user_data (a Box<Box<dyn Fn()>>)
    let callback = &*(user_data as *const Box<dyn Fn() + Send + Sync>);

    // Catch panics to avoid unwinding across FFI boundary
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      callback();
    }));

    if result.is_err() {
      log::error!("DisplayLink callback panicked");
    }
  }
  kCVReturnSuccess
}

/// Start a display link that calls the given callback on each vsync.
///
/// The callback is called on a high-priority display link thread.
/// Keep work minimal and thread-safe.
///
/// # Example
///
/// ```ignore
/// let handle = start_display_link(|| {
///     println!("Vsync!");
/// })?;
///
/// // Link runs until handle is dropped
/// std::thread::sleep(std::time::Duration::from_secs(1));
/// // handle.stop(); // or just let it drop
/// ```
#[allow(deprecated)] // CVDisplayLink deprecated in macOS 15, but still works
pub fn start_display_link<F>(callback: F) -> Result<DisplayLinkHandle, &'static str>
where
  F: Fn() + Send + Sync + 'static,
{
  // Create display link for all active displays
  let mut link: *mut CVDisplayLink = std::ptr::null_mut();
  let result =
    unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::new(&mut link).unwrap()) };

  if result != kCVReturnSuccess || link.is_null() {
    return Err("Failed to create CVDisplayLink");
  }

  let link = unsafe { NonNull::new_unchecked(link) };

  // Double-box the callback to get a stable pointer
  // The outer Box is for the trait object, the inner keeps it on heap with stable address
  let callback: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(callback));
  let callback_ptr = &*callback as *const _ as *mut c_void;

  // Set callback
  let result = unsafe {
    link
      .as_ref()
      .set_output_callback(Some(display_link_callback), callback_ptr)
  };

  if result != kCVReturnSuccess {
    return Err("Failed to set CVDisplayLink callback");
  }

  // Start the link
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
