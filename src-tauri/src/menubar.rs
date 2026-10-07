//! The menu bar item, as native menus built fresh on every click: left click is a launcher
//! (the overlays, grouped and described), right click is a status menu (what allio is doing
//! right now, and a way out of each thing).
//!
//! Overlays describe themselves: `<title>`, `<meta name="description">` and
//! `<meta name="allio:group">` (`hidden` leaves a page out).

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread as _, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
  NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSImage, NSMenu,
  NSMenuItem, NSMenuItemBadge, NSStatusItem,
};
use objc2_foundation::NSString;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::{backstage::Backstages, pointer::Pointers, shaders::Shaders, AppState};

/// The parts of allio the status menu reports on.
pub struct Services {
  pub shaders: std::sync::Arc<Shaders>,
  pub pointers: std::sync::Arc<Pointers>,
  pub backstages: std::sync::Arc<Backstages>,
}

/// Group order in the launcher; anything else comes after, under its own name.
const GROUPS: [&str; 3] = ["Accessibility", "Shaders", "Pointer & windows"];

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
  fn CGPreflightScreenCaptureAccess() -> bool;
  fn CGRequestScreenCaptureAccess() -> bool;
}

/// What a menu item does.
#[derive(Clone)]
enum Action {
  Overlay(String),
  LoadUrl,
  LoadFile,
  Passthrough,
  ReleasePointer,
  RemoveBackstage,
  Accessibility,
  ScreenRecording,
  Devtools,
  Reload,
  Quit,
}

/// An overlay page, as it describes itself.
struct Overlay {
  file: String,
  title: String,
  description: String,
  group: String,
}

/// Everything a menu shows, read before it is built (off the main thread is fine).
struct Snapshot {
  overlays: Vec<Overlay>,
  current: String,
  passthrough: bool,
  accessibility: bool,
  screen_recording: bool,
  shaders: (usize, usize),
  pointer: bool,
  backstage: Option<allio_display::Frame>,
}

fn snapshot(app: &AppHandle) -> Snapshot {
  let state = app.state::<AppState>();
  let services = app.state::<Services>();
  let current = state.current_overlay.lock().unwrap().clone();
  Snapshot {
    overlays: crate::get_overlay_files()
      .into_iter()
      .map(|file| describe(&file))
      .filter(|o| o.group != "hidden")
      .collect(),
    current,
    passthrough: state
      .clickthrough_enabled
      .load(std::sync::atomic::Ordering::Relaxed),
    accessibility: allio::Allio::has_permissions(),
    screen_recording: unsafe { CGPreflightScreenCaptureAccess() },
    shaders: services.shaders.summary(),
    pointer: services.pointers.active(),
    backstage: services.backstages.frame(),
  }
}

/// Where an overlay's HTML is on disk.
fn overlay_path(file: &str) -> Option<PathBuf> {
  if crate::is_dev_mode() {
    Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src-web/overlays").join(file))
  } else {
    crate::get_dist_directory().map(|d| d.join(file))
  }
}

/// The text between `start` and the next `end` after it.
fn between<'a>(html: &'a str, start: &str, end: &str) -> Option<&'a str> {
  let from = html.find(start)? + start.len();
  let len = html[from..].find(end)?;
  Some(html[from..from + len].trim())
}

fn describe(file: &str) -> Overlay {
  let html = overlay_path(file)
    .and_then(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_default();
  let meta = |name: &str| between(&html, &format!("<meta name=\"{name}\" content=\""), "\"");
  Overlay {
    file: file.to_string(),
    title: between(&html, "<title>", "</title>")
      .unwrap_or_else(|| file.trim_end_matches(".html"))
      .to_string(),
    description: meta("description").unwrap_or_default().to_string(),
    group: meta("allio:group").unwrap_or("Other").to_string(),
  }
}

/// Receives menu item clicks; the item's tag indexes the menu's actions.
struct Handler {
  app: AppHandle,
  actions: Vec<Action>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "AllioMenuTarget"]
  #[ivars = Handler]
  struct Target;

  impl Target {
    #[unsafe(method(pick:))]
    fn pick(&self, item: &NSMenuItem) {
      let handler = self.ivars();
      let Some(action) = usize::try_from(item.tag()).ok().and_then(|i| handler.actions.get(i)) else {
        return;
      };
      let (app, action) = (handler.app.clone(), action.clone());
      // After the menu has closed, outside its tracking loop.
      let _ = handler.app.run_on_main_thread(move || perform(&app, action));
    }
  }
);

thread_local! {
  /// Menu items hold their target weakly: this keeps the open menu's target alive.
  static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
}

fn perform(app: &AppHandle, action: Action) {
  let services = app.state::<Services>();
  match action {
    Action::Overlay(file) => {
      let _ = crate::switch_overlay(app, &file);
    }
    Action::LoadUrl => crate::show_url_dialog(app),
    Action::LoadFile => crate::show_file_dialog(app),
    Action::Passthrough => {
      let _ = crate::toggle_passthrough(app);
    }
    Action::ReleasePointer => services.pointers.release(),
    Action::RemoveBackstage => services.backstages.remove(),
    Action::Accessibility => open_settings("Privacy_Accessibility"),
    Action::ScreenRecording => {
      if !unsafe { CGRequestScreenCaptureAccess() } {
        open_settings("Privacy_ScreenCapture");
      }
    }
    Action::Devtools => {
      if let Ok(w) = crate::get_main_window(app) {
        if w.is_devtools_open() {
          w.close_devtools();
        } else {
          w.open_devtools();
        }
      }
    }
    Action::Reload => {
      if let Ok(w) = crate::get_main_window(app) {
        let _ = w.eval("location.reload()");
      }
    }
    Action::Quit => app.exit(0),
  }
}

/// Opens a pane of Privacy & Security in System Settings.
fn open_settings(pane: &str) {
  let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
  let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Builds a menu, collecting the actions its items trigger.
struct Builder {
  mtm: MainThreadMarker,
  menu: Retained<NSMenu>,
  actions: Vec<Action>,
}

/// An item to add: everything but the title is optional.
#[derive(Default)]
struct Item<'a> {
  title: &'a str,
  subtitle: Option<&'a str>,
  symbol: Option<&'a str>,
  badge: Option<&'a str>,
  checked: bool,
  /// Shown, and also works while the menu is open: (key, modifiers).
  key: Option<(&'a str, NSEventModifierFlags)>,
  action: Option<Action>,
}

impl Builder {
  fn new(mtm: MainThreadMarker) -> Self {
    let menu = NSMenu::new(mtm);
    // Items without an action are information, not disabled commands: show them plainly.
    menu.setAutoenablesItems(false);
    Self {
      mtm,
      menu,
      actions: Vec::new(),
    }
  }

  fn header(&self, title: &str) {
    self
      .menu
      .addItem(&NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), self.mtm));
  }

  fn separator(&self) {
    self.menu.addItem(&NSMenuItem::separatorItem(self.mtm));
  }

  fn item(&mut self, item: Item<'_>) {
    let (key, modifiers) = item.key.unwrap_or(("", NSEventModifierFlags::empty()));
    let ns = unsafe {
      NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(self.mtm),
        &NSString::from_str(item.title),
        item.action.is_some().then_some(sel!(pick:)),
        &NSString::from_str(key),
      )
    };
    ns.setKeyEquivalentModifierMask(modifiers);
    if let Some(subtitle) = item.subtitle.filter(|s| !s.is_empty()) {
      ns.setSubtitle(Some(&NSString::from_str(subtitle)));
    }
    if let Some(symbol) = item.symbol {
      ns.setImage(
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None)
          .as_deref(),
      );
    }
    if let Some(badge) = item.badge {
      let badge = NSMenuItemBadge::initWithString(NSMenuItemBadge::alloc(), &NSString::from_str(badge));
      ns.setBadge(Some(&badge));
    }
    ns.setState(if item.checked {
      NSControlStateValueOn
    } else {
      NSControlStateValueOff
    });
    if let Some(action) = item.action {
      ns.setTag(self.actions.len() as isize);
      self.actions.push(action);
    }
    self.menu.addItem(&ns);
  }

  /// The finished menu, with its items pointed at a target that runs their actions.
  fn finish(self, app: &AppHandle) -> Retained<NSMenu> {
    let target = Target::alloc(self.mtm).set_ivars(Handler {
      app: app.clone(),
      actions: self.actions,
    });
    let target: Retained<Target> = unsafe { msg_send![super(target), init] };
    for item in self.menu.itemArray().iter() {
      if item.action().is_some() {
        unsafe { item.setTarget(Some(&target as &AnyObject)) };
      }
    }
    TARGET.with(|t| *t.borrow_mut() = Some(target));
    self.menu
  }
}

/// Left click: the overlays, by group, each with what it does.
fn launcher(mtm: MainThreadMarker, s: &Snapshot) -> Builder {
  let mut b = Builder::new(mtm);
  let mut groups: Vec<&str> = GROUPS.to_vec();
  for o in &s.overlays {
    if !groups.contains(&o.group.as_str()) {
      groups.push(&o.group);
    }
  }
  for group in groups {
    let members: Vec<&Overlay> = s.overlays.iter().filter(|o| o.group == group).collect();
    if members.is_empty() {
      continue;
    }
    b.header(group);
    for o in members {
      b.item(Item {
        title: &o.title,
        subtitle: Some(&o.description),
        checked: o.file == s.current,
        action: Some(Action::Overlay(o.file.clone())),
        ..Item::default()
      });
    }
  }
  b.separator();
  b.item(Item {
    title: "Load URL…",
    symbol: Some("link"),
    action: Some(Action::LoadUrl),
    ..Item::default()
  });
  b.item(Item {
    title: "Load File…",
    symbol: Some("doc"),
    action: Some(Action::LoadFile),
    ..Item::default()
  });
  b
}

/// Right click: what allio is doing, and a way out of each thing.
fn status(mtm: MainThreadMarker, s: &Snapshot) -> Builder {
  let cmd_shift = NSEventModifierFlags::Command | NSEventModifierFlags::Shift;
  let mut b = Builder::new(mtm);
  let overlay = s
    .overlays
    .iter()
    .find(|o| o.file == s.current)
    .map_or(s.current.as_str(), |o| o.title.as_str());
  b.item(Item {
    title: &format!("allio {}", env!("CARGO_PKG_VERSION")),
    subtitle: Some(&format!("Showing {overlay}")),
    ..Item::default()
  });

  b.header("Permissions");
  for (title, symbol, granted, action) in [
    ("Accessibility", "accessibility", s.accessibility, Action::Accessibility),
    (
      "Screen Recording",
      "record.circle",
      s.screen_recording,
      Action::ScreenRecording,
    ),
  ] {
    b.item(Item {
      title,
      symbol: Some(symbol),
      badge: Some(if granted { "Granted" } else { "Needed" }),
      subtitle: (!granted).then_some("Open System Settings to allow"),
      action: (!granted).then_some(action),
      ..Item::default()
    });
  }

  b.header("Running");
  let (shaders, captures) = s.shaders;
  let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
  b.item(Item {
    title: "Shaders",
    symbol: Some("sparkles"),
    subtitle: Some(&if shaders == 0 {
      "None".to_string()
    } else {
      format!(
        "{}, {}",
        plural(shaders, "shader", "shaders"),
        plural(captures, "capture stream", "capture streams")
      )
    }),
    ..Item::default()
  });
  b.item(Item {
    title: "Pointer field",
    symbol: Some("cursorarrow.motionlines"),
    subtitle: Some(if s.pointer {
      "Moving the real cursor · click to release"
    } else {
      "Off"
    }),
    key: s.pointer.then_some(("\u{1b}", cmd_shift)),
    action: s.pointer.then_some(Action::ReleasePointer),
    ..Item::default()
  });
  b.item(Item {
    title: "Backstage display",
    symbol: Some("display.2"),
    subtitle: Some(&s.backstage.map_or("Off".to_string(), |f| {
      format!("{} × {} at ({}, {}) · click to remove", f.w, f.h, f.x, f.y)
    })),
    action: s.backstage.is_some().then_some(Action::RemoveBackstage),
    ..Item::default()
  });

  b.header("Overlay");
  b.item(Item {
    title: "Passthrough",
    subtitle: Some("Clicks go through the overlay"),
    checked: s.passthrough,
    key: Some(("e", cmd_shift)),
    action: Some(Action::Passthrough),
    ..Item::default()
  });
  b.item(Item {
    title: "Reload",
    symbol: Some("arrow.clockwise"),
    key: Some(("r", NSEventModifierFlags::Command)),
    action: Some(Action::Reload),
    ..Item::default()
  });
  b.item(Item {
    title: "Developer Tools",
    symbol: Some("hammer"),
    key: Some(("i", NSEventModifierFlags::Command | NSEventModifierFlags::Option)),
    action: Some(Action::Devtools),
    ..Item::default()
  });
  b.separator();
  b.item(Item {
    title: "Quit allio",
    key: Some(("q", NSEventModifierFlags::Command)),
    action: Some(Action::Quit),
    ..Item::default()
  });
  b
}

/// Opens `menu` from the status item, as if it were the item's own menu.
fn pop_up(item: &NSStatusItem, menu: &NSMenu, mtm: MainThreadMarker) {
  let Some(button) = item.button(mtm) else {
    return;
  };
  item.setMenu(Some(menu));
  unsafe { button.performClick(None) };
  item.setMenu(None);
}

/// The tray's click handler: left for the launcher, right for status.
pub fn on_event(tray: &TrayIcon, event: TrayIconEvent) {
  let TrayIconEvent::Click {
    button,
    button_state: MouseButtonState::Down,
    ..
  } = event
  else {
    return;
  };
  let left = match button {
    MouseButton::Left => true,
    MouseButton::Right => false,
    MouseButton::Middle => return,
  };
  let app = tray.app_handle().clone();
  let snap = snapshot(&app);
  let _ = tray.with_inner_tray_icon(move |inner| {
    let (Some(mtm), Some(item)) = (MainThreadMarker::new(), inner.ns_status_item()) else {
      return;
    };
    let builder = if left { launcher(mtm, &snap) } else { status(mtm, &snap) };
    let menu = builder.finish(&app);
    pop_up(&item, &menu, mtm);
  });
}
