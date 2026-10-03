#![allow(unsafe_code)]

//! `ScreenCaptureKit` stream of one region, delivered as Metal textures.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // pixel sizes are positive and small

use crate::diag::warn_once;
use crate::spec::{Hide, Region};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetIOSurface, CVPixelBufferGetWidth};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_io_surface::IOSurfaceRef;
use objc2_screen_capture_kit::{
  SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
  SCStreamOutputType,
};
use parking_lot::Mutex;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

/// 'BGRA' fourcc.
const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

/// Frame rate ceiling. `ScreenCaptureKit` only delivers a frame when the screen changes.
const MAX_FPS: i32 = 120;

/// Windows below this level are ordinary app windows and floating panels. At and above it are the
/// Dock, the menu bar and notifications.
const DOCK_LEVEL: isize = 20;

/// How long to wait for `ScreenCaptureKit` callbacks, including the Screen Recording prompt.
const TIMEOUT: Duration = Duration::from_secs(10);

/// A captured frame. The surface is recycled by `ScreenCaptureKit` from a small pool: drop the
/// frame as soon as the GPU is done with it or the stream will stall.
pub(crate) struct Frame {
  pub(crate) surface: CFRetained<IOSurfaceRef>,
  pub(crate) width: usize,
  pub(crate) height: usize,
  _hold: CFRetained<CMSampleBuffer>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[name = "AllioShaderCaptureOutput"]
  #[ivars = Callback]
  struct Output;

  unsafe impl NSObjectProtocol for Output {}

  unsafe impl SCStreamOutput for Output {
    #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
    fn did_output(&self, _stream: &SCStream, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
      if kind != SCStreamOutputType::Screen {
        return;
      }
      // Frames without an image buffer are status updates (idle/blank).
      let Some(pb) = (unsafe { buffer.image_buffer() }) else {
        return;
      };
      let Some(surface) = CVPixelBufferGetIOSurface(Some(&pb)) else {
        return warn_once("capture: frame", "pixel buffer has no IOSurface");
      };
      let (width, height) = (CVPixelBufferGetWidth(&pb), CVPixelBufferGetHeight(&pb));
      let hold = unsafe { CFRetained::retain(NonNull::from(buffer)) };
      (self.ivars().0)(Frame {
        surface,
        width,
        height,
        _hold: hold,
      });
    }
  }
);

/// Receives each frame on the capture queue.
struct Callback(Box<dyn Fn(Frame) + Send + Sync>);

/// A running capture. Stops on drop.
pub(crate) struct CaptureHandle {
  stream: Retained<SCStream>,
  config: Retained<SCStreamConfiguration>,
  /// Top-left of the captured display in global coordinates. Regions are global, but
  /// `ScreenCaptureKit` wants them relative to the display.
  display_origin: CGPoint,
  display_id: u32,
  exclude_pid: i32,
  hide: Mutex<Hide>,
  _output: Retained<Output>,
  _queue: dispatch2::DispatchRetained<DispatchQueue>,
}

// `SCStream` is thread-safe; we only call `updateConfiguration` and stop on it.
unsafe impl Send for CaptureHandle {}

impl Drop for CaptureHandle {
  fn drop(&mut self) {
    let done = RcBlock::new(|_: *mut NSError| {});
    unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&done)) };
  }
}

struct SendContent(Retained<SCShareableContent>);
unsafe impl Send for SendContent {}

/// Runs an asynchronous `ScreenCaptureKit` call and waits for the result it sends back.
fn wait<T: Send>(
  what: &str,
  begin: impl FnOnce(mpsc::Sender<Result<T, String>>),
) -> Result<T, String> {
  let (tx, rx) = mpsc::channel();
  begin(tx);
  rx.recv_timeout(TIMEOUT)
    .map_err(|_| format!("timed out {what}"))?
}

fn describe(err: &NSError) -> String {
  err.localizedDescription().to_string()
}

fn local_rect(region: Region, display_origin: CGPoint) -> CGRect {
  CGRect::new(
    CGPoint::new(region.x - display_origin.x, region.y - display_origin.y),
    CGSize::new(region.w, region.h),
  )
}

fn pixels(points: f64, scale: f64) -> usize {
  (points * scale).round().max(1.0) as usize
}

/// The display containing the middle of the region, or the first one.
fn display_for(content: &SCShareableContent, region: Region) -> Option<Retained<SCDisplay>> {
  let (cx, cy) = (region.x + region.w / 2.0, region.y + region.h / 2.0);
  let displays = unsafe { content.displays() };
  displays
    .iter()
    .find(|d| {
      let f = unsafe { d.frame() };
      cx >= f.origin.x
        && cx < f.origin.x + f.size.width
        && cy >= f.origin.y
        && cy < f.origin.y + f.size.height
    })
    .or_else(|| displays.firstObject())
}

/// Fetches the current windows, displays and applications.
fn shareable_content() -> Result<SendContent, String> {
  wait(
    "getting shareable content (is Screen Recording allowed?)",
    |tx| {
      let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let result = unsafe { Retained::retain(content) }
          .map(SendContent)
          .ok_or_else(|| {
            unsafe { err.as_ref() }.map_or_else(|| "no shareable content".to_string(), describe)
          });
        drop(tx.send(result));
      });
      unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    },
  )
}

/// Builds the filter for `hide`. Windows of process `exclude_pid` are always left out; in
/// `Hide::Windows` mode that covers the ones that exist right now.
fn filter_for(
  content: &SCShareableContent,
  display: &SCDisplay,
  hide: &Hide,
  exclude_pid: i32,
) -> Retained<SCContentFilter> {
  unsafe {
    match hide {
      Hide::None | Hide::All => {
        // `All` leaves out every application that owns an ordinary window, and nothing else, so
        // the desktop (drawn by the Dock and the wallpaper agent), the Dock and the menu bar stay.
        let with_windows: Vec<i32> = if matches!(hide, Hide::All) {
          content
            .windows()
            .iter()
            .filter(|w| (0..DOCK_LEVEL).contains(&w.windowLayer()))
            .filter_map(|w| w.owningApplication().map(|a| a.processID()))
            .collect()
        } else {
          Vec::new()
        };
        let excluded: Vec<_> = content
          .applications()
          .iter()
          .filter(|a| a.processID() == exclude_pid || with_windows.contains(&a.processID()))
          .collect();
        if excluded.is_empty() {
          warn_once(
            "capture: filter",
            "own app not found; its windows will be captured",
          );
        }
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
          SCContentFilter::alloc(),
          display,
          &NSArray::from_retained_slice(&excluded),
          &NSArray::new(),
        )
      }
      Hide::Windows(ids) => {
        let excluded: Vec<_> = content
          .windows()
          .iter()
          .filter(|w| {
            ids.contains(&w.windowID())
              || w
                .owningApplication()
                .is_some_and(|a| a.processID() == exclude_pid)
          })
          .collect();
        SCContentFilter::initWithDisplay_excludingWindows(
          SCContentFilter::alloc(),
          display,
          &NSArray::from_retained_slice(&excluded),
        )
      }
    }
  }
}

impl CaptureHandle {
  /// Changes which windows are left out of the capture, without restarting the stream.
  pub(crate) fn set_hide(&self, hide: &Hide) -> Result<(), String> {
    let mut current = self.hide.lock();
    if *current == *hide {
      return Ok(());
    }
    let content = shareable_content()?.0;
    let display = unsafe { content.displays() }
      .iter()
      .find(|d| unsafe { d.displayID() } == self.display_id)
      .ok_or("the captured display went away")?;
    let filter = filter_for(&content, &display, hide, self.exclude_pid);
    unsafe {
      self
        .stream
        .updateContentFilter_completionHandler(&filter, None);
    }
    current.clone_from(hide);
    Ok(())
  }

  /// Moves or resizes the captured region without restarting the stream. `scale` is output
  /// pixels per point. The region must stay on the display the stream started on.
  pub(crate) fn set_region(&self, region: Region, scale: f64) {
    unsafe {
      self
        .config
        .setSourceRect(local_rect(region, self.display_origin));
      self.config.setWidth(pixels(region.w, scale));
      self.config.setHeight(pixels(region.h, scale));
      self
        .stream
        .updateConfiguration_completionHandler(&self.config, None);
    }
  }
}

/// Starts capturing `region`, excluding every window of process `exclude_pid` (including ones
/// created later). Blocks until the stream is running. Needs Screen Recording permission; the
/// first call triggers the macOS prompt.
pub(crate) fn start(
  region: Region,
  scale: f64,
  hide: &Hide,
  exclude_pid: i32,
  on_frame: impl Fn(Frame) + Send + Sync + 'static,
) -> Result<CaptureHandle, String> {
  let content = shareable_content()?.0;
  let display = display_for(&content, region).ok_or("no displays")?;
  let display_origin = unsafe { display.frame() }.origin;
  let display_id = unsafe { display.displayID() };
  let filter = filter_for(&content, &display, hide, exclude_pid);

  let config = unsafe { SCStreamConfiguration::new() };
  unsafe {
    config.setSourceRect(local_rect(region, display_origin));
    config.setWidth(pixels(region.w, scale));
    config.setHeight(pixels(region.h, scale));
    config.setPixelFormat(PIXEL_FORMAT_BGRA);
    config.setShowsCursor(false);
    config.setQueueDepth(3);
    config.setMinimumFrameInterval(CMTime::new(1, MAX_FPS));
  }

  let stream = unsafe {
    SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None)
  };
  let output: Retained<Output> = {
    let this = Output::alloc().set_ivars(Callback(Box::new(on_frame)));
    unsafe { msg_send![super(this), init] }
  };
  let queue = DispatchQueue::new("allio.shader.capture", DispatchQueueAttr::SERIAL);
  unsafe {
    stream.addStreamOutput_type_sampleHandlerQueue_error(
      ProtocolObject::from_ref(&*output),
      SCStreamOutputType::Screen,
      Some(&queue),
    )
  }
  .map_err(|e| describe(&e))?;

  // Build the handle before starting so that any failure below stops the stream on drop.
  let handle = CaptureHandle {
    stream,
    config,
    display_origin,
    display_id,
    exclude_pid,
    hide: Mutex::new(hide.clone()),
    _output: output,
    _queue: queue,
  };
  let stream = &handle.stream;
  wait("starting capture", |tx| {
    let block = RcBlock::new(move |err: *mut NSError| {
      drop(tx.send(unsafe { err.as_ref() }.map_or(Ok(()), |e| Err(describe(e)))));
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
  })?;
  Ok(handle)
}
