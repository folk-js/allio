//! macOS: a session event tap that rewrites mouse motion through a [`Field`].
//!
//! The window server has already moved the cursor by the time a tap sees a move. For each move
//! the [`Tracker`] works out the hand's motion and moves the visual pointer; the real cursor is
//! moved to where the screen drawn under the visual pointer really is (by a marked event of our
//! own, see [`Mover`]), and the event's location rewritten so the app there agrees. While the two differ the system cursor is hidden and the
//! page draws the visual one. Nothing is synthesised: every event is a real one, passed on.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use objc2_core_foundation::{
  kCFRunLoopCommonModes, CFBoolean, CFMachPort, CFRetained, CFRunLoop, CFString,
};
use objc2_core_graphics::{
  CGDirectDisplayID, CGDisplayBounds, CGDisplayHideCursor, CGDisplayShowCursor, CGEvent,
  CGEventField, CGEventMask, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
  CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType, CGGetActiveDisplayList,
  CGMainDisplayID, CGMouseButton,
};
use objc2_core_foundation::CGPoint;
use parking_lot::Mutex;

use crate::field::{Field, Vec2};
use crate::spec::{PointerSpec, PointerState, Rect};
use crate::tracker::Tracker;

/// What the tap's callback works with. Lives as long as the [`Pointer`] that made it.
struct Context {
  field: Mutex<Field>,
  displays: Mutex<Vec<Rect>>,
  tracker: Mutex<Tracker>,
  /// Where the real cursor was last put, for rewriting event deltas.
  real: Mutex<Option<Vec2>>,
  cursor: Mutex<Cursor>,
  mover: Mutex<Mover>,
  /// The tap itself, so the callback can turn it back on when the system disables it.
  port: AtomicPtr<CFMachPort>,
}

/// The tap's run loop, to stop it from another thread.
struct RunLoop(CFRetained<CFRunLoop>);
// SAFETY: `CFRunLoopStop` is documented as callable from any thread.
unsafe impl Send for RunLoop {}

/// Reshapes real mouse motion while it exists. Dropping it removes the tap.
pub struct Pointer {
  context: Arc<Context>,
  run_loop: Option<RunLoop>,
  thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Pointer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Pointer").finish_non_exhaustive()
  }
}

impl Pointer {
  /// Installs the tap. Fails without the Accessibility permission.
  pub fn new(spec: PointerSpec) -> Result<Self, String> {
    let mut field = Field::default();
    field.set(spec);
    let context = Arc::new(Context {
      field: Mutex::new(field),
      displays: Mutex::new(displays()),
      tracker: Mutex::new(Tracker::default()),
      real: Mutex::new(None),
      cursor: Mutex::new(Cursor::default()),
      mover: Mutex::new(Mover::new()),
      port: AtomicPtr::new(std::ptr::null_mut()),
    });

    let (tx, rx) = mpsc::channel();
    let shared = context.clone();
    let thread = thread::Builder::new()
      .name("allio-pointer".into())
      .spawn(move || run(&shared, &tx))
      .map_err(|e| e.to_string())?;

    match rx.recv() {
      Ok(Ok(run_loop)) => Ok(Self {
        context,
        run_loop: Some(run_loop),
        thread: Some(thread),
      }),
      Ok(Err(e)) => {
        drop(thread.join());
        Err(e)
      }
      Err(_) => Err("pointer tap thread exited".into()),
    }
  }

  /// Replaces the field. Takes effect on the next move.
  pub fn set(&self, spec: PointerSpec) {
    self.context.field.lock().set(spec);
    *self.context.displays.lock() = displays();
  }

  /// Where the pointer appears, and whether the page has to draw it.
  pub fn state(&self) -> PointerState {
    let visual = self.context.tracker.lock().visual();
    PointerState {
      x: visual.map_or(0.0, |v| v.x),
      y: visual.map_or(0.0, |v| v.y),
      hidden: self.context.cursor.lock().hidden,
    }
  }
}

impl Drop for Pointer {
  fn drop(&mut self) {
    self.context.cursor.lock().set_hidden(false);
    if let Some(run_loop) = self.run_loop.take() {
      run_loop.0.stop();
    }
    if let Some(thread) = self.thread.take() {
      drop(thread.join());
    }
  }
}

/// The tap thread: create the tap, report back, run until stopped.
fn run(context: &Arc<Context>, ready: &mpsc::Sender<Result<RunLoop, String>>) {
  let mask: CGEventMask = [
    CGEventType::MouseMoved,
    CGEventType::LeftMouseDragged,
    CGEventType::RightMouseDragged,
    CGEventType::OtherMouseDragged,
  ]
  .iter()
  .fold(0, |m, t| m | (1 << t.0));

  // SAFETY: `callback` matches `CGEventTapCallBack`; `user_info` points at a `Context` that the
  // `Arc` held by this thread keeps alive for as long as the tap exists.
  let port = unsafe {
    CGEvent::tap_create(
      CGEventTapLocation::SessionEventTap,
      CGEventTapPlacement::HeadInsertEventTap,
      CGEventTapOptions::Default,
      mask,
      Some(callback),
      Arc::as_ptr(context).cast_mut().cast::<c_void>(),
    )
  };
  let Some(port) = port else {
    drop(ready.send(Err(
      "couldn't create a mouse event tap (is the Accessibility permission granted?)".into(),
    )));
    return;
  };
  context
    .port
    .store(CFRetained::as_ptr(&port).as_ptr(), Ordering::Release);

  let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
    drop(ready.send(Err("couldn't create a run loop source for the tap".into())));
    return;
  };
  let Some(run_loop) = CFRunLoop::current() else {
    drop(ready.send(Err("no run loop".into())));
    return;
  };
  // SAFETY: reading an immutable CF constant.
  let mode = unsafe { kCFRunLoopCommonModes };
  run_loop.add_source(Some(&source), mode);
  CGEvent::tap_enable(&port, true);

  if ready.send(Ok(RunLoop(run_loop))).is_err() {
    return;
  }
  CFRunLoop::run();

  CGEvent::tap_enable(&port, false);
  context.port.store(std::ptr::null_mut(), Ordering::Release);
}

unsafe extern "C-unwind" fn callback(
  _proxy: CGEventTapProxy,
  kind: CGEventType,
  event: NonNull<CGEvent>,
  user_info: *mut c_void,
) -> *mut CGEvent {
  // SAFETY: `user_info` is the `Context` passed to `tap_create`, alive while the tap is.
  let context = unsafe { &*user_info.cast::<Context>() };

  if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
    let port = context.port.load(Ordering::Acquire);
    // SAFETY: the port outlives the run loop that delivers this callback.
    if let Some(port) = unsafe { port.as_ref() } {
      CGEvent::tap_enable(port, true);
    }
    return event.as_ptr();
  }

  // SAFETY: the system hands us a valid event for the duration of the callback.
  reshape(context, kind, unsafe { event.as_ref() });
  event.as_ptr()
}

/// Marks the moves we post, so the tap knows them when they come back through.
const OURS: i64 = 0x0061_6c6c_696f; // "allio"

/// Moves the cursor where the field says, and makes the event agree.
fn reshape(context: &Context, kind: CGEventType, event: &CGEvent) {
  let seen = CGEvent::location(Some(event));
  let seen = Vec2::new(seen.x, seen.y);
  let mut tracker = context.tracker.lock();

  if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData) == OURS {
    tracker.landed(seen);
    return;
  }

  let displays = context.displays.lock();
  let step = tracker.moved(&context.field.lock(), seen, |p| {
    clamp_to_displays(&displays, p)
  });
  drop(displays);
  context
    .cursor
    .lock()
    .set_hidden(step.real.sub(step.visual).len() > 0.5);

  let point = CGPoint {
    x: step.real.x,
    y: step.real.y,
  };
  if step.real.sub(seen).len() > 0.01 {
    context.mover.lock().move_to(kind, event, point);
  }
  CGEvent::set_location(Some(event), point);

  // Apps that read deltas (games, some drags) see the motion the real cursor made.
  let mut real = context.real.lock();
  if let Some(from) = real.replace(step.real) {
    let d = step.real.sub(from);
    CGEvent::set_double_value_field(Some(event), CGEventField::MouseEventDeltaX, d.x);
    CGEvent::set_double_value_field(Some(event), CGEventField::MouseEventDeltaY, d.y);
  }
}

/// Moves the real cursor by posting a marked mouse event of the same kind (so a drag stays a
/// drag). Its source doesn't suppress local events afterwards: `CGWarpMouseCursorPosition` would
/// freeze the hardware for a quarter of a second after every move, and that can't be turned off
/// from a background app.
struct Mover {
  source: Option<CFRetained<CGEventSource>>,
}

// SAFETY: only used from the tap thread, behind a mutex.
unsafe impl Send for Mover {}

impl Mover {
  fn new() -> Self {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    if let Some(source) = &source {
      CGEventSource::set_local_events_suppression_interval(Some(source), 0.0);
    }
    Self { source }
  }

  fn move_to(&self, kind: CGEventType, like: &CGEvent, to: CGPoint) {
    let button = CGMouseButton(
      u32::try_from(CGEvent::integer_value_field(Some(like), CGEventField::MouseEventButtonNumber))
        .unwrap_or(0),
    );
    let Some(event) = CGEvent::new_mouse_event(self.source.as_deref(), kind, to, button) else {
      return;
    };
    CGEvent::set_integer_value_field(Some(&event), CGEventField::EventSourceUserData, OURS);
    CGEvent::set_flags(Some(&event), CGEvent::flags(Some(like)));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
  }
}

/// The system cursor's visibility. While the pointer acts somewhere other than where it appears,
/// the system cursor (drawn where it acts) is hidden and the page draws it where it appears.
#[derive(Default)]
struct Cursor {
  hidden: bool,
  allowed: bool,
}

impl Cursor {
  fn set_hidden(&mut self, hidden: bool) {
    if hidden == self.hidden {
      return;
    }
    if !self.allowed {
      allow_hiding_in_background();
      self.allowed = true;
    }
    let display = CGMainDisplayID();
    let err = if hidden {
      CGDisplayHideCursor(display)
    } else {
      CGDisplayShowCursor(display)
    };
    if err.0 == 0 {
      self.hidden = hidden;
    }
  }
}

/// Lets this process hide the cursor while another app is frontmost, which the public API
/// doesn't allow. Private (`CGSSetConnectionProperty`), but stable for many macOS releases.
fn allow_hiding_in_background() {
  #[link(name = "CoreGraphics", kind = "framework")]
  extern "C" {
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(cid: i32, target: i32, key: *const c_void, value: *const c_void)
      -> i32;
  }
  let key = CFString::from_static_str("SetsCursorInBackground");
  // SAFETY: both pointers are valid CF objects for the duration of the call.
  let err = unsafe {
    let cid = _CGSDefaultConnection();
    CGSSetConnectionProperty(
      cid,
      cid,
      CFRetained::as_ptr(&key).as_ptr().cast(),
      std::ptr::from_ref(CFBoolean::new(true)).cast(),
    )
  };
  if err != 0 {
    log::warn!("couldn't allow hiding the cursor in the background: {err}");
  }
}

/// The active displays, in global screen points.
fn displays() -> Vec<Rect> {
  let mut ids: [CGDirectDisplayID; 16] = [0; 16];
  let mut count = 0u32;
  // SAFETY: `ids` has room for 16 displays and `count` receives how many were written.
  let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &raw mut count) };
  if err.0 != 0 {
    log::warn!("CGGetActiveDisplayList failed: {}", err.0);
    return Vec::new();
  }
  ids
    .iter()
    .take(count as usize)
    .map(|&id| {
      let b = CGDisplayBounds(id);
      Rect {
        x: b.origin.x,
        y: b.origin.y,
        w: b.size.width,
        h: b.size.height,
      }
    })
    .collect()
}

/// Keeps a point on some display, as the window server does with the real cursor.
fn clamp_to_displays(displays: &[Rect], p: Vec2) -> Vec2 {
  let clamp = |r: &Rect| {
    Vec2::new(
      p.x.clamp(r.x, r.x + r.w - 1.0),
      p.y.clamp(r.y, r.y + r.h - 1.0),
    )
  };
  displays
    .iter()
    .map(clamp)
    .min_by(|a, b| {
      let da = (a.x - p.x).hypot(a.y - p.y);
      let db = (b.x - p.x).hypot(b.y - p.y);
      da.total_cmp(&db)
    })
    .unwrap_or(p)
}
