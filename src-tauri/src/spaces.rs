//! The overlay on every Space, sliding with each one.
//!
//! The page is one document, in one webview, in an overlay window that joins every Space and
//! takes all input. It doesn't draw itself: macOS holds a window that is on every Space still
//! while you swipe between Spaces, so it would pop out and back in. Instead each Space gets a
//! mirror, a click-through window placed on that Space, which shows the page through
//! `CAPortalLayer`s (Core Animation rendering layers of another layer context): the whole page,
//! and on top, that Space's container (the server side of `AllioBelonging` in allio-client:
//! UI belonging to a window, an element or a Space lives in its Space's container, hidden in the
//! page itself). So everything is on its Space all the time, live, and slides with it. Input
//! reaches the page through the overlay, where everything is at the same place as it appears.
//!
//! The overlay hides its own drawing (its webview's superview at opacity 0; portals don't take on
//! their source's ancestors' opacity) only once the current Space's mirror is showing the page.
//! If mirrors can't be made, it keeps drawing itself as before.
//!
//! Finding a container's layer: WebKit names layers after their element (`… id='allio-space-<id>'
//! …`), and the client also gives each container an opacity of `1 - marker / 100000` (renders
//! exactly like 1), read back off the layer. Both must agree; if WebKit ever stops naming
//! layers, the opacity alone is used. WebKit replaces a container's layer only if the container
//! moves in the DOM or comes back from `display: none` (the client does neither); a rescan, four
//! times a second, rebinds it anyway. See `docs/SPACES.md` and `probes/layers.swift`.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use allio::{Allio, SpaceId};
use allio_ws::ConnId;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
  NSBackingStoreType, NSColor, NSPanel, NSView, NSWindow, NSWindowCollectionBehavior,
  NSWindowStyleMask,
};
use objc2_foundation::{NSNumber, NSRect, NSString};
use objc2_quartz_core::CALayer;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// How often layers are rescanned and mirrors reconciled.
const TICK: Duration = Duration::from_millis(250);
/// Mirrors are where the overlay was: floating, above ordinary windows.
const MIRROR_LEVEL: isize = 3;

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
  fn CALayerGetRenderId(layer: *const CALayer) -> u64;
}

#[link(name = "ColorSync", kind = "framework")]
extern "C" {
  fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
  fn CFUUIDCreateString(allocator: *const c_void, uuid: *const c_void) -> *const c_void;
  fn CFRelease(cf: *const c_void);
}

/// One Space's container, as the client declares it.
#[derive(Deserialize)]
struct Layer {
  space: u64,
  marker: u32,
}

#[derive(Deserialize)]
struct Request {
  layers: Vec<Layer>,
}

#[derive(Default)]
struct State {
  owner: Option<ConnId>,
  /// Space id to marker.
  layers: BTreeMap<u64, u32>,
}

pub struct Spaces {
  state: Mutex<State>,
}

impl Spaces {
  /// Starts reconciling (four times a second, on the main thread).
  pub fn new(app: AppHandle, allio: Allio) -> std::sync::Arc<Self> {
    let spaces = std::sync::Arc::new(Self {
      state: Mutex::new(State::default()),
    });
    let ticking = spaces.clone();
    std::thread::spawn(move || loop {
      std::thread::sleep(TICK);
      let declared = ticking.state.lock().unwrap().layers.clone();
      let all = allio.spaces();
      let handle = app.clone();
      if app
        .run_on_main_thread(move || reconcile(&handle, &all, &declared))
        .is_err()
      {
        break;
      }
    });
    spaces
  }

  /// Handles `space_layers_set`; anything else falls through.
  pub fn handle(&self, conn: ConnId, method: &str, args: &Value) -> Option<Value> {
    if method != "space_layers_set" {
      return None;
    }
    let request: Request = match serde_json::from_value(args.clone()) {
      Ok(r) => r,
      Err(e) => return Some(json!({ "error": e.to_string() })),
    };
    let mut state = self.state.lock().unwrap();
    state.owner = Some(conn);
    state.layers = request.layers.iter().map(|l| (l.space, l.marker)).collect();
    Some(json!({ "result": null }))
  }

  /// Forgets the containers if the closed connection declared them.
  pub fn disconnected(&self, conn: ConnId) {
    let mut state = self.state.lock().unwrap();
    if state.owner == Some(conn) {
      *state = State::default();
    }
  }
}

define_class!(
  /// A view with a top-left origin, like WebKit's layers, so portals land where their source is.
  #[unsafe(super(NSView))]
  #[thread_kind = MainThreadOnly]
  #[name = "AllioFlippedView"]
  struct FlippedView;

  impl FlippedView {
    #[unsafe(method(isFlipped))]
    fn is_flipped(&self) -> bool {
      true
    }
  }
);

/// A window on one Space showing the page, and that Space's container on top.
struct Mirror {
  panel: Retained<NSPanel>,
  page: Retained<CALayer>,
  container: Retained<CALayer>,
  created: Instant,
  /// Placed on its Space (it starts on the current one, invisible).
  placed: bool,
  /// What each portal shows (render ids), once found.
  page_bound: Option<u64>,
  container_bound: Option<u64>,
}

thread_local! {
  static MIRRORS: RefCell<HashMap<u64, Mirror>> = RefCell::new(HashMap::new());
  /// Containers we have already warned about (their signals disagreed).
  static WARNED: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
  /// Whether mirrors can't be made here (no `CAPortalLayer`).
  static UNSUPPORTED: Cell<bool> = const { Cell::new(false) };
  /// Whether we have warned that the overlay couldn't be found.
  static MISSING_OVERLAY: Cell<bool> = const { Cell::new(false) };
}

/// The overlay: its window, its webview's view, and its root layer's context id (what portals
/// reference).
struct Overlay {
  window: Retained<NSWindow>,
  web: Retained<NSView>,
  context: u32,
}

fn overlay(app: &AppHandle) -> Option<Overlay> {
  let window = app.get_webview_window("main")?;
  let window: Retained<NSWindow> =
    unsafe { Retained::retain(window.ns_window().ok()?.cast::<NSWindow>())? };
  let content = window.contentView()?;
  let root = content.layer()?;
  let context: Option<Retained<AnyObject>> =
    unsafe { msg_send![&*root, valueForKey: &*NSString::from_str("context")] };
  let id: Option<Retained<NSNumber>> =
    unsafe { msg_send![&*context?, valueForKey: &*NSString::from_str("contextId")] };
  Some(Overlay {
    web: find_webview(&content)?,
    window,
    context: id?.as_u32(),
  })
}

/// The webview in a view tree: a `WKWebView` or a subclass (Tauri's is `WryWebView`).
fn find_webview(view: &NSView) -> Option<Retained<NSView>> {
  let class = AnyClass::get(c"WKWebView")?;
  let is_webview: bool = unsafe { msg_send![view, isKindOfClass: class] };
  if is_webview {
    return unsafe { Retained::retain(std::ptr::from_ref(view).cast_mut()) };
  }
  view.subviews().iter().find_map(|v| find_webview(&v))
}

/// The layer to show the whole page from. Not the webview's own layer: that one flips y
/// (`geometryFlipped`), bridging AppKit's bottom-up coordinates to WebKit's top-down ones, and a
/// portal shows its source's contents without its source's own flip, so the page would come out
/// upside down. Its child holds WebKit's layers in WebKit's top-down coordinates, the same as
/// the containers the other portals show (and as the mirrors' flipped views).
fn page_layer(web: &NSView) -> Option<Retained<CALayer>> {
  let layer = web.layer()?;
  unsafe { layer.sublayers() }?.firstObject()
}

/// The UUID of the display a window is on, as Spaces name displays.
fn display_uuid(window: &NSWindow) -> Option<String> {
  let screen = window.screen()?;
  let number: Retained<NSNumber> = unsafe {
    let description = screen.deviceDescription();
    let value = description.objectForKey(&NSString::from_str("NSScreenNumber"))?;
    Retained::cast_unchecked(value)
  };
  unsafe {
    let uuid = CGDisplayCreateUUIDFromDisplayID(number.as_u32());
    if uuid.is_null() {
      return None;
    }
    let string = CFUUIDCreateString(std::ptr::null(), uuid);
    CFRelease(uuid);
    if string.is_null() {
      return None;
    }
    let text = (*string.cast::<NSString>()).to_string();
    CFRelease(string);
    Some(text)
  }
}

/// The layer showing each declared container, by Space: named after its element and carrying
/// its opacity marker (both must agree; the marker alone if WebKit names no layers).
fn container_layers(root: &CALayer, declared: &BTreeMap<u64, u32>) -> HashMap<u64, u64> {
  let mut by_name: HashMap<u64, Vec<Retained<CALayer>>> = HashMap::new();
  let mut by_marker: HashMap<u64, Vec<Retained<CALayer>>> = HashMap::new();
  let mut any_named = false;
  let Some(root) = (unsafe { Retained::retain(std::ptr::from_ref(root).cast_mut()) }) else {
    return HashMap::new();
  };
  let mut stack = vec![root];
  while let Some(layer) = stack.pop() {
    let name = layer.name().map(|n| n.to_string());
    if name.is_some() {
      any_named = true;
    }
    let opacity = f64::from(layer.opacity());
    for (space, marker) in declared {
      let is_named = name.as_deref().is_some_and(|n| {
        n.contains(&format!("id='allio-space-{space}'")) && !n.ends_with("(anchor)")
      });
      if is_named {
        by_name.entry(*space).or_default().push(layer.clone());
      }
      if (opacity - (1.0 - f64::from(*marker) / 100_000.0)).abs() < 2e-6 {
        by_marker.entry(*space).or_default().push(layer.clone());
      }
    }
    if let Some(subs) = unsafe { layer.sublayers() } {
      stack.extend(subs.iter());
    }
  }

  let mut found = HashMap::new();
  for space in declared.keys() {
    let named = by_name.get(space).map_or(&[][..], Vec::as_slice);
    let marked = by_marker.get(space).map_or(&[][..], Vec::as_slice);
    let layer = match (named, marked) {
      ([n], [m]) if Retained::as_ptr(n) == Retained::as_ptr(m) => Some(n),
      ([], [m]) if !any_named => Some(m),
      ([], []) => None,
      _ => {
        if WARNED.with(|w| w.borrow_mut().insert(*space)) {
          log::warn!(
            "space {space}: its container's layer is ambiguous ({} by name, {} by marker); not shown",
            named.len(),
            marked.len()
          );
        }
        None
      }
    };
    if let Some(layer) = layer {
      found.insert(*space, unsafe { CALayerGetRenderId(Retained::as_ptr(layer)) });
    }
  }
  found
}

fn set(object: &AnyObject, key: &str, value: &AnyObject) {
  let _: () = unsafe { msg_send![object, setValue: value, forKey: &*NSString::from_str(key)] };
}

fn portal(context: u32) -> Option<Retained<CALayer>> {
  let class = AnyClass::get(c"CAPortalLayer")?;
  let portal: Retained<CALayer> = unsafe { msg_send![class, new] };
  set(&portal, "sourceContextId", &NSNumber::new_u32(context));
  set(&portal, "matchesPosition", &NSNumber::new_bool(true));
  set(&portal, "crossDisplay", &NSNumber::new_bool(true));
  portal.setHidden(true);
  Some(portal)
}

fn mirror(mtm: MainThreadMarker, frame: NSRect, context: u32) -> Option<Mirror> {
  let page = portal(context)?;
  let container = portal(context)?;
  let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
    NSPanel::alloc(mtm),
    frame,
    NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
    NSBackingStoreType::Buffered,
    false,
  );
  unsafe { panel.setReleasedWhenClosed(false) };
  panel.setLevel(MIRROR_LEVEL);
  panel.setOpaque(false);
  panel.setBackgroundColor(Some(&NSColor::clearColor()));
  panel.setHasShadow(false);
  panel.setIgnoresMouseEvents(true);
  // On its own Space only (placed below), including full-screen ones.
  panel.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenAuxiliary);
  let view: Retained<FlippedView> = unsafe { msg_send![FlippedView::alloc(mtm), init] };
  view.setWantsLayer(true);
  let layer = view.layer()?;
  // The page, then its Space's own UI on top (as in the page: the containers come last).
  layer.addSublayer(&page);
  layer.addSublayer(&container);
  panel.setContentView(Some(&view));
  page.setFrame(view.bounds());
  container.setFrame(view.bounds());
  // Invisible until it is on its Space: ordering in puts it on the current one first.
  panel.setAlphaValue(0.0);
  panel.orderFrontRegardless();
  Some(Mirror {
    panel,
    page,
    container,
    created: Instant::now(),
    placed: false,
    page_bound: None,
    container_bound: None,
  })
}

/// Points a portal at a layer (or hides it). Returns the new binding.
fn bind(portal: &CALayer, bound: Option<u64>, layer: Option<u64>, context: u32) -> Option<u64> {
  if layer != bound {
    if let Some(id) = layer {
      set(portal, "sourceContextId", &NSNumber::new_u32(context));
      set(portal, "sourceLayerRenderId", &NSNumber::new_u64(id));
    }
    portal.setHidden(layer.is_none());
  }
  layer
}

/// Hides (or shows) the overlay's own drawing; its mirrors show it instead.
fn hide_overlay(overlay: &Overlay, hide: bool) {
  if let Some(layer) = unsafe { overlay.web.superview() }.and_then(|v| v.layer()) {
    let opacity = if hide { 0.0 } else { 1.0 };
    if (layer.opacity() - opacity).abs() > f32::EPSILON {
      layer.setOpacity(opacity);
    }
  }
}

/// Makes the mirrors match the Spaces (on the overlay's display) and the declared containers.
/// Main thread.
fn reconcile(app: &AppHandle, spaces: &[allio::Space], declared: &BTreeMap<u64, u32>) {
  let Some(mtm) = MainThreadMarker::new() else {
    return;
  };
  let Some(overlay) = overlay(app) else {
    if !MISSING_OVERLAY.replace(true) {
      log::warn!("spaces: can't find the overlay's window, layer context or webview");
    }
    return;
  };
  if UNSUPPORTED.get() {
    hide_overlay(&overlay, false);
    return;
  }
  let display = display_uuid(&overlay.window);
  let ours: Vec<&allio::Space> = spaces
    .iter()
    .filter(|s| display.as_deref().is_none_or(|d| s.display == d))
    .collect();
  let page = page_layer(&overlay.web).map(|l| unsafe { CALayerGetRenderId(Retained::as_ptr(&l)) });
  let containers = overlay
    .web
    .layer()
    .map(|l| container_layers(&l, declared))
    .unwrap_or_default();
  let frame = overlay.window.frame();

  MIRRORS.with(|mirrors| {
    let mut mirrors = mirrors.borrow_mut();
    let live: HashSet<u64> = ours.iter().map(|s| s.id.0).collect();
    mirrors.retain(|space, m| {
      let keep = live.contains(space);
      if !keep {
        m.panel.orderOut(None);
      }
      keep
    });

    let mut current_shown = false;
    for space in &ours {
      let id = space.id.0;
      let m = match mirrors.entry(id) {
        std::collections::hash_map::Entry::Occupied(m) => m.into_mut(),
        std::collections::hash_map::Entry::Vacant(slot) => {
          let Some(m) = mirror(mtm, frame, overlay.context) else {
            log::warn!("no CAPortalLayer on this macOS: the overlay draws itself, without Spaces");
            UNSUPPORTED.set(true);
            hide_overlay(&overlay, false);
            return;
          };
          slot.insert(m)
        }
      };
      if m.panel.frame() != frame {
        m.panel.setFrame_display(frame, false);
        if let Some(view) = m.panel.contentView() {
          m.page.setFrame(view.bounds());
          m.container.setFrame(view.bounds());
        }
      }
      // Placed once AppKit has had a run loop turn to order it in.
      if !m.placed && m.created.elapsed() >= TICK / 2 {
        let number = u32::try_from(m.panel.windowNumber()).unwrap_or(0);
        m.placed = allio::place_window_on_space(number, SpaceId(id));
        if m.placed {
          m.panel.setAlphaValue(1.0);
        }
      }
      m.page_bound = bind(&m.page, m.page_bound, page, overlay.context);
      m.container_bound = bind(&m.container, m.container_bound, containers.get(&id).copied(), overlay.context);
      if space.current && m.placed && m.page_bound.is_some() {
        current_shown = true;
      }
    }
    // The overlay draws itself until the current Space's mirror shows the page.
    hide_overlay(&overlay, current_shown);
  });
}
