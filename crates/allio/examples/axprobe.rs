/*!
axprobe — raw accessibility inspection, independent of allio's element model.

Allio reads a fixed set of ~18 attributes and only writes `AXValue` on roles a
static table deems writable. This tool shows what the OS *actually* offers:
every attribute name, its raw CF type, whether the app claims it is settable,
parameterized attributes and actions.

Read-only except `set`, `param` and `perform` (one action, by exact name or unique substring). `set` writes an attribute on the
hovered/focused element, trying each `--value` in turn until one visibly changes
it. `param` calls a parameterized attribute (some, like `AXReplaceRangeWithText`,
have side effects) with each `--param`. Both report attribute diffs.
The opt-in `--enhanced` / `--manual` flags set `AXEnhancedUserInterface` /
`AXManualAccessibility` on the target *app* (the signals VoiceOver / assistive
tech send) and leave them set; relaunch the app to clear.

```text
cargo run -p allio --example axprobe -- focused [--delay 3] [--depth 1]
cargo run -p allio --example axprobe -- hover   [--delay 3] [--depth 2]
cargo run -p allio --example axprobe -- watch   [--delay 3] [--focused] [--secs 20]
cargo run -p allio --example axprobe -- census <app-name|pid> [--max 3000] [--menus]
cargo run -p allio --example axprobe -- set --attr AXValue --value '"rgb 1 0 0 1"' --value '{"cgcolor":[1,0,0,1]}'
cargo run -p allio --example axprobe -- param --attr AXReplaceRangeWithText --param '[{"range":[0,0]},"x"]'
```

Values are JSON, encoded to CF: primitives directly; `{"cgcolor":[r,g,b,a]}`,
`{"range":[loc,len]}`, `{"point":[x,y]}`, `{"size":[w,h]}`, `{"rect":[x,y,w,h]}`;
arrays → CFArray; other objects → CFDictionary.

`focused`/`hover` print a full-detail JSON subtree (plus the ancestor chain).
`watch` registers every common notification on the element and its app, polls
the element's attributes, and streams JSON lines for each notification / diff.
`census` walks an app's windows and aggregates, per platform role, which
attributes appear, their value types, how often they're settable, and which
nodes are only reachable through attributes other than `AXChildren`.

The terminal running this needs Accessibility permission.
*/

#![allow(
  unsafe_code,
  missing_docs,
  unreachable_pub,
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::indexing_slicing,
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::wildcard_enum_match_arm
)]

use objc2_application_services::{
  AXError, AXIsProcessTrusted, AXObserver, AXUIElement, AXValue, AXValueType,
};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFArray, CFAttributedString, CFBoolean, CFDictionary, CFNumber, CFRange,
  CFData, CFRetained, CFRunLoop, CFString, CFType, CFURL, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};
use serde_json::{json, Map, Value as J};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::ptr::{null, NonNull};

extern "C" {
  fn CFCopyDescription(cf: *const c_void) -> *const CFString;
  fn CFHash(cf: *const c_void) -> usize;
}

const STR_CAP: usize = 160;
/// How many instances of each (role, attribute) pair get a settability check in census mode.
const SETTABLE_SAMPLES: u32 = 4;

// ---------------------------------------------------------------------------
// Raw AX helpers
// ---------------------------------------------------------------------------

type El = CFRetained<AXUIElement>;

fn names(f: impl FnOnce(NonNull<*const CFArray>) -> AXError) -> Vec<String> {
  let mut p: *const CFArray = null();
  if f(NonNull::from(&mut p)) != AXError::Success || p.is_null() {
    return Vec::new();
  }
  let arr: CFRetained<CFArray<CFString>> =
    unsafe { CFRetained::from_raw(NonNull::new_unchecked(p.cast::<CFArray<CFString>>().cast_mut())) };
  (0..arr.len()).filter_map(|i| arr.get(i)).map(|s| s.to_string()).collect()
}

fn attr_names(el: &AXUIElement) -> Vec<String> {
  names(|p| unsafe { el.copy_attribute_names(p) })
}
fn param_attr_names(el: &AXUIElement) -> Vec<String> {
  names(|p| unsafe { el.copy_parameterized_attribute_names(p) })
}
fn action_names(el: &AXUIElement) -> Vec<String> {
  names(|p| unsafe { el.copy_action_names(p) })
}

fn attr(el: &AXUIElement, name: &str) -> Result<CFRetained<CFType>, AXError> {
  let mut p: *const CFType = null();
  let err = unsafe { el.copy_attribute_value(&CFString::from_str(name), NonNull::from(&mut p)) };
  if err != AXError::Success {
    return Err(err);
  }
  if p.is_null() {
    return Err(AXError::NoValue);
  }
  Ok(unsafe { CFRetained::from_raw(NonNull::new_unchecked(p.cast_mut())) })
}

fn settable(el: &AXUIElement, name: &str) -> Option<bool> {
  let mut b: u8 = 0;
  let err = unsafe { el.is_attribute_settable(&CFString::from_str(name), NonNull::from(&mut b)) };
  (err == AXError::Success).then_some(b != 0)
}

fn attr_str(el: &AXUIElement, name: &str) -> Option<String> {
  attr(el, name)
    .ok()?
    .downcast_ref::<CFString>()
    .map(ToString::to_string)
    .filter(|s| !s.is_empty())
}

fn attr_elements(el: &AXUIElement, name: &str) -> Vec<El> {
  let Ok(v) = attr(el, name) else { return Vec::new() };
  let Ok(arr) = v.downcast::<CFArray>() else { return Vec::new() };
  let arr: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(arr) };
  (0..arr.len())
    .filter_map(|i| arr.get(i))
    .filter_map(|v| v.downcast::<AXUIElement>().ok())
    .collect()
}

fn platform_role(el: &AXUIElement) -> String {
  let role = attr_str(el, "AXRole").unwrap_or_else(|| "?".into());
  match attr_str(el, "AXSubrole") {
    Some(sr) => format!("{role}/{sr}"),
    None => role,
  }
}

fn el_key(el: &AXUIElement) -> usize {
  unsafe { CFHash((el as *const AXUIElement).cast()) }
}

fn cap(s: String) -> String {
  if s.chars().count() > STR_CAP {
    format!("{}…", s.chars().take(STR_CAP).collect::<String>())
  } else {
    s
  }
}

fn cf_description(v: &CFType) -> String {
  unsafe {
    let p = CFCopyDescription((v as *const CFType).cast());
    if p.is_null() {
      return "?".into();
    }
    CFRetained::from_raw(NonNull::new_unchecked(p.cast_mut())).to_string()
  }
}

/// One-line summary of an element reference (no recursion).
fn el_summary(el: &AXUIElement) -> String {
  let mut s = platform_role(el);
  if let Some(t) = attr_str(el, "AXTitle").or_else(|| attr_str(el, "AXDescription")) {
    s.push_str(&format!(" {:?}", cap(t)));
  }
  s
}

/// Classify a raw CF value: (type tag, JSON rendering).
fn describe(v: &CFType, depth: u8) -> (String, J) {
  if let Some(s) = v.downcast_ref::<CFString>() {
    return ("string".into(), J::String(cap(s.to_string())));
  }
  if let Some(b) = v.downcast_ref::<CFBoolean>() {
    return ("bool".into(), J::Bool(b.as_bool()));
  }
  if let Some(n) = v.downcast_ref::<CFNumber>() {
    if n.is_float_type() {
      return ("float".into(), json!(n.as_f64()));
    }
    return ("int".into(), json!(n.as_i64()));
  }
  if let Some(a) = v.downcast_ref::<CFAttributedString>() {
    let text = a.string().map(|s| s.to_string()).unwrap_or_default();
    if depth > 0 {
      return ("attributed_string".into(), J::String(cap(text)));
    }
    // Style runs: each run's range + its attribute dictionary (fonts, colors, links…).
    let mut runs = Vec::new();
    let (mut loc, len) = (0isize, a.length());
    while loc < len && runs.len() < 32 {
      let mut r = CFRange { location: 0, length: 0 };
      let attrs = unsafe { a.attributes(loc, &raw mut r) };
      let attrs = attrs.map_or(J::Null, |d| describe(&d, 1).1);
      runs.push(json!({ "range": [r.location, r.length], "attributes": attrs }));
      if r.length <= 0 {
        break;
      }
      loc = r.location + r.length;
    }
    return ("attributed_string".into(), json!({ "text": cap(text), "runs": runs }));
  }
  if let Some(d) = v.downcast_ref::<CFData>() {
    let bytes = d.to_vec();
    return match String::from_utf8(bytes.clone()) {
      Ok(text) => ("data:utf8".into(), J::String(text.chars().take(4000).collect())),
      Err(_) => ("data".into(), json!({ "len": bytes.len() })),
    };
  }
  if let Some(u) = v.downcast_ref::<CFURL>() {
    let s = u.string().to_string();
    return ("url".into(), J::String(cap(s)));
  }
  if let Some(el) = v.downcast_ref::<AXUIElement>() {
    let summary = if depth == 0 { el_summary(el) } else { platform_role(el) };
    return ("element".into(), J::String(summary));
  }
  if let Some(ax) = v.downcast_ref::<AXValue>() {
    return describe_axvalue(ax);
  }
  if let Some(dict) = v.downcast_ref::<CFDictionary>() {
    let mut o = Map::new();
    for (k, val) in dict_entries(dict) {
      let key = k.downcast_ref::<CFString>().map_or_else(|| cf_description(&k), ToString::to_string);
      o.insert(key, if depth < 3 { describe(&val, depth + 1).1 } else { J::Null });
    }
    return ("dict".into(), J::Object(o));
  }
  if let Some(arr) = v.downcast_ref::<CFArray>() {
    let arr: &CFArray<CFType> = unsafe { &*(arr as *const CFArray).cast() };
    let len = arr.len();
    let shown = match depth {
      0 => 6,
      1 => 2,
      _ => 0,
    };
    let items: Vec<(String, J)> =
      (0..len.min(shown)).filter_map(|i| arr.get(i)).map(|x| describe(&x, depth + 1)).collect();
    let elem_type = items.first().map_or_else(|| "?".to_string(), |(t, _)| t.clone());
    return (
      format!("array<{elem_type}>"),
      json!({ "len": len, "items": items.into_iter().map(|(_, j)| j).collect::<Vec<_>>() }),
    );
  }
  // Dates, dictionaries, data, etc. — fall back to CF's own description.
  let desc = cf_description(v);
  let tag = desc
    .split(|c: char| c == ' ' || c == '[' || c == '{' || c == '<')
    .find(|s| !s.is_empty())
    .unwrap_or("other")
    .to_string();
  (format!("cf:{tag}"), J::String(cap(desc)))
}

fn dict_entries(dict: &CFDictionary) -> Vec<(CFRetained<CFType>, CFRetained<CFType>)> {
  let n = dict.count() as usize;
  let mut keys: Vec<*const c_void> = vec![null(); n];
  let mut vals: Vec<*const c_void> = vec![null(); n];
  unsafe { dict.keys_and_values(keys.as_mut_ptr(), vals.as_mut_ptr()) };
  keys
    .into_iter()
    .zip(vals)
    .filter(|(k, v)| !k.is_null() && !v.is_null())
    .map(|(k, v)| unsafe {
      (
        CFRetained::retain(NonNull::new_unchecked(k.cast_mut().cast::<CFType>())),
        CFRetained::retain(NonNull::new_unchecked(v.cast_mut().cast::<CFType>())),
      )
    })
    .collect()
}

/// Every element reference reachable inside a value (direct, in arrays, in dict values).
fn element_refs(v: &CFType, out: &mut Vec<El>) {
  if let Some(el) = v.downcast_ref::<AXUIElement>() {
    out.push(unsafe { CFRetained::retain(NonNull::from(el)) });
  } else if let Some(arr) = v.downcast_ref::<CFArray>() {
    let arr: &CFArray<CFType> = unsafe { &*(arr as *const CFArray).cast() };
    for x in (0..arr.len()).filter_map(|i| arr.get(i)) {
      element_refs(&x, out);
    }
  } else if let Some(dict) = v.downcast_ref::<CFDictionary>() {
    for (_, x) in dict_entries(dict) {
      element_refs(&x, out);
    }
  }
}

/// Attributes pointing "up" or sideways to chrome — not content discovery.
const UP_LINKS: &[&str] = &[
  "AXParent",
  "AXWindow",
  "AXTopLevelUIElement",
  "AXFocusedWindow",
  "AXMainWindow",
  "AXFocusedUIElement",
  "AXExtrasMenuBar",
];

fn describe_axvalue(ax: &AXValue) -> (String, J) {
  unsafe {
    let t = ax.r#type();
    if t == AXValueType::CGPoint {
      let mut p = CGPoint { x: 0.0, y: 0.0 };
      ax.value(t, NonNull::from(&mut p).cast());
      return ("point".into(), json!([p.x, p.y]));
    }
    if t == AXValueType::CGSize {
      let mut s = CGSize { width: 0.0, height: 0.0 };
      ax.value(t, NonNull::from(&mut s).cast());
      return ("size".into(), json!([s.width, s.height]));
    }
    if t == AXValueType::CGRect {
      let mut r = CGRect::default();
      ax.value(t, NonNull::from(&mut r).cast());
      return ("rect".into(), json!([r.origin.x, r.origin.y, r.size.width, r.size.height]));
    }
    if t == AXValueType::CFRange {
      let mut r = CFRange { location: 0, length: 0 };
      ax.value(t, NonNull::from(&mut r).cast());
      return ("range".into(), json!([r.location, r.length]));
    }
    if t == AXValueType::AXError {
      let mut e = AXError::Success;
      ax.value(t, NonNull::from(&mut e).cast());
      return ("ax_error".into(), json!(format!("{e:?}")));
    }
  }
  ("ax_value:illegal".into(), J::Null)
}

fn err_str(e: AXError) -> String {
  format!("{e:?}")
}

// ---------------------------------------------------------------------------
// Full-detail node dump
// ---------------------------------------------------------------------------

/// Children are not inlined as attributes; they're recursed separately.
const STRUCTURAL: &[&str] = &["AXChildren", "AXChildrenInNavigationOrder"];

fn dump_node(el: &AXUIElement, depth: usize, max_depth: usize, seen: &mut HashSet<usize>) -> J {
  let mut attrs = Map::new();
  for name in attr_names(el) {
    let entry = match attr(el, &name) {
      Ok(v) => {
        let (ty, val) = if STRUCTURAL.contains(&name.as_str()) {
          let len = v.downcast_ref::<CFArray>().map_or(0, CFArray::len);
          (format!("array<element>"), json!({ "len": len }))
        } else {
          describe(&v, 0)
        };
        json!({ "type": ty, "value": val, "settable": settable(el, &name) })
      }
      Err(e) => json!({ "error": err_str(e), "settable": settable(el, &name) }),
    };
    attrs.insert(name, entry);
  }

  let mut node = Map::new();
  node.insert("role".into(), J::String(platform_role(el)));
  node.insert("attributes".into(), J::Object(attrs));
  let params = param_attr_names(el);
  if !params.is_empty() {
    node.insert("parameterized".into(), json!(params));
  }
  node.insert("actions".into(), json!(action_names(el)));

  let children = attr_elements(el, "AXChildren");
  if depth < max_depth && !children.is_empty() {
    let mut kids = Vec::with_capacity(children.len());
    for c in &children {
      if seen.insert(el_key(c)) {
        kids.push(dump_node(c, depth + 1, max_depth, seen));
      }
    }
    node.insert("children".into(), J::Array(kids));
  } else if !children.is_empty() {
    node.insert("children_truncated".into(), json!(children.len()));
  }
  J::Object(node)
}

fn ancestors(el: &AXUIElement) -> Vec<String> {
  let mut chain = Vec::new();
  let mut cur = attr(el, "AXParent").ok().and_then(|v| v.downcast::<AXUIElement>().ok());
  while let Some(p) = cur {
    chain.push(el_summary(&p));
    if chain.len() > 64 {
      break;
    }
    cur = attr(&p, "AXParent").ok().and_then(|v| v.downcast::<AXUIElement>().ok());
  }
  chain.reverse();
  chain
}

// ---------------------------------------------------------------------------
// Census: aggregate per-role capability matrix
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AttrStats {
  present: u32,
  errors: BTreeMap<String, u32>,
  types: BTreeMap<String, u32>,
  settable_checked: u32,
  settable_true: u32,
  examples: Vec<J>,
}

#[derive(Default)]
struct RoleStats {
  count: u32,
  /// Nodes of this role not reachable purely through `AXChildren` edges, by the attribute that first exposed them.
  hidden_via: BTreeMap<String, u32>,
  attrs: BTreeMap<String, AttrStats>,
  params: BTreeMap<String, u32>,
  actions: BTreeMap<String, u32>,
}

fn census(app: &AXUIElement, max_nodes: usize, include_menus: bool) -> J {
  let mut roles: BTreeMap<String, RoleStats> = BTreeMap::new();
  let mut seen = HashSet::new();
  // (element, hidden_via): None = reached via AXChildren edges only.
  // Child edges are drained before cross-reference edges so "hidden" really means
  // "not reachable through the children tree we'd normally walk".
  let mut queue: VecDeque<(El, Option<String>)> =
    attr_elements(app, "AXWindows").into_iter().map(|w| (w, None)).collect();
  let mut refs: VecDeque<(El, Option<String>)> = VecDeque::new();
  if include_menus {
    if let Ok(mb) = attr(app, "AXMenuBar").and_then(|v| v.downcast::<AXUIElement>().map_err(|_| AXError::Failure)) {
      queue.push_back((mb, None));
    }
  }
  let mut visited = 0usize;

  while let Some((el, hidden_via)) = queue.pop_front().or_else(|| refs.pop_front()) {
    if visited >= max_nodes {
      break;
    }
    if !seen.insert(el_key(&el)) {
      continue;
    }
    visited += 1;
    if visited % 250 == 0 {
      eprintln!("  …{visited} nodes");
    }

    let role = platform_role(&el);
    let rs = roles.entry(role).or_default();
    rs.count += 1;
    if let Some(via) = &hidden_via {
      *rs.hidden_via.entry(via.clone()).or_default() += 1;
    }
    for name in attr_names(&el) {
      let st = rs.attrs.entry(name.clone()).or_default();
      st.present += 1;
      if !STRUCTURAL.contains(&name.as_str()) {
        match attr(&el, &name) {
          Ok(v) => {
            if !UP_LINKS.contains(&name.as_str()) {
              let mut found = Vec::new();
              element_refs(&v, &mut found);
              for r in found {
                if !seen.contains(&el_key(&r)) {
                  refs.push_back((r, Some(hidden_via.clone().unwrap_or_else(|| name.clone()))));
                }
              }
            }
            let (ty, val) = describe(&v, 1);
            *st.types.entry(ty).or_default() += 1;
            if st.examples.len() < 3 && !val.is_null() && val != json!("") && !st.examples.contains(&val) {
              st.examples.push(val);
            }
          }
          Err(e) => *st.errors.entry(err_str(e)).or_default() += 1,
        }
      }
      if st.settable_checked < SETTABLE_SAMPLES {
        st.settable_checked += 1;
        if settable(&el, &name) == Some(true) {
          st.settable_true += 1;
        }
      }
    }
    for p in param_attr_names(&el) {
      *rs.params.entry(p).or_default() += 1;
    }
    for a in action_names(&el) {
      *rs.actions.entry(a).or_default() += 1;
    }
    queue.extend(attr_elements(&el, "AXChildren").into_iter().map(|c| (c, hidden_via.clone())));
  }

  let roles_json: Map<String, J> = roles
    .into_iter()
    .map(|(role, rs)| {
      let attrs: Map<String, J> = rs
        .attrs
        .into_iter()
        .map(|(name, st)| {
          let mut o = Map::new();
          o.insert("present".into(), json!(st.present));
          if !st.types.is_empty() {
            o.insert("types".into(), json!(st.types));
          }
          if !st.errors.is_empty() {
            o.insert("errors".into(), json!(st.errors));
          }
          o.insert("settable".into(), json!(format!("{}/{}", st.settable_true, st.settable_checked)));
          if !st.examples.is_empty() {
            o.insert("examples".into(), J::Array(st.examples));
          }
          (name, J::Object(o))
        })
        .collect();
      let mut o = Map::new();
      o.insert("count".into(), json!(rs.count));
      if !rs.hidden_via.is_empty() {
        o.insert("hidden_via".into(), json!(rs.hidden_via));
      }
      o.insert("actions".into(), json!(rs.actions));
      if !rs.params.is_empty() {
        o.insert("parameterized".into(), json!(rs.params));
      }
      o.insert("attributes".into(), J::Object(attrs));
      (role, J::Object(o))
    })
    .collect();

  json!({
    "nodes_visited": visited,
    "truncated": !queue.is_empty() || !refs.is_empty(),
    "roles": roles_json,
  })
}

// ---------------------------------------------------------------------------
// Watch: which notifications fire, and what changes under polling
// ---------------------------------------------------------------------------

const NOTIFICATIONS: &[&str] = &[
  "AXValueChanged",
  "AXTitleChanged",
  "AXUIElementDestroyed",
  "AXCreated",
  "AXLayoutChanged",
  "AXMoved",
  "AXResized",
  "AXFocusedUIElementChanged",
  "AXSelectedChildrenChanged",
  "AXSelectedChildrenMoved",
  "AXSelectedTextChanged",
  "AXSelectedRowsChanged",
  "AXSelectedColumnsChanged",
  "AXSelectedCellsChanged",
  "AXRowCountChanged",
  "AXRowExpanded",
  "AXRowCollapsed",
  "AXExpandedChanged",
  "AXElementBusyChanged",
  "AXMenuItemSelected",
  "AXAnnouncementRequested",
  "AXLiveRegionChanged",
  "AXUnitsChanged",
  "AXWindowCreated",
  "AXSheetCreated",
];

static WATCH_TARGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static WATCH_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn ms() -> u128 {
  WATCH_START.get().map_or(0, |t| t.elapsed().as_millis())
}

unsafe extern "C-unwind" fn watch_callback(
  _obs: NonNull<AXObserver>,
  el: NonNull<AXUIElement>,
  notif: NonNull<CFString>,
  refcon: *mut c_void,
) {
  let el = unsafe { el.as_ref() };
  let scope = if refcon.is_null() { "app" } else { "element" };
  let is_target = WATCH_TARGET.get() == Some(&el_key(el));
  let line = json!({
    "t_ms": ms(),
    "kind": "notification",
    "registered_on": scope,
    "notification": unsafe { notif.as_ref() }.to_string(),
    "is_target": is_target,
    "source": el_summary(el),
  });
  println!("{line}");
}

fn snapshot_attrs(el: &AXUIElement) -> BTreeMap<String, J> {
  attr_names(el)
    .into_iter()
    .filter(|n| !STRUCTURAL.contains(&n.as_str()))
    .map(|n| {
      let v = attr(el, &n).map_or_else(|e| json!({ "error": err_str(e) }), |v| describe(&v, 0).1);
      (n, v)
    })
    .collect()
}

fn watch(el: &AXUIElement, secs: u64, poll_ms: u64) {
  let mut pid = 0;
  unsafe { el.pid(NonNull::from(&mut pid)) };
  let _ = WATCH_TARGET.set(el_key(el));
  let _ = WATCH_START.set(std::time::Instant::now());

  let observer = unsafe {
    let mut p: *mut AXObserver = std::ptr::null_mut();
    if AXObserver::create(pid, Some(watch_callback), NonNull::from(&mut p)) != AXError::Success || p.is_null() {
      eprintln!("AXObserverCreate failed");
      std::process::exit(1);
    }
    CFRetained::from_raw(NonNull::new_unchecked(p))
  };
  let app = unsafe { AXUIElement::new_application(pid) };
  let mut registered = Map::new();
  for n in NOTIFICATIONS {
    let name = CFString::from_str(n);
    // Non-null refcon marks element-level registrations.
    let on_el = unsafe { observer.add_notification(el, &name, NonNull::<u8>::dangling().as_ptr().cast()) };
    let on_app = unsafe { observer.add_notification(&app, &name, null::<c_void>().cast_mut()) };
    registered.insert((*n).to_string(), json!({ "element": err_str(on_el), "app": err_str(on_app) }));
  }
  unsafe {
    CFRunLoop::current()
      .expect("run loop")
      .add_source(Some(&observer.run_loop_source()), kCFRunLoopDefaultMode);
  }

  let mut last = snapshot_attrs(el);
  println!(
    "{}",
    json!({ "kind": "start", "target": el_summary(el), "pid": pid, "registrations": registered, "attributes": last })
  );
  eprintln!("  watching for {secs}s — interact with the element now…");

  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
  while std::time::Instant::now() < deadline {
    unsafe { CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, poll_ms as f64 / 1000.0, false) };
    let now = snapshot_attrs(el);
    for (k, v) in &now {
      if last.get(k) != Some(v) && !matches!(k.as_str(), "AXFrame" | "AXPosition" | "AXSize") {
        println!(
          "{}",
          json!({ "t_ms": ms(), "kind": "poll_diff", "attribute": k, "from": last.get(k), "to": v })
        );
      }
    }
    last = now;
  }
  println!("{}", json!({ "kind": "end", "t_ms": ms() }));
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn flag<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
  args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok())
}

fn has(args: &[String], name: &str) -> bool {
  args.iter().any(|a| a == name)
}

fn resolve_pid(target: &str) -> Option<i32> {
  if let Ok(pid) = target.parse() {
    return Some(pid);
  }
  let out = std::process::Command::new("pgrep").args(["-x", "-i", target]).output().ok()?;
  String::from_utf8_lossy(&out.stdout).lines().next()?.trim().parse().ok()
}

fn mouse() -> (f32, f32) {
  let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
  let ev = CGEvent::new(src.as_deref()).expect("CGEvent");
  let p = CGEvent::location(Some(&ev));
  (p.x as f32, p.y as f32)
}

fn countdown(secs: u64) {
  for i in (1..=secs).rev() {
    eprintln!("  capturing in {i}…");
    std::thread::sleep(std::time::Duration::from_secs(1));
  }
}

fn pid_of(el: &AXUIElement) -> i32 {
  let mut pid = 0;
  unsafe { el.pid(NonNull::from(&mut pid)) };
  pid
}

fn target_info(el: &AXUIElement) -> J {
  let pid = pid_of(el);
  let app = std::process::Command::new("ps")
    .args(["-o", "comm=", "-p", &pid.to_string()])
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().rsplit('/').next().unwrap_or("").to_string())
    .unwrap_or_default();
  json!({ "pid": pid, "app": app })
}

/// Opt-in "an assistive technology is present" signals. Some apps (notably iWork)
/// only build parts of their tree when these are set. Returns what was done, for the report.
/// Values are left as set: restoring would tear the tree down before the user can inspect it.
fn assistive_signals(pid: i32, args: &[String]) -> J {
  let app = unsafe { AXUIElement::new_application(pid) };
  let mut out = Map::new();
  for (flag_name, attr_name) in [("--enhanced", "AXEnhancedUserInterface"), ("--manual", "AXManualAccessibility")] {
    if !has(args, flag_name) {
      continue;
    }
    let before = attr(&app, attr_name).ok().map(|v| describe(&v, 0).1);
    let err = unsafe { app.set_attribute_value(&CFString::from_str(attr_name), CFBoolean::new(true)) };
    out.insert(attr_name.into(), json!({ "before": before, "set": err_str(err) }));
  }
  if !out.is_empty() {
    // Give the app a moment to materialise its tree.
    std::thread::sleep(std::time::Duration::from_millis(1500));
  }
  J::Object(out)
}

fn hit_test(system: &AXUIElement) -> Option<El> {
  let (x, y) = mouse();
  let mut p: *const AXUIElement = null();
  let err = unsafe { system.copy_element_at_position(x, y, NonNull::from(&mut p)) };
  (err == AXError::Success && !p.is_null()).then(|| unsafe { CFRetained::from_raw(NonNull::new_unchecked(p.cast_mut())) })
}

fn pick_target(system: &AXUIElement, args: &[String], focused: bool) -> (El, J) {
  countdown(flag(args, "--delay").unwrap_or(3));
  let find = || {
    if focused {
      attr(system, "AXFocusedUIElement").ok().and_then(|v| v.downcast::<AXUIElement>().ok())
    } else {
      hit_test(system)
    }
  };
  let Some(mut el) = find() else {
    eprintln!("No element found.");
    std::process::exit(1);
  };
  let signals = assistive_signals(pid_of(&el), args);
  if signals.as_object().is_some_and(|o| !o.is_empty()) {
    if let Some(again) = find() {
      el = again;
    }
  }
  (el, signals)
}

extern "C" {
  fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
}

fn cf_equal(a: &AXUIElement, b: &AXUIElement) -> bool {
  unsafe { CFEqual((a as *const AXUIElement).cast(), (b as *const AXUIElement).cast()) != 0 }
}

/// Does asking the app for "the same" element twice give back an equal handle?
/// Allio dedups elements by CFHash/CFEqual, so unstable identity means churn.
fn identity_check(system: &AXUIElement, el: &AXUIElement, rehit: bool) -> J {
  let parent = |e: &AXUIElement| attr(e, "AXParent").ok().and_then(|v| v.downcast::<AXUIElement>().ok());
  let mut o = Map::new();
  if rehit {
    o.insert("rehit_equal".into(), json!(hit_test(system).map(|e| cf_equal(&e, el))));
  }
  if let (Some(p1), Some(p2)) = (parent(el), parent(el)) {
    o.insert("parent_twice_equal".into(), json!(cf_equal(&p1, &p2)));
    // Is this element found among its parent's children (by equality)?
    let kids = attr_elements(&p1, "AXChildren");
    o.insert("found_in_parent_children".into(), json!(kids.iter().any(|k| cf_equal(k, el))));
    let kids2 = attr_elements(&p1, "AXChildren");
    let stable = kids.len() == kids2.len() && kids.iter().zip(&kids2).all(|(a, b)| cf_equal(a, b));
    o.insert("parent_children_twice_equal".into(), json!(stable));
  }
  J::Object(o)
}

extern "C" {
  fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> *const c_void;
}

fn ax_value<T>(t: AXValueType, mut v: T) -> Option<CFRetained<CFType>> {
  unsafe { AXValue::new(t, NonNull::from(&mut v).cast()) }.map(Into::into)
}

fn nums(v: &J) -> Option<Vec<f64>> {
  v.as_array()?.iter().map(J::as_f64).collect()
}

/// Encode JSON as a CF value. Primitives map directly (`--cf` forces the CF type of a
/// top-level primitive). Typed wrappers: `{"cgcolor":[r,g,b,a]}`, `{"range":[loc,len]}`,
/// `{"point":[x,y]}`, `{"size":[w,h]}`, `{"rect":[x,y,w,h]}`,
/// `{"attributed":{"text":"..","attrs":{..}}}`. Arrays → CFArray, other objects →
/// CFDictionary with string keys.
fn encode(v: &J, cf: Option<&str>) -> Option<CFRetained<CFType>> {
  let boolean = |b: bool| -> CFRetained<CFType> { unsafe { CFRetained::retain(NonNull::from(CFBoolean::new(b))) }.into() };
  Some(match (cf, v) {
    (_, J::Object(o)) if o.len() == 1 => {
      let (k, inner) = o.iter().next()?;
      let n = nums(inner);
      match (k.as_str(), n.as_deref()) {
        ("cgcolor", Some(&[r, g, b, a])) => unsafe {
          let p = CGColorCreateSRGB(r, g, b, a);
          CFRetained::from_raw(NonNull::new(p.cast_mut().cast::<CFType>())?)
        },
        ("range", Some(&[loc, len])) => ax_value(AXValueType::CFRange, CFRange { location: loc as isize, length: len as isize })?,
        ("point", Some(&[x, y])) => ax_value(AXValueType::CGPoint, CGPoint { x, y })?,
        ("size", Some(&[w, h])) => ax_value(AXValueType::CGSize, CGSize { width: w, height: h })?,
        ("rect", Some(&[x, y, w, h])) => {
          ax_value(AXValueType::CGRect, CGRect { origin: CGPoint { x, y }, size: CGSize { width: w, height: h } })?
        }
        ("attributed", _) => {
          // {"attributed": {"text": "...", "attrs": {"AXForegroundColor": {"cgcolor": [...]}, ...}}}
          let text = CFString::from_str(inner.get("text")?.as_str()?);
          let attrs = match inner.get("attrs") {
            Some(J::Object(a)) => Some(encode_dict(a)?),
            _ => None,
          };
          let dict = attrs.as_deref().and_then(|d| d.downcast_ref::<CFDictionary>());
          let a = unsafe { CFAttributedString::new(None, Some(&text), dict) }?;
          unsafe { CFRetained::cast_unchecked::<CFType>(a) }
        }
        _ => encode_dict(o)?,
      }
    }
    (_, J::Object(o)) => encode_dict(o)?,
    (_, J::Array(items)) => {
      let encoded: Vec<CFRetained<CFType>> = items.iter().map(|x| encode(x, None)).collect::<Option<_>>()?;
      unsafe { CFRetained::cast_unchecked::<CFType>(CFArray::<CFType>::from_retained_objects(&encoded)) }
    }
    (None | Some("bool"), J::Bool(b)) => boolean(*b),
    (Some("int"), J::Bool(b)) => CFNumber::new_i32(i32::from(*b)).into(),
    (Some("int"), J::Number(n)) => CFNumber::new_i64(n.as_f64()? as i64).into(),
    (Some("float"), J::Number(n)) => CFNumber::new_f64(n.as_f64()?).into(),
    (None, J::Number(n)) if n.is_i64() => CFNumber::new_i64(n.as_i64()?).into(),
    (None, J::Number(n)) => CFNumber::new_f64(n.as_f64()?).into(),
    (None | Some("string"), J::String(st)) => CFString::from_str(st).into(),
    _ => return None,
  })
}

fn encode_dict(o: &Map<String, J>) -> Option<CFRetained<CFType>> {
  let keys: Vec<CFRetained<CFString>> = o.keys().map(|k| CFString::from_str(k)).collect();
  let vals: Vec<CFRetained<CFType>> = o.values().map(|v| encode(v, None)).collect::<Option<_>>()?;
  let kr: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
  let vr: Vec<&CFType> = vals.iter().map(|v| &**v).collect();
  Some(unsafe { CFRetained::cast_unchecked::<CFType>(CFDictionary::<CFString, CFType>::from_slices(&kr, &vr)) })
}

fn all_flags(args: &[String], name: &str) -> Vec<String> {
  args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}

/// Parse each `--value`/`--param` as JSON (falling back to a bare string) and encode it.
fn encoded_values(args: &[String], name: &str) -> Vec<(J, CFRetained<CFType>)> {
  let cf = flag::<String>(args, "--cf");
  all_flags(args, name)
    .into_iter()
    .map(|raw| {
      let parsed: J = serde_json::from_str(&raw).unwrap_or(J::String(raw));
      let Some(v) = encode(&parsed, cf.as_deref()) else {
        eprintln!("could not encode {parsed} (cf type {cf:?})");
        std::process::exit(2);
      };
      (parsed, v)
    })
    .collect()
}

fn diff(before: &BTreeMap<String, J>, after: &BTreeMap<String, J>) -> J {
  let mut o = Map::new();
  for (k, v) in after {
    if before.get(k) != Some(v) && !matches!(k.as_str(), "AXFrame" | "AXPosition" | "AXSize") {
      o.insert(k.clone(), json!({ "from": before.get(k), "to": v }));
    }
  }
  J::Object(o)
}

/// Try each value in turn; stop at the first that visibly changes the element.
/// `then`: an action (e.g. `AXConfirm`) performed after each write, for controls that
/// only commit on confirm.
fn set_lab(el: &AXUIElement, name: &str, values: &[(J, CFRetained<CFType>)], then: Option<&str>) -> J {
  let mut attempts = Vec::new();
  for (raw, value) in values {
    let before = snapshot_attrs(el);
    let err = unsafe { el.set_attribute_value(&CFString::from_str(name), value) };
    let then_result = then.map(|a| err_str(unsafe { el.perform_action(&CFString::from_str(a)) }));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let changes = diff(&before, &snapshot_attrs(el));
    let changed = changes.as_object().is_some_and(|o| !o.is_empty());
    attempts.push(json!({ "value": raw, "cf": describe(value, 0).0, "set_result": err_str(err), "then": then, "then_result": then_result, "changes": changes }));
    if changed {
      break;
    }
  }
  json!({ "target": el_summary(el), "attribute": name, "settable": settable(el, name), "attempts": attempts })
}

/// Call a parameterized attribute with each parameter; report the result and any
/// side effects on the element's own attributes. Stops at the first with side effects.
fn param_lab(el: &AXUIElement, name: &str, params: &[(J, CFRetained<CFType>)]) -> J {
  let mut attempts = Vec::new();
  for (raw, param) in params {
    let before = snapshot_attrs(el);
    let mut p: *const CFType = null();
    let err = unsafe { el.copy_parameterized_attribute_value(&CFString::from_str(name), param, NonNull::from(&mut p)) };
    let result = (!p.is_null()).then(|| {
      let v = unsafe { CFRetained::from_raw(NonNull::new_unchecked(p.cast_mut())) };
      let (ty, val) = describe(&v, 0);
      json!({ "type": ty, "value": val })
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    let side_effects = diff(&before, &snapshot_attrs(el));
    let acted = side_effects.as_object().is_some_and(|o| !o.is_empty());
    attempts.push(json!({
      "param": raw,
      "cf": describe(param, 0).0,
      "error": err_str(err),
      "result": result,
      "side_effects": side_effects,
    }));
    if acted {
      break;
    }
  }
  json!({ "target": el_summary(el), "parameterized_attribute": name, "attempts": attempts })
}

/// Perform an action (exact name, or a unique substring — custom actions have long
/// names like "Name:Flag\nTarget:0x0\nSelector:(null)") and report attribute diffs.
/// If the hit element lacks it, the nearest ancestor (up to 8 levels) that has it is used.
fn perform_lab(el: &AXUIElement, wanted: &str) -> J {
  let find = |e: &AXUIElement| -> Vec<String> {
    action_names(e).into_iter().filter(|a| a == wanted || a.contains(wanted)).collect()
  };
  let mut target: El = unsafe { CFRetained::retain(NonNull::from(el)) };
  let mut levels_up = 0;
  let mut matches = find(&target);
  while matches.is_empty() && levels_up < 8 {
    let Some(p) = attr(&target, "AXParent").ok().and_then(|v| v.downcast::<AXUIElement>().ok()) else { break };
    target = p;
    levels_up += 1;
    matches = find(&target);
  }
  let [action] = matches.as_slice() else {
    return json!({ "target": el_summary(el), "error": "action not found or ambiguous", "wanted": wanted, "matches": matches, "available_on_hit": action_names(el) });
  };
  let before = snapshot_attrs(&target);
  let err = unsafe { target.perform_action(&CFString::from_str(action)) };
  std::thread::sleep(std::time::Duration::from_millis(500));
  json!({
    "hit": el_summary(el),
    "target": el_summary(&target),
    "levels_up": levels_up,
    "action": action,
    "result": err_str(err),
    "changes": diff(&before, &snapshot_attrs(&target)),
  })
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  if !unsafe { AXIsProcessTrusted() } {
    eprintln!("This process is not trusted for Accessibility. Grant your terminal access in System Settings → Privacy & Security → Accessibility.");
    std::process::exit(1);
  }
  let system = unsafe { AXUIElement::new_system_wide() };
  unsafe { system.set_messaging_timeout(2.0) };

  let out = match args.first().map(String::as_str) {
    Some(mode @ ("focused" | "hover")) => {
      let (el, signals) = pick_target(&system, &args, mode == "focused");
      let depth = flag(&args, "--depth").unwrap_or(if mode == "focused" { 1 } else { 2 });
      let mut info = target_info(&el);
      info["mode"] = json!(mode);
      info["signals"] = signals;
      info["ancestors"] = json!(ancestors(&el));
      info["identity"] = identity_check(&system, &el, mode == "hover");
      info["node"] = dump_node(&el, 0, depth, &mut HashSet::new());
      info
    }
    Some("watch") => {
      let (el, _) = pick_target(&system, &args, has(&args, "--focused"));
      watch(&el, flag(&args, "--secs").unwrap_or(20), flag(&args, "--poll-ms").unwrap_or(150));
      return;
    }
    Some(mode @ ("set" | "param")) => {
      let key = if mode == "set" { "--value" } else { "--param" };
      let Some(name) = flag::<String>(&args, "--attr") else {
        eprintln!("usage: axprobe {mode} --attr NAME {key} '<json>' [{key} ...] [--cf bool|int|float|string] [--focused] [--delay S]");
        std::process::exit(2);
      };
      let values = encoded_values(&args, key);
      if values.is_empty() {
        eprintln!("need at least one {key}");
        std::process::exit(2);
      }
      let (el, _) = pick_target(&system, &args, has(&args, "--focused"));
      if mode == "set" {
        set_lab(&el, &name, &values, flag::<String>(&args, "--then").as_deref())
      } else {
        param_lab(&el, &name, &values)
      }
    }
    Some("perform") => {
      let Some(action) = flag::<String>(&args, "--action") else {
        eprintln!("usage: axprobe perform --action NAME [--focused] [--delay S]");
        std::process::exit(2);
      };
      let (el, _) = pick_target(&system, &args, has(&args, "--focused"));
      perform_lab(&el, &action)
    }
    Some("census") => {
      let Some(pid) = args.get(1).and_then(|t| resolve_pid(t)) else {
        eprintln!("usage: axprobe census <app-name|pid> [--max N] [--menus] [--enhanced] [--manual]");
        std::process::exit(2);
      };
      let app = unsafe { AXUIElement::new_application(pid) };
      unsafe { app.set_messaging_timeout(2.0) };
      let mut info = target_info(&app);
      info["mode"] = json!("census");
      info["signals"] = assistive_signals(pid, &args);
      let result = census(&app, flag(&args, "--max").unwrap_or(3000), has(&args, "--menus"));
      info.as_object_mut().unwrap().extend(result.as_object().unwrap().clone());
      info
    }
    _ => {
      eprintln!(
        "usage:\n  axprobe focused [--delay S] [--depth N] [--enhanced] [--manual]\n  axprobe hover   [--delay S] [--depth N] [--enhanced] [--manual]\n  axprobe watch   [--delay S] [--focused] [--secs N] [--poll-ms N]\n  axprobe set     --attr NAME --value JSON [--value JSON ...] [--cf T] [--then ACTION] [--focused] [--delay S]\n  axprobe perform --action NAME [--focused] [--delay S]\n  axprobe param   --attr NAME --param JSON [--param JSON ...] [--focused] [--delay S]\n  axprobe census <app-name|pid> [--max N] [--menus] [--enhanced] [--manual]"
      );
      std::process::exit(2);
    }
  };
  println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
