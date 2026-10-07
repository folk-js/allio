//! `CGVirtualDisplay` (private `CoreGraphics` classes), set up the way Chromium's test utility and
//! go-macos/virtualdisplay do.

#![allow(unsafe_code)]

use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchQueueGlobalPriority, GlobalQueueIdentifier};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_core_foundation::{CGPoint, CGSize};
use objc2_core_graphics::{
  kCGNullDirectDisplay, CGBeginDisplayConfiguration, CGCompleteDisplayConfiguration,
  CGConfigureDisplayMirrorOfDisplay, CGConfigureDisplayOrigin, CGConfigureOption,
  CGDirectDisplayID, CGDisplayBounds, CGDisplayConfigRef, CGDisplayIsInMirrorSet,
  CGDisplayIsMain, CGDisplayMirrorsDisplay, CGGetActiveDisplayList,
};
use objc2_foundation::{NSArray, NSString};

use crate::Frame;

/// Fixed identity, so macOS remembers the display (and its arrangement) between runs. A display
/// that comes back with a different size gets a new serial, as the remembered mode would be wrong.
const VENDOR: u32 = 0xA110;
const PRODUCT: u32 = 0x0001;
/// Pixels per inch reported for it (only used to give it a physical size).
const PPI: f64 = 110.0;
/// How long to wait for the window server to list a new display, or to settle its arrangement.
const APPEAR: Duration = Duration::from_secs(3);

/// A virtual display, removed on drop.
pub struct Backstage {
  /// Keeps the display alive; releasing it removes the display.
  _display: Retained<AnyObject>,
  id: CGDirectDisplayID,
}

// The display object is only used to keep the display alive and to read its id.
unsafe impl Send for Backstage {}
unsafe impl Sync for Backstage {}

impl std::fmt::Debug for Backstage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Backstage").field("id", &self.id).finish_non_exhaustive()
  }
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
  AnyClass::get(name).ok_or_else(|| {
    format!(
      "{} is missing: this macOS has no virtual displays",
      name.to_string_lossy()
    )
  })
}

impl Backstage {
  /// Creates a Retina (2x) display of `w` x `h` points, to the right of the real displays, and waits
  /// until the window server lists it.
  pub fn new(w: u32, h: u32) -> Result<Self, String> {
    let descriptor_class = class(c"CGVirtualDisplayDescriptor")?;
    let display_class = class(c"CGVirtualDisplay")?;
    let settings_class = class(c"CGVirtualDisplaySettings")?;
    let mode_class = class(c"CGVirtualDisplayMode")?;
    let (pw, ph) = (w * 2, h * 2);

    let display = unsafe {
      let descriptor: Retained<AnyObject> = msg_send![descriptor_class, new];
      let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
        DispatchQueueGlobalPriority::High,
      ));
      let queue: &AnyObject = &*std::ptr::from_ref::<DispatchQueue>(&queue).cast::<AnyObject>();
      let _: () = msg_send![&*descriptor, setQueue: queue];
      let name = NSString::from_str("Allio backstage");
      let _: () = msg_send![&*descriptor, setName: &*name];
      let _: () = msg_send![&*descriptor, setWhitePoint: CGPoint::new(0.3125, 0.3291)];
      let _: () = msg_send![&*descriptor, setBluePrimary: CGPoint::new(0.1494, 0.0557)];
      let _: () = msg_send![&*descriptor, setGreenPrimary: CGPoint::new(0.2559, 0.6983)];
      let _: () = msg_send![&*descriptor, setRedPrimary: CGPoint::new(0.6797, 0.3203)];
      let _: () = msg_send![&*descriptor, setMaxPixelsWide: pw];
      let _: () = msg_send![&*descriptor, setMaxPixelsHigh: ph];
      let mm = CGSize::new(25.4 * f64::from(pw) / PPI, 25.4 * f64::from(ph) / PPI);
      let _: () = msg_send![&*descriptor, setSizeInMillimeters: mm];
      let _: () = msg_send![&*descriptor, setVendorID: VENDOR];
      let _: () = msg_send![&*descriptor, setProductID: PRODUCT];
      let _: () = msg_send![&*descriptor, setSerialNum: pw ^ (ph << 16)];

      let allocated: Allocated<AnyObject> = msg_send![display_class, alloc];
      let display: Option<Retained<AnyObject>> =
        msg_send![allocated, initWithDescriptor: &*descriptor];
      let display = display.ok_or("the window server refused the virtual display")?;

      let settings: Retained<AnyObject> = msg_send![settings_class, new];
      let _: () = msg_send![&*settings, setHiDPI: 1u32];
      let allocated: Allocated<AnyObject> = msg_send![mode_class, alloc];
      let mode: Option<Retained<AnyObject>> =
        msg_send![allocated, initWithWidth: w, height: h, refreshRate: 60.0f64];
      let mode = mode.ok_or("couldn't make a display mode")?;
      let modes = NSArray::<AnyObject>::from_retained_slice(&[mode]);
      let _: () = msg_send![&*settings, setModes: &*modes];
      let applied: bool = msg_send![&*display, applySettings: &*settings];
      if !applied {
        return Err("the virtual display rejected its settings".into());
      }
      display
    };
    let id: CGDirectDisplayID = unsafe { msg_send![&*display, displayID] };
    if id == 0 {
      return Err("the virtual display has no id".into());
    }
    let backstage = Self {
      _display: display,
      id,
    };

    // Wait for it to appear, then put it to the right of everything else, top-aligned.
    let started = Instant::now();
    while !active_displays().contains(&id) {
      if started.elapsed() > APPEAR {
        return Err("the virtual display never appeared".into());
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    let right = active_displays()
      .into_iter()
      .filter(|d| *d != id)
      .map(|d| {
        let b = CGDisplayBounds(d);
        b.origin.x + b.size.width
      })
      .fold(0.0, f64::max);
    backstage.place(right, 0.0);

    // macOS may decide to mirror a new display (a real screen then shows the backstage) or make
    // it the main display. Either takes over a screen someone is looking at: if the arrangement
    // above didn't stick, remove it again (dropping it does) rather than leave it like that.
    let started = Instant::now();
    while backstage.intrudes() {
      if started.elapsed() > APPEAR {
        return Err(
          "macOS mirrored the backstage display onto a real one (or made it the main display), so it was removed again"
            .into(),
        );
      }
      std::thread::sleep(Duration::from_millis(50));
    }
    Ok(backstage)
  }

  /// Whether the display shows up where someone would see it: mirrored with a real display, or
  /// as the main display.
  fn intrudes(&self) -> bool {
    CGDisplayIsInMirrorSet(self.id) || CGDisplayIsMain(self.id)
  }

  /// Where the display is, in global screen points.
  pub fn frame(&self) -> Frame {
    let b = CGDisplayBounds(self.id);
    Frame {
      x: b.origin.x,
      y: b.origin.y,
      w: b.size.width,
      h: b.size.height,
    }
  }

  /// The display's id, as `CGDirectDisplayID` (and `ScreenCaptureKit`) know it.
  pub const fn id(&self) -> u32 {
    self.id
  }

  /// Puts the display in the arrangement (for this session only) as its own display: at (x, y),
  /// and in no mirror set, whichever way round macOS mirrored it.
  #[allow(clippy::cast_possible_truncation)]
  fn place(&self, x: f64, y: f64) {
    unsafe {
      let mut config: CGDisplayConfigRef = std::ptr::null_mut();
      if CGBeginDisplayConfiguration(&raw mut config).0 != 0 {
        return log::warn!("couldn't arrange the backstage display");
      }
      for display in active_displays() {
        if display == self.id || CGDisplayMirrorsDisplay(display) == self.id {
          let _ = CGConfigureDisplayMirrorOfDisplay(config, display, kCGNullDirectDisplay);
        }
      }
      let _ = CGConfigureDisplayOrigin(config, self.id, x.round() as i32, y.round() as i32);
      let _ = CGCompleteDisplayConfiguration(config, CGConfigureOption::ForSession);
    }
  }
}

impl Drop for Backstage {
  fn drop(&mut self) {
    // Releasing `_display` (next) removes it; the window server moves its windows back.
    log::info!("removing backstage display {}", self.id);
  }
}

fn active_displays() -> Vec<CGDirectDisplayID> {
  let mut ids: [CGDirectDisplayID; 16] = [0; 16];
  let mut count = 0u32;
  let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &raw mut count) };
  if err.0 != 0 {
    return Vec::new();
  }
  ids.into_iter().take(count as usize).collect()
}
