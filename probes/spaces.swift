// Probe: can Spaces be identified robustly, and can we draw on a chosen one?
//
// Uses SkyLight (the window server's private framework), looked up at runtime. Nothing here
// needs SIP off or touches other apps' windows; it only reads, and moves our own windows.
//
//   swift probes/spaces.swift list    displays, their Spaces, and the windows on each
//   swift probes/spaces.swift watch   prints whenever the current Space changes (Ctrl-C)
//   swift probes/spaces.swift draw [add|move|sticky]
//                                     one labelled panel per Space, placed by that method (Ctrl-C)
//   swift probes/spaces.swift check   the same with invisible panels, reporting and exiting
//   swift probes/spaces.swift ax      which windows accessibility can reach, per Space
//                                     (needs Accessibility for the terminal running it)
//   swift probes/spaces.swift keep    holds the windows accessibility lists now, and on every
//                                     Space change says whether each handle still works
//
// `draw` tries each way of placing a window on a Space and reports which ones the window
// server honoured, by reading back which Spaces the window is on.

import AppKit

// MARK: SkyLight

let skylight = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_NOW)
func sym<T>(_ name: String, _: T.Type) -> T? {
  guard let p = dlsym(skylight, name) else {
    print("missing \(name)")
    return nil
  }
  return unsafeBitCast(p, to: T.self)
}

typealias CID = Int32
let mainConnection = sym("SLSMainConnectionID", (@convention(c) () -> CID).self)!
let copyManagedDisplaySpaces = sym(
  "SLSCopyManagedDisplaySpaces", (@convention(c) (CID) -> Unmanaged<CFArray>?).self)!
let copySpacesForWindows = sym(
  "SLSCopySpacesForWindows", (@convention(c) (CID, Int32, CFArray) -> Unmanaged<CFArray>?).self)!
let spaceGetType = sym("SLSSpaceGetType", (@convention(c) (CID, UInt64) -> Int32).self)
let addWindowsToSpaces = sym(
  "SLSAddWindowsToSpaces", (@convention(c) (CID, CFArray, CFArray) -> Void).self)
let removeWindowsFromSpaces = sym(
  "SLSRemoveWindowsFromSpaces", (@convention(c) (CID, CFArray, CFArray) -> Void).self)
let moveWindowsToManagedSpace = sym(
  "SLSMoveWindowsToManagedSpace", (@convention(c) (CID, CFArray, UInt64) -> Void).self)

let cid = mainConnection()

struct Space {
  let id: UInt64
  let uuid: String
  let type: Int  // 0 desktop, 4 full screen
  let display: String
  let index: Int  // 1-based, per display, as Mission Control orders them
  let current: Bool
  let info: [String: Any]
}

func spaces() -> [Space] {
  let displays = copyManagedDisplaySpaces(cid)?.takeRetainedValue() as? [[String: Any]] ?? []
  var out: [Space] = []
  for d in displays {
    let display = d["Display Identifier"] as? String ?? "?"
    let current = (d["Current Space"] as? [String: Any])?["ManagedSpaceID"] as? UInt64
    for (i, s) in (d["Spaces"] as? [[String: Any]] ?? []).enumerated() {
      let id = s["ManagedSpaceID"] as? UInt64 ?? s["id64"] as? UInt64 ?? 0
      out.append(
        Space(
          id: id, uuid: s["uuid"] as? String ?? "", type: s["type"] as? Int ?? -1,
          display: display, index: i + 1, current: id == current, info: s))
    }
  }
  return out
}

func spacesOf(_ windows: [Int]) -> [UInt64] {
  // Mask 7: every kind of Space (current, others, user and full screen).
  copySpacesForWindows(cid, 7, windows as CFArray)?.takeRetainedValue() as? [UInt64] ?? []
}

/// All windows the window server knows (not only on-screen ones), layer 0.
func allWindows() -> [[String: Any]] {
  (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? [])
    .filter { ($0[kCGWindowLayer as String] as? Int) == 0 }
}

func describe(_ s: Space) -> String {
  let kind = s.type == 4 ? "full screen" : s.type == 0 ? "desktop" : "type \(s.type)"
  return "space \(s.id) #\(s.index) \(kind)\(s.current ? " (current)" : "") uuid=\(s.uuid.isEmpty ? "-" : s.uuid)"
}

// MARK: Commands

func list() {
  let all = spaces()
  let windows = allWindows()
  var bySpace: [UInt64: [String]] = [:]
  for w in windows {
    let n = w[kCGWindowNumber as String] as? Int ?? 0
    let owner = w[kCGWindowOwnerName as String] as? String ?? "?"
    let onScreen = (w[kCGWindowIsOnscreen as String] as? Bool) == true
    for sid in spacesOf([n]) {
      bySpace[sid, default: []].append("\(owner) #\(n)\(onScreen ? "" : " (off screen)")")
    }
  }
  var lastDisplay = ""
  for s in all {
    if s.display != lastDisplay {
      print("display \(s.display)")
      lastDisplay = s.display
    }
    print("  " + describe(s))
    if s.type == 4, let tile = s.info["TileLayoutManager"] as? [String: Any] {
      print("    tiles: \(tile["TileSpaces"].map { "\($0)".prefix(200) } ?? "-")")
    }
    for w in bySpace[s.id] ?? [] { print("    \(w)") }
  }
  let sticky = windows.filter { spacesOf([$0[kCGWindowNumber as String] as? Int ?? 0]).count > 1 }
  print("windows on more than one Space: \(sticky.count)")
  let none = windows.filter { spacesOf([$0[kCGWindowNumber as String] as? Int ?? 0]).isEmpty }
  print("windows on no Space (minimised, hidden or not real): \(none.count)")
}

func watch() {
  func show(_ why: String) {
    let current = spaces().filter(\.current).map(describe).joined(separator: " | ")
    print("\(why): \(current)")
  }
  show("now")
  NSWorkspace.shared.notificationCenter.addObserver(
    forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
  ) { _ in show("activeSpaceDidChange") }
  NotificationCenter.default.addObserver(
    forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
  ) { _ in show("screens changed") }
  NSApplication.shared.run()
}

var panels: [(NSPanel, Space)] = []

func panel(_ text: String, on screen: NSScreen, row: Int, sticky: Bool = false) -> (NSPanel, NSTextField) {
  let size = NSSize(width: 560, height: 64)
  let f = screen.visibleFrame
  let p = NSPanel(
    contentRect: NSRect(
      x: f.midX - size.width / 2, y: f.maxY - CGFloat(row + 1) * (size.height + 8) - 32,
      width: size.width, height: size.height),
    styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
  p.level = .floating
  p.isOpaque = false
  p.backgroundColor = NSColor(hue: CGFloat(row) * 0.17, saturation: 0.6, brightness: 0.35, alpha: 0.85)
  p.ignoresMouseEvents = true
  // Full-screen Spaces only show auxiliary windows; sticky ones show on every Space.
  p.collectionBehavior = sticky ? [.canJoinAllSpaces, .fullScreenAuxiliary] : [.fullScreenAuxiliary]
  let label = NSTextField(labelWithString: text)
  label.textColor = .white
  label.font = .monospacedSystemFont(ofSize: 12, weight: .medium)
  label.frame = NSRect(x: 14, y: 10, width: size.width - 28, height: 44)
  label.maximumNumberOfLines = 3
  p.contentView?.addSubview(label)
  return (p, label)
}

func screenFor(_ display: String) -> NSScreen {
  NSScreen.screens.first { s in
    guard let n = s.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID,
      let uuid = CGDisplayCreateUUIDFromDisplayID(n)?.takeRetainedValue()
    else { return false }
    return CFUUIDCreateString(nil, uuid) as String == display
  } ?? NSScreen.main!
}

/// Where each panel is, as the window server and AppKit see it.
func report(_ why: String) {
  let current = spaces().filter(\.current).map { "\($0.id)" }.joined(separator: ",")
  print("\(why) (current: \(current))")
  for (p, s) in panels {
    print("  panel for \(s.id): on \(spacesOf([p.windowNumber])), onActiveSpace=\(p.isOnActiveSpace) visible=\(p.isVisible)")
  }
}

/// One panel per Space, placed by `method`: `add` (SLSAddWindowsToSpaces), `move`
/// (SLSMoveWindowsToManagedSpace), or `sticky` (one panel on every Space that changes what it
/// says when the Space changes: public API only).
func draw(_ method: String, invisible: Bool = false) {
  let app = NSApplication.shared
  app.setActivationPolicy(.accessory)
  if method == "sticky" {
    let (p, label) = panel("", on: NSScreen.main!, row: 0, sticky: true)
    func update() {
      label.stringValue = spaces().filter(\.current).map(describe).joined(separator: "\n")
    }
    update()
    p.orderFrontRegardless()
    NSWorkspace.shared.notificationCenter.addObserver(
      forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
    ) { _ in update() }
    print("One sticky panel; it should say which Space you're on, on every Space. Ctrl-C to quit.")
    app.run()
  }
  for (row, s) in spaces().enumerated() {
    let (p, _) = panel("\(describe(s))\nplaced by \(method)", on: screenFor(s.display), row: row)
    // Ordered in invisible: until it is placed, it is on the current Space.
    p.alphaValue = 0
    p.orderFrontRegardless()
    panels.append((p, s))
  }
  // AppKit commits ordering a window in to the window server later (at the end of the run loop
  // turn), and that commit puts the window on the current Space. Place it once that is done.
  DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
    for (p, s) in panels {
      let n = p.windowNumber
      let before = spacesOf([n])
      if method == "move" {
        moveWindowsToManagedSpace?(cid, [n] as CFArray, s.id)
      } else {
        addWindowsToSpaces?(cid, [n] as CFArray, [s.id] as CFArray)
        let others = before.filter { $0 != s.id }
        if !others.isEmpty { removeWindowsFromSpaces?(cid, [n] as CFArray, others as CFArray) }
      }
      let after = spacesOf([n])
      if !invisible { p.alphaValue = 1 }
      print("\(after == [s.id] ? "ok  " : "FAIL") \(describe(s)): window \(n) was on \(before), now on \(after)")
    }
    if invisible {
      DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
        report("after 1 s")
        exit(0)
      }
    }
  }
  if invisible {
    app.run()
  }
  DispatchQueue.main.asyncAfter(deadline: .now() + 1.3) { report("after 1 s") }
  NSWorkspace.shared.notificationCenter.addObserver(
    forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
  ) { _ in report("space changed") }
  print("Labels are stacked in rows, so overlapping ones show. Switch Spaces; Ctrl-C to quit.")
  app.run()
}

@_silgen_name("_AXUIElementGetWindow")
func axWindowID(_ e: AXUIElement, _ id: UnsafeMutablePointer<CGWindowID>) -> AXError

@_silgen_name("_AXUIElementCreateWithRemoteToken")
func axCreateWithRemoteToken(_ token: CFData) -> Unmanaged<AXUIElement>?

/// An app's windows found by building element handles directly (pid, magic, element index), as
/// AltTab does: this reaches windows that `AXWindows` leaves out, like those on other Spaces.
func windowsByToken(_ pid: pid_t, upTo limit: UInt64 = 1000) -> [Int: AXUIElement] {
  var token = Data(count: 20)
  token.replaceSubrange(0..<4, with: withUnsafeBytes(of: pid) { Data($0) })
  token.replaceSubrange(4..<8, with: withUnsafeBytes(of: Int32(0)) { Data($0) })
  token.replaceSubrange(8..<12, with: withUnsafeBytes(of: Int32(0x636f_636f)) { Data($0) })
  var found: [Int: AXUIElement] = [:]
  for element: UInt64 in 0..<limit {
    token.replaceSubrange(12..<20, with: withUnsafeBytes(of: element) { Data($0) })
    guard let e = axCreateWithRemoteToken(token as CFData)?.takeRetainedValue() else { continue }
    var role: CFTypeRef?
    AXUIElementCopyAttributeValue(e, kAXRoleAttribute as CFString, &role)
    guard role as? String == kAXWindowRole else { continue }
    var id: CGWindowID = 0
    if axWindowID(e, &id) == .success, id != 0 { found[Int(id)] = e }
  }
  return found
}

/// For each app, its windows the window server has (by Space) against those accessibility lists.
func ax() {
  guard AXIsProcessTrusted() else {
    print("This terminal needs Accessibility (System Settings > Privacy & Security)")
    exit(1)
  }
  let kinds = Dictionary(uniqueKeysWithValues: spaces().map { ($0.id, $0) })
  let windows = allWindows().filter {
    ((($0[kCGWindowBounds as String] as? [String: Any])?["Height"] as? Double) ?? 0) > 100
  }
  for app in NSWorkspace.shared.runningApplications where app.activationPolicy == .regular {
    let pid = app.processIdentifier
    var value: CFTypeRef?
    AXUIElementCopyAttributeValue(AXUIElementCreateApplication(pid), kAXWindowsAttribute as CFString, &value)
    let reachable = Set((value as? [AXUIElement] ?? []).map { e -> Int in
      var id: CGWindowID = 0
      _ = axWindowID(e, &id)
      return Int(id)
    })
    let mine = windows.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
    if mine.isEmpty { continue }
    var byToken = windowsByToken(pid)
    let missing = mine.map { $0[kCGWindowNumber as String] as? Int ?? 0 }
      .filter { !reachable.contains($0) && byToken[$0] == nil && !spacesOf([$0]).isEmpty }
    var deep = ""
    if !missing.isEmpty {
      let started = Date()
      byToken = windowsByToken(pid, upTo: 100_000)
      deep = String(format: " (scanned 100k element ids in %.1f s)", Date().timeIntervalSince(started))
    }
    print((app.localizedName ?? "?") + deep)
    for w in mine {
      let n = w[kCGWindowNumber as String] as? Int ?? 0
      let on = spacesOf([n]).map { sid in
        kinds[sid].map { "#\($0.index)\($0.current ? "*" : "")\($0.type == 4 ? "fs" : "")" } ?? "\(sid)"
      }
      let how = reachable.contains(n) ? "AXWindows" : byToken[n] != nil ? "token only" : "unreachable"
      print("  window \(n) on \(on.isEmpty ? "no Space" : on.joined(separator: ",")): \(how)")
    }
  }
}

/// Do accessibility handles taken now keep working once their window is on another Space?
func keep() {
  guard AXIsProcessTrusted() else {
    print("This terminal needs Accessibility (System Settings > Privacy & Security)")
    exit(1)
  }
  var held: [(String, Int, AXUIElement)] = []
  for app in NSWorkspace.shared.runningApplications where app.activationPolicy == .regular {
    var value: CFTypeRef?
    AXUIElementCopyAttributeValue(
      AXUIElementCreateApplication(app.processIdentifier), kAXWindowsAttribute as CFString, &value)
    for e in value as? [AXUIElement] ?? [] {
      var id: CGWindowID = 0
      _ = axWindowID(e, &id)
      held.append((app.localizedName ?? "?", Int(id), e))
    }
  }
  func check(_ why: String) {
    let current = spaces().filter(\.current).map { "\($0.id)" }.joined(separator: ",")
    print("\(why) (current: \(current))")
    for (name, id, e) in held {
      var title: CFTypeRef?
      let err = AXUIElementCopyAttributeValue(e, kAXTitleAttribute as CFString, &title)
      var children: CFTypeRef?
      AXUIElementCopyAttributeValue(e, kAXChildrenAttribute as CFString, &children)
      let kids = (children as? [AXUIElement])?.count ?? 0
      let on = spacesOf([id]).map(String.init).joined(separator: ",")
      print("  \(name) #\(id) on [\(on)]: \(err == .success ? "ok, \(kids) children, \"\((title as? String ?? "").prefix(40))\"" : "error \(err.rawValue)")")
    }
  }
  check("held \(held.count) windows")
  NSWorkspace.shared.notificationCenter.addObserver(
    forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
  ) { _ in check("space changed") }
  print("Switch to another Space and back. Ctrl-C to quit.")
  NSApplication.shared.run()
}

switch CommandLine.arguments.dropFirst().first ?? "list" {
case "keep": keep()
case "ax": ax()
case "watch": watch()
case "draw": draw(CommandLine.arguments.dropFirst(2).first ?? "add")
case "check": draw("add", invisible: true)
default: list()
}
