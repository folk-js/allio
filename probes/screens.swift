// Probe: one document, many screens. The whole architecture, end to end.
//
// - One WKWebView holds a region per Space on the main display, side by side, each a screen in
//   size. Its window joins every Space, is invisible (alpha 0), and is moved so the current
//   Space's region lies exactly over the display. It is the only thing that takes input.
// - One mirror window per Space (the current one included) is placed on that Space and shows its
//   region by hosting the webview window's layer context. Mirrors ignore the mouse, and slide
//   with their Space during a swipe.
// - Passthrough as allio does it: the webview takes the mouse only over a sticky; elsewhere
//   clicks go to the apps below.
//
// Each region has one sticky: its Space, a clock, a frame counter, a button and a text field.
//
//   swift probes/screens.swift            run it; look, click, type, swipe (Ctrl-C to quit)
//   swift probes/screens.swift one        a single region (the current Space) and no mirrors
//                                         except its own: the memory baseline
//   ... layers                            each sticky on its own compositing layer (cheaper?)
//
// It reports by itself, every 5 s: memory (this process, its web content process, WebKit's GPU
// process), the page's visibility and frame count; and every click and keystroke the page gets.

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
/// Give each sticky its own compositing layer, so the transparent page itself may need no tiles.
let layered = args.contains("layers")
/// Only the first region has a sticky: does memory follow the webview's area or its content?
let emptyRegions = args.contains("empty")

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
  for (i, space) in spaces.enumerated() where !(emptyRegions && i > 0) {
    regions += """
      <div class=sticky style="left:\(Int(CGFloat(i) * W + sticky.minX))px;top:\(Int(sticky.minY))px;\
      width:\(Int(sticky.width))px;height:\(Int(sticky.height))px;background:hsl(\(i * 57) 70% 42% / .95)">
        <b>Space #\(i + 1) (\(space))</b>
        <div class=clock></div><div class=frame></div>
        <button onclick="clicked(\(i))">Click me</button>
        <input placeholder="type here" oninput="typed(\(i), this.value)">
      </div>
      """
  }
  return """
    <html><head><style>
      body { margin: 0; background: transparent; overflow: hidden; width: \(Int(CGFloat(N) * W))px;
             font: 600 16px -apple-system; color: white }
      .sticky { \(layered ? "will-change: transform;" : "") position: absolute; box-sizing: border-box; padding: 14px 16px; border-radius: 14px;
                box-shadow: 0 8px 30px rgba(0,0,0,.35) }
      .clock { font: 700 52px ui-monospace, monospace; margin: 6px 0 }
      .frame { opacity: .75; font-size: 13px; margin-bottom: 10px }
      button, input { font: inherit; font-size: 14px }
    </style></head><body>\(regions)<script>
      let n = 0;
      const post = (m) => window.webkit.messageHandlers.log.postMessage(m);
      function clicked(i) { post('click on Space #' + (i + 1) + "'s sticky"); }
      function typed(i, v) { post('typed on Space #' + (i + 1) + ': ' + v); }
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

// The webview's window: on every Space, invisible, holding every region.
let a = NSPanel(
  contentRect: NSRect(x: display.minX, y: display.minY, width: CGFloat(N) * W, height: H),
  styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
a.level = NSWindow.Level(rawValue: NSWindow.Level.floating.rawValue + 1)
a.isOpaque = false
a.backgroundColor = .clear
a.hasShadow = false
a.alphaValue = 0
a.ignoresMouseEvents = true
a.becomesKeyOnlyIfNeeded = true
a.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
a.contentView!.wantsLayer = true
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: CGFloat(N) * W, height: H), configuration: config)
web.setValue(false, forKey: "drawsBackground")
a.contentView!.addSubview(web)
web.loadHTMLString(page(), baseURL: nil)
a.orderFrontRegardless()

/// Index of the current Space among ours, or nil if it is one we have no region for.
var currentIndex: Int? { spaces.firstIndex(of: displaySpaces().current) }

/// Moves the webview's window so the current Space's region lies over the display.
func alignToCurrent() {
  guard let i = currentIndex else { return }
  a.setFrameOrigin(NSPoint(x: display.minX - CGFloat(i) * W, y: display.minY))
}
alignToCurrent()

// The mirrors: one per Space, hosting the webview window's context, shifted to its region.
var mirrors: [NSPanel] = []
DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
  guard let context = a.contentView!.layer!.value(forKey: "context") as AnyObject?,
    let contextId = (context.value(forKey: "contextId") as? NSNumber)?.uint32Value,
    let hostClass = NSClassFromString("CALayerHost") as? CALayer.Type
  else {
    print("FAIL: can't host the webview window's context")
    exit(1)
  }
  for (i, _) in spaces.enumerated() {
    let m = NSPanel(
      contentRect: display, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
      defer: false)
    m.level = .floating
    m.isOpaque = false
    m.backgroundColor = .clear
    m.hasShadow = false
    m.ignoresMouseEvents = true
    m.collectionBehavior = [.fullScreenAuxiliary]
    m.contentView!.wantsLayer = true
    let host = hostClass.init()
    host.setValue(NSNumber(value: contextId), forKey: "contextId")
    host.anchorPoint = .zero
    host.frame = NSRect(x: -CGFloat(i) * W, y: 0, width: CGFloat(N) * W, height: H)
    m.contentView!.layer!.masksToBounds = true
    m.contentView!.layer!.addSublayer(host)
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
  print("Space changed: now region \(currentIndex.map { "#\($0 + 1)" } ?? "none")")
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

app.run()
