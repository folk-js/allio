#![allow(unsafe_code)]
#![allow(clippy::expect_used)] // the main queue always runs on the main thread, and runs what it is given

use crate::capture::{self, CaptureHandle};
use crate::render::{self, Renderer};
use crate::spec::{Hide, Region, ShaderSpec, Source};
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
use std::collections::{BTreeMap, BTreeSet};
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

/// What a running capture is of, to tell whether a changed spec still wants it.
#[derive(Debug, Clone, PartialEq)]
enum Want {
  /// Part of the display: the shader's region (`None`) or its own.
  Display { region: Option<Region>, hide: Hide },
  /// One window.
  Window(u32),
}

impl Want {
  /// Whether a running capture for `self` can be adjusted into one for `other`.
  const fn same_kind(&self, other: &Self) -> bool {
    match (self, other) {
      (Self::Display { .. }, Self::Display { .. }) => true,
      (Self::Window(a), Self::Window(b)) => *a == *b,
      _ => false,
    }
  }
}

/// The captures a spec needs: `screen`, `behind` and its named sources, but only the ones the
/// shader actually reads.
fn wanted(spec: &ShaderSpec, reads: &BTreeSet<String>) -> BTreeMap<String, Want> {
  let mut want = BTreeMap::new();
  if reads.contains(render::SCREEN) {
    want.insert(
      render::SCREEN.to_string(),
      Want::Display {
        region: None,
        hide: spec.hide.clone(),
      },
    );
  }
  if let (Some(hide), true) = (&spec.behind, reads.contains(render::BEHIND)) {
    want.insert(
      render::BEHIND.to_string(),
      Want::Display {
        region: None,
        hide: hide.clone(),
      },
    );
  }
  for (name, source) in &spec.sources {
    let Some(source) = source.as_ref().filter(|_| reads.contains(name)) else {
      continue;
    };
    let w = match source {
      Source::Window { window } => Want::Window(*window),
      Source::Display { region, hide } => Want::Display {
        region: Some(*region),
        hide: hide.clone().unwrap_or_default(),
      },
    };
    want.insert(name.clone(), w);
  }
  want
}

/// A live shader: draws a screen region back through a WGSL fragment shader, reading captures
/// of the screen and of windows.
///
/// Creating one starts everything (window, captures, vsync rendering); dropping it stops
/// everything. A shader stays on the display its region started on.
pub struct Shader {
  // Field order is drop order: stop drawing, stop capturing, then tear down the rest.
  _ticker: TickerGuard,
  /// Running captures by texture name, with what each is of.
  captures: Mutex<BTreeMap<String, (Want, CaptureHandle)>>,
  /// The spec as last applied, patches included.
  spec: Mutex<ShaderSpec>,
  renderer: Renderer,
  overlay: Overlay,
  scale: f64,
  pid: i32,
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
    let renderer = Renderer::new(device, layer.clone(), scale, pipeline, spec)?;
    let ticker = TickerGuard::start(&layer, renderer.clone());
    let shader = Self {
      _ticker: ticker,
      captures: Mutex::new(BTreeMap::new()),
      spec: Mutex::new(spec.clone()),
      renderer,
      overlay,
      scale,
      pid: i32::try_from(std::process::id()).map_err(|e| e.to_string())?,
    };
    shader.sync()?;
    Ok(shader)
  }

  /// Applies a changed spec: rebuilds the pipeline only if the WGSL or declarations changed (on
  /// error the previous one keeps running), moves the region, and starts, adjusts or stops
  /// captures to match.
  pub fn update(&self, spec: &ShaderSpec) -> Result<(), String> {
    {
      let current = self.spec.lock();
      if current.behind.is_some() != spec.behind.is_some() {
        return Err("`behind` can't be added or removed while a shader is running".to_string());
      }
    }
    let result = self.renderer.update(spec);
    *self.spec.lock() = spec.clone();
    self.set_region(spec.region);
    result.and(self.sync())
  }

  /// Overwrites uniform values. Latest wins: nothing is queued.
  pub fn set_values(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<(), String> {
    self.renderer.set_values(values)
  }

  /// Changes which windows are left out of the captured `screen`.
  pub fn set_hide(&self, hide: &Hide) -> Result<(), String> {
    self.spec.lock().hide = hide.clone();
    self.sync()
  }

  /// Changes which windows are left out of `behind`.
  pub fn set_behind(&self, hide: &Hide) -> Result<(), String> {
    {
      let mut spec = self.spec.lock();
      let behind = spec
        .behind
        .as_mut()
        .ok_or_else(|| "this shader has no `behind` capture".to_string())?;
      *behind = hide.clone();
    }
    self.sync()
  }

  /// How many captures are running for it.
  pub fn captures(&self) -> usize {
    self.captures.lock().len()
  }

  /// Reads one cell of the simulation state at a screen point, as `[r, g, b, a]`.
  pub fn probe(&self, x: f64, y: f64) -> Result<[f32; 4], String> {
    self.renderer.probe(x, y)
  }

  /// Moves or resizes the drawn region (and the captures that follow it).
  pub fn set_region(&self, region: Region) {
    self.spec.lock().region = region;
    if self.renderer.set_region(region) {
      for (want, capture) in self.captures.lock().values() {
        if matches!(want, Want::Display { region: None, .. }) {
          capture.set_region(region, self.scale);
        }
      }
      self.overlay.set_region(region);
    }
  }

  /// Tells window captures their window's new size (in points), so they stay one to one.
  pub fn window_resized(&self, window: u32, w: f64, h: f64) {
    for (want, capture) in self.captures.lock().values() {
      if *want == Want::Window(window) {
        capture.set_size(w, h, self.scale);
      }
    }
  }

  /// Makes the running captures match the spec and what the shader reads. Reports the first
  /// capture that couldn't be started or changed; the others are still applied.
  fn sync(&self) -> Result<(), String> {
    let spec = self.spec.lock().clone();
    let want = wanted(&spec, &self.renderer.reads());
    let mut captures = self.captures.lock();
    captures.retain(|name, (had, _)| {
      let keep = want.get(name).is_some_and(|w| had.same_kind(w));
      if !keep {
        self.renderer.clear(name);
      }
      keep
    });

    let mut first_err = None;
    for (name, w) in want {
      let result = if let Some((had, capture)) = captures.get_mut(&name) {
        let mut result = Ok(());
        if let (
          Want::Display {
            region: r0,
            hide: h0,
          },
          Want::Display {
            region: r1,
            hide: h1,
          },
        ) = (&*had, &w)
        {
          if let (Some(r), true) = (r1, r0 != r1) {
            capture.set_region(*r, self.scale);
          }
          if h0 != h1 {
            result = capture.set_hide(h1);
          }
        }
        *had = w;
        result
      } else {
        let frames = self.renderer.clone();
        let named = name.clone();
        let deliver = move |f| frames.present(&named, f);
        let started = match &w {
          Want::Window(id) => capture::start_window(*id, self.scale, deliver),
          Want::Display { region, hide } => capture::start(
            region.unwrap_or(spec.region),
            self.scale,
            hide,
            self.pid,
            deliver,
          ),
        };
        started.map(|capture| {
          captures.insert(name.clone(), (w, capture));
        })
      };
      if let Err(e) = result {
        first_err = first_err.or(Some(format!("{name}: {e}")));
      }
    }
    first_err.map_or(Ok(()), Err)
  }
}
