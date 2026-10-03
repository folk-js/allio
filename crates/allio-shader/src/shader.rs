#![allow(unsafe_code)]
#![allow(clippy::expect_used)] // the main queue always runs on the main thread, and runs what it is given

use crate::capture::{self, CaptureHandle};
use crate::render::{self, Renderer};
use crate::spec::{Region, ShaderSpec};
use crate::ticker::Ticker;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
  NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSScreen, NSWindow,
  NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice, MTLPixelFormat};
use objc2_quartz_core::CAMetalLayer;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Just below floating panels, so an overlay panel (e.g. the Tauri webview) stays on top.
const WINDOW_LEVEL: isize = NSFloatingWindowLevel - 1;

/// Marks a value that may be moved between threads but only *used* on the main thread.
struct MainOnly<T>(T);
unsafe impl<T> Send for MainOnly<T> {}

/// Runs `f` on the main thread and returns its result (directly if already there).
fn on_main<R: Send>(f: impl FnOnce(MainThreadMarker) -> R + Send) -> R {
  if let Some(mtm) = MainThreadMarker::new() {
    return f(mtm);
  }
  let out = Arc::new(Mutex::new(None));
  let slot = out.clone();
  DispatchQueue::main().exec_sync(move || {
    let mtm = MainThreadMarker::new().expect("main queue runs on the main thread");
    *slot.lock() = Some(f(mtm));
  });
  let result = out.lock().take();
  result.expect("main queue ran the closure")
}

fn on_main_async(f: impl FnOnce(MainThreadMarker) + Send + 'static) {
  DispatchQueue::main().exec_async(move || {
    if let Some(mtm) = MainThreadMarker::new() {
      f(mtm);
    }
  });
}

/// Window frame (bottom-left origin) for a global top-left-origin region. The flip is relative
/// to the primary screen, which is the first one and has its origin at (0, 0).
fn frame_for(mtm: MainThreadMarker, r: Region) -> CGRect {
  let primary_height = NSScreen::screens(mtm)
    .firstObject()
    .map_or(0.0, |s| s.frame().size.height);
  CGRect::new(
    CGPoint::new(r.x, primary_height - r.y - r.h),
    CGSize::new(r.w, r.h),
  )
}

/// The transparent, click-through window hosting the Metal layer. Closed on drop.
struct Overlay {
  window: Option<MainOnly<Retained<NSWindow>>>,
}

impl Overlay {
  /// Returns the overlay, its layer and the backing scale factor to capture at.
  fn create(
    device: &Retained<ProtocolObject<dyn MTLDevice>>,
    region: Region,
  ) -> Result<(Self, Retained<CAMetalLayer>, f64), String> {
    let device = MainOnly(device.clone());
    let made = on_main(move |mtm| {
      let device = device;
      let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
          mtm.alloc(),
          frame_for(mtm, region),
          NSWindowStyleMask::Borderless,
          NSBackingStoreType::Buffered,
          false,
        )
      };
      unsafe { window.setReleasedWhenClosed(false) };
      window.setOpaque(false);
      window.setBackgroundColor(Some(&NSColor::clearColor()));
      window.setHasShadow(false);
      window.setIgnoresMouseEvents(true);
      window.setLevel(WINDOW_LEVEL);
      window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
          | NSWindowCollectionBehavior::Stationary
          | NSWindowCollectionBehavior::IgnoresCycle
          | NSWindowCollectionBehavior::FullScreenAuxiliary,
      );
      let scale = window.backingScaleFactor();

      let layer = CAMetalLayer::new();
      layer.setDevice(Some(&device.0));
      layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
      layer.setOpaque(false);
      layer.setContentsScale(scale);
      layer.setDrawableSize(CGSize::new(
        (region.w * scale).round(),
        (region.h * scale).round(),
      ));
      layer.setMaximumDrawableCount(3);

      let made = window.contentView().map(|view| {
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
      });
      window.orderFrontRegardless();
      MainOnly(made.map(|()| (window, layer, scale)))
    });
    let (window, layer, scale) = made.0.ok_or("no content view")?;
    Ok((
      Self {
        window: Some(MainOnly(window)),
      },
      layer,
      scale,
    ))
  }

  fn set_region(&self, region: Region) {
    if let Some(MainOnly(w)) = &self.window {
      let w = MainOnly(w.clone());
      on_main_async(move |mtm| {
        let w = w; // capture the whole wrapper, not its field
        w.0.setFrame_display(frame_for(mtm, region), false);
      });
    }
  }
}

impl Drop for Overlay {
  fn drop(&mut self) {
    if let Some(w) = self.window.take() {
      on_main_async(move |_| {
        let w = w;
        w.0.close();
      });
    }
  }
}

/// Stops the display link on the main thread when dropped.
struct TickerGuard(Option<MainOnly<Ticker>>);

impl TickerGuard {
  fn start(layer: &Retained<CAMetalLayer>, renderer: Renderer) -> Self {
    let layer = MainOnly(layer.clone());
    Self(Some(on_main(move |main| {
      let layer = layer;
      MainOnly(Ticker::start(main, &layer.0, renderer))
    })))
  }
}

impl Drop for TickerGuard {
  fn drop(&mut self) {
    if let Some(ticker) = self.0.take() {
      on_main_async(move |main| {
        let ticker = ticker;
        ticker.0.stop(main);
      });
    }
  }
}

/// A live shader: captures a screen region and draws it back through a WGSL fragment shader.
///
/// Creating one starts everything (window, capture stream, vsync rendering); dropping it
/// stops everything. A shader stays on the display its region started on.
pub struct Shader {
  // Field order is drop order: stop drawing, stop capturing, then tear down the rest.
  _ticker: TickerGuard,
  stream: CaptureHandle,
  renderer: Renderer,
  overlay: Overlay,
  scale: f64,
}

impl std::fmt::Debug for Shader {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Shader").finish_non_exhaustive()
  }
}

impl Shader {
  /// Starts a shader. Needs Screen Recording permission; the first call triggers the macOS
  /// prompt. An invalid spec (WGSL diagnostics, bad uniform values) fails before anything
  /// visible is created.
  pub fn new(spec: &ShaderSpec) -> Result<Self, String> {
    let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
    let pipeline = render::build(&device, spec)?;
    let (overlay, layer, scale) = Overlay::create(&device, spec.region)?;
    let renderer = Renderer::new(device, layer.clone(), scale, pipeline, spec.region)?;

    let frames = renderer.clone();
    let pid = i32::try_from(std::process::id()).map_err(|e| e.to_string())?;
    let stream = capture::start(spec.region, scale, pid, move |f| frames.present(f))?;
    let ticker = TickerGuard::start(&layer, renderer.clone());

    Ok(Self {
      _ticker: ticker,
      stream,
      renderer,
      overlay,
      scale,
    })
  }

  /// Applies a changed spec: rebuilds the pipeline only if the WGSL or declarations changed (on
  /// error the previous one keeps running), and moves the region only if it changed.
  pub fn update(&self, spec: &ShaderSpec) -> Result<(), String> {
    let result = self.renderer.update(spec);
    self.set_region(spec.region);
    result
  }

  /// Overwrites uniform values. Latest wins: nothing is queued.
  pub fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
    self.renderer.set_values(values)
  }

  /// Moves or resizes the captured region.
  pub fn set_region(&self, region: Region) {
    if self.renderer.set_region(region) {
      self.stream.set_region(region, self.scale);
      self.overlay.set_region(region);
    }
  }
}
