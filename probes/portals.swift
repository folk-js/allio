// Probe: one document, many screens, light. Like screens.swift, but the webview is one screen in
// size: every Space's region overlaps in it, each on its own compositing layer, and each mirror
// shows only its region's layer through a CAPortalLayer (Core Animation's "render that layer
// here", across layer contexts). Memory should follow content, not the number of Spaces.
//
//   swift probes/portals.swift          run it; look, click, type, swipe (Ctrl-C to quit)
//   swift probes/portals.swift one      only the current Space: the memory baseline
//
// It reports by itself every 5 s: memory, the page's visibility and frame count; and every click
// and keystroke the page gets.

import AppKit
import WebKit

setvbuf(stdout, nil, _IOLBF, 0)

// MARK: SkyLight

let skylight = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_NOW)
func sym<T>(_ name: String, _: T.Type) -> T { unsafeBitCast(dlsym(skylight, name)!, to: T.self) }
let cid = sym("SLSMainConnectionID", (@convention(c) () -> Int32).self)()
let copyManagedDisplaySpaces = sym(
  "SLSCopyManagedDisplaySpaces", (@convention(c) (Int32) -> Unmanaged<CFArray>?).self)
let copySpacesForWindows = sym(
  "SLSCopySpacesForWindows", (@convention(c) (Int32, Int32, CFArray) -> Unmanaged<CFArray>?).self)
let addWindowsToSpaces = sym(
  "SLSAddWindowsToSpaces", (@convention(c) (Int32, CFArray, CFArray) -> Void).self)
let removeWindowsFromSpaces = sym(
  "SLSRemoveWindowsFromSpaces", (@convention(c) (Int32, CFArray, CFArray) -> Void).self)

/// The main display's Spaces in Mission Control order, and the current one.
func displaySpaces() -> (all: [UInt64], current: UInt64) {
  let displays = copyManagedDisplaySpaces(cid)?.takeRetainedValue() as? [[String: Any]] ?? []
  guard let d = displays.first else { return ([], 0) }
  let current = (d["Current Space"] as? [String: Any])?["ManagedSpaceID"] as? UInt64 ?? 0
  let all = (d["Spaces"] as? [[String: Any]] ?? []).compactMap { $0["ManagedSpaceID"] as? UInt64 }
  return (all, current)
}

func place(_ window: NSWindow, on space: UInt64) {
  let n = window.windowNumber
  let on = copySpacesForWindows(cid, 7, [n] as CFArray)?.takeRetainedValue() as? [UInt64] ?? []
  addWindowsToSpaces(cid, [n] as CFArray, [space] as CFArray)
  let others = on.filter { $0 != space }
  if !others.isEmpty { removeWindowsFromSpaces(cid, [n] as CFArray, others as CFArray) }
}

// MARK: Setup

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let args = Set(CommandLine.arguments.dropFirst())
let baseline = args.contains("one")


let display = NSScreen.main!.frame
let W = display.width
let H = display.height
let (allSpaces, firstCurrent) = displaySpaces()
let spaces = baseline ? [firstCurrent] : allSpaces
let N = spaces.count

/// Where each region's sticky is, in its region (top-left origin, points).
let sticky = CGRect(x: 80, y: 140, width: 380, height: 230)

func page() -> String {
  var regions = ""
  for (i, space) in spaces.enumerated() {
    regions += """
      <div class=region style="z-index:\(i)"><div class=sticky \
      style="left:\(Int(sticky.minX))px;top:\(Int(sticky.minY))px;width:\(Int(sticky.width))px;\
      height:\(Int(sticky.height))px;background:hsl(\(i * 57) 70% 42% / .95)">
        <b>Space #\(i + 1) (\(space))</b>
        <div class=clock></div><div class=frame></div>
        <button onclick="clicked(\(i))">Click me</button>
        <input placeholder="type here" oninput="typed(\(i), this.value)">
      </div></div>
      """
  }
  return """
    <html><head><style>
      body { margin: 0; background: transparent; overflow: hidden; font: 600 16px -apple-system; color: white }
      /* Each region: a layer of its own with nothing painted itself (no backing store). */
      .region { position: fixed; inset: 0; will-change: transform; pointer-events: none }
      .region.current .sticky { pointer-events: auto }
      .sticky { position: absolute; will-change: transform; box-sizing: border-box; padding: 14px 16px;
                border-radius: 14px }
      .clock { font: 700 52px ui-monospace, monospace; margin: 6px 0 }
      .frame { opacity: .75; font-size: 13px; margin-bottom: 10px }
      button, input { font: inherit; font-size: 14px }
    </style></head><body>\(regions)<script>
      let n = 0;
      const post = (m) => window.webkit.messageHandlers.log.postMessage(m);
      function clicked(i) { post('click on Space #' + (i + 1) + "'s sticky"); }
      function typed(i, v) { post('typed on Space #' + (i + 1) + ': ' + v); }
      function setCurrent(i) {
        document.querySelectorAll('.region').forEach((r, j) => r.classList.toggle('current', j === i));
      }
      function tick() {
        n++;
        const t = new Date().toLocaleTimeString([], { hour12: false });
        for (const c of document.querySelectorAll('.clock')) c.textContent = t;
        for (const f of document.querySelectorAll('.frame')) f.textContent = 'frame ' + n;
        requestAnimationFrame(tick);
      }
      requestAnimationFrame(tick);
    </script></body></html>
    """
}

final class Log: NSObject, WKScriptMessageHandler {
  func userContentController(_: WKUserContentController, didReceive message: WKScriptMessage) {
    print("page: \(message.body)")
  }
}

/// WebKit helper processes running now, by kind.
func webkitProcesses(_ kind: String) -> Set<Int32> {
  let p = Process()
  p.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
  p.arguments = ["-f", "com.apple.WebKit.\(kind)"]
  let pipe = Pipe()
  p.standardOutput = pipe
  try? p.run()
  p.waitUntilExit()
  let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
  return Set(out.split(separator: "\n").compactMap { Int32($0) })
}
let gpuBefore = webkitProcesses("GPU")

let config = WKWebViewConfiguration()
let log = Log()
config.userContentController.add(log, name: "log")

/// Borderless panels can't become key by default; the webview's must, for typing.
final class KeyPanel: NSPanel {
  override var canBecomeKey: Bool { true }
}

/// A view with a top-left origin, like WebKit's layers, so portals land where their source is.
final class FlippedView: NSView {
  override var isFlipped: Bool { true }
}

// The webview's window: on every Space, one screen in size, at full alpha so the window server
// gives it clicks (a window at alpha 0 gets none). It shows nothing itself: the webview sits in a
// container at opacity 0. Portals don't take on their source's opacity (`matchesOpacity` is off),
// so the mirrors still show it.
let a = KeyPanel(
  contentRect: display,
  styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
a.level = NSWindow.Level(rawValue: NSWindow.Level.floating.rawValue + 1)
a.isOpaque = false
a.backgroundColor = .clear
a.hasShadow = false
a.ignoresMouseEvents = true
a.becomesKeyOnlyIfNeeded = true
a.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
a.contentView!.wantsLayer = true
let hider = NSView(frame: NSRect(x: 0, y: 0, width: W, height: H))
hider.wantsLayer = true
hider.layer!.opacity = 0
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: W, height: H), configuration: config)
web.setValue(false, forKey: "drawsBackground")
// The webview's window is invisible on purpose; don't let WebKit throttle it as hidden (it did on
// full-screen Spaces).
let noOcclusion = NSSelectorFromString("_setWindowOcclusionDetectionEnabled:")
if web.responds(to: noOcclusion) {
  web.perform(noOcclusion, with: false)
} else {
  print("note: this WebKit has no _setWindowOcclusionDetectionEnabled:")
}
hider.addSubview(web)
a.contentView!.addSubview(hider)
web.loadHTMLString(page(), baseURL: nil)
a.orderFrontRegardless()

/// Index of the current Space among ours, or nil if it is one we have no region for.
var currentIndex: Int? { spaces.firstIndex(of: displaySpaces().current) }

/// Tells the page which region takes input: the current Space's.
func alignToCurrent() {
  web.evaluateJavaScript("setCurrent(\(currentIndex ?? -1))")
}

// The region layers: WebKit gives each region (and each sticky) a layer. A region's layer is the
// parent of its sticky's; stickies come in the regions' order.
func regionLayers() -> [CALayer] {
  var found: [CALayer] = []
  func walk(_ l: CALayer) {
    if abs(l.bounds.width - sticky.width) < 1, abs(l.bounds.height - sticky.height) < 1,
      let parent = l.superlayer
    {
      found.append(parent)
      return
    }
    for sub in l.sublayers ?? [] { walk(sub) }
  }
  walk(a.contentView!.layer!)
  return found
}

@_silgen_name("CALayerGetRenderId") func layerRenderId(_ layer: CALayer) -> UInt64

var mirrors: [NSPanel] = []
DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
  alignToCurrent()
  guard let context = a.contentView!.layer!.value(forKey: "context") as AnyObject?,
    let contextId = (context.value(forKey: "contextId") as? NSNumber)?.uint32Value,
    let portalClass = NSClassFromString("CAPortalLayer") as? CALayer.Type
  else {
    print("FAIL: no context or no CAPortalLayer")
    exit(1)
  }
  let regions = regionLayers()
  print("found \(regions.count) region layers for \(N) Spaces: \(regions.map { "\(type(of: $0)) \(Int($0.bounds.width))x\(Int($0.bounds.height)) render id \(layerRenderId($0))" })")
  guard regions.count == N else {
    print("FAIL: region layers don't match the Spaces")
    exit(1)
  }
  for region in regions {
    let m = NSPanel(
      contentRect: display, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
      defer: false)
    m.level = .floating
    m.isOpaque = false
    m.backgroundColor = .clear
    m.hasShadow = false
    m.ignoresMouseEvents = true
    m.collectionBehavior = [.fullScreenAuxiliary]
    m.contentView = FlippedView(frame: display)
    m.contentView!.wantsLayer = true
    let portal = portalClass.init()
    portal.setValue(NSNumber(value: contextId), forKey: "sourceContextId")
    portal.setValue(NSNumber(value: layerRenderId(region)), forKey: "sourceLayerRenderId")
    portal.setValue(true, forKey: "matchesPosition")
    portal.setValue(true, forKey: "crossDisplay")
    portal.frame = m.contentView!.bounds
    m.contentView!.layer!.addSublayer(portal)
    m.alphaValue = 0
    m.orderFrontRegardless()
    mirrors.append(m)
  }
  // Place once AppKit has committed ordering them in (else they snap back to this Space).
  DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
    for (m, space) in zip(mirrors, spaces) {
      place(m, on: space)
      m.alphaValue = 1
    }
    a.orderFrontRegardless()
    print("\(N) regions, \(mirrors.count) mirrors placed. Click and type in a sticky, swipe between Spaces.")
  }
}

// Passthrough: the webview takes the mouse only over the current region's sticky.
Timer.scheduledTimer(withTimeInterval: 1.0 / 120.0, repeats: true) { _ in
  let m = NSEvent.mouseLocation
  let topLeft = CGPoint(x: m.x - display.minX, y: display.maxY - m.y)
  let over = currentIndex != nil && sticky.contains(topLeft)
  if a.ignoresMouseEvents == over { a.ignoresMouseEvents = !over }
}

NSWorkspace.shared.notificationCenter.addObserver(
  forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
) { _ in
  alignToCurrent()
  print("Space changed: now region \(currentIndex.map { "#\($0 + 1)" } ?? "none"); webview window on it: \(a.isOnActiveSpace), occlusion visible: \(a.occlusionState.contains(.visible))")
}

// MARK: Reporting

func footprint(_ pid: Int32) -> String {
  let p = Process()
  p.executableURL = URL(fileURLWithPath: "/usr/bin/footprint")
  p.arguments = ["-p", "\(pid)"]
  let pipe = Pipe()
  p.standardOutput = pipe
  p.standardError = pipe
  try? p.run()
  p.waitUntilExit()
  let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
  guard let line = out.split(separator: "\n").first(where: { $0.contains("Footprint:") }),
    let range = line.range(of: "Footprint: ")
  else { return "?" }
  return String(line[range.upperBound...].split(separator: "(")[0]).trimmingCharacters(in: .whitespaces)
}

let started = Date()
Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in
  let content = (web.value(forKey: "_webProcessIdentifier") as? NSNumber)?.int32Value ?? 0
  let gpu = webkitProcesses("GPU").subtracting(gpuBefore).first
  web.evaluateJavaScript("[n, document.visibilityState].join(' ')") { r, _ in
    print(
      String(format: "%4.0f s", Date().timeIntervalSince(started))
        + ": page frame \((r as? String) ?? "?")"
        + " | memory: probe \(footprint(getpid())), web content \(content > 0 ? footprint(content) : "?")"
        + ", GPU \(gpu.map(footprint) ?? "(shared, not ours alone)")")
  }
}

// Never outlive the test, even if its terminal is lost.
DispatchQueue.main.asyncAfter(deadline: .now() + 180) {
  print("3 minutes up, quitting")
  exit(0)
}
app.run()
