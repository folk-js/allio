// Probe: one webview, many screens. Can a second window, possibly on another Space, show part of
// one live webview without a second webview?
//
// Window A holds the only WKWebView: a page twice as wide as the window's visible part, with two
// regions, each animating (a frame counter and a moving bar). A sits at the right edge of the
// main screen so region 1 is on screen and region 2 hangs off it.
//
// Window B has no webview. It hosts A's layer context (CALayerHost, our own process's content)
// shifted left so that region 2 shows through it.
//
//   swift probes/mirror.swift here    B on this Space, at the left: both regions on screen
//   swift probes/mirror.swift there   B on another Space: switch to it, and pan back and forth
//
// Variants, as extra arguments:
//   occlusion   turn off WebKit's window occlusion detection (it throttles pages it thinks are hidden)
//   onscreen    put A entirely on screen, nearly transparent, so no part of the page is off screen
//
//   paint       no mirror: just report, every 2 s, whether WebKit is still painting each region
//               (read from its own surfaces in this process: no screen capture), then exit
//
// Every 2 s it logs the page's frame counter and visibility, to tell "the page stopped running"
// from "the page runs but region 2 stopped being painted".
//
// Look for: does B show region 2 at all, is it live (counter and bar moving), and with `there`,
// does it slide in with its Space during the swipe? Ctrl-C to quit.

import AppKit
import WebKit

setvbuf(stdout, nil, _IOLBF, 0)

// MARK: SkyLight (as in spaces.swift)

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

func spacesOf(_ n: Int) -> [UInt64] {
  copySpacesForWindows(cid, 7, [n] as CFArray)?.takeRetainedValue() as? [UInt64] ?? []
}

/// The current Space and the others, on the first display.
func spaces() -> (current: UInt64, others: [UInt64]) {
  let displays = copyManagedDisplaySpaces(cid)?.takeRetainedValue() as? [[String: Any]] ?? []
  guard let d = displays.first else { return (0, []) }
  let current = (d["Current Space"] as? [String: Any])?["ManagedSpaceID"] as? UInt64 ?? 0
  let all = (d["Spaces"] as? [[String: Any]] ?? []).compactMap { $0["ManagedSpaceID"] as? UInt64 }
  return (current, all.filter { $0 != current })
}

// MARK: The page

let W: CGFloat = 480
let H: CGFloat = 300
let page = """
  <html><body style="margin:0;background:transparent;font:600 20px -apple-system;color:white;overflow:hidden">
  <div id=r1 style="position:absolute;left:0;top:0;width:\(Int(W))px;height:\(Int(H))px;background:rgba(40,90,210,.9)">
    <div style="padding:16px">Region 1 (in the webview's window)</div><div class=t style="padding:0 16px"></div>
    <div class=bar style="position:absolute;bottom:20px;left:0;width:40px;height:40px;background:white;border-radius:8px"></div>
  </div>
  <div id=r2 style="position:absolute;left:\(Int(W))px;top:0;width:\(Int(W))px;height:\(Int(H))px;background:rgba(210,60,90,.9);text-align:center">
    <div style="padding:18px 16px 0">Mirrored. Is this clock ticking?</div>
    <div class=clock style="font:700 84px ui-monospace,monospace;padding-top:24px"></div>
    <div class=t style="opacity:.7"></div>
    <div class=bar style="position:absolute;bottom:20px;left:0;width:40px;height:40px;background:white;border-radius:8px"></div>
  </div>
  <script>
    let n = 0;
    function tick(t) {
      n++;
      for (const r of document.querySelectorAll('#r1,#r2')) {
        r.querySelector('.t').textContent = 'frame ' + n;
        const c = r.querySelector('.clock');
        if (c) c.textContent = new Date().toLocaleTimeString([], { hour12: false });
        r.querySelector('.bar').style.transform = 'translateX(' + ((t / 4) % (\(Int(W)) - 40)) + 'px)';
      }
      requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  </script></body></html>
  """

// MARK: Layer inspection

func layerContext(_ layer: CALayer) -> (AnyObject, UInt32)? {
  guard layer.responds(to: NSSelectorFromString("context")),
    let context = layer.value(forKey: "context") as AnyObject?
  else { return nil }
  let id = (context.value(forKey: "contextId") as? NSNumber)?.uint32Value ?? 0
  return (context, id)
}

/// Prints the layer tree, marking hosted contexts (how WebKit shows its content process).
func dump(_ layer: CALayer, _ depth: Int = 0, _ maxDepth: Int = 7) {
  var line = String(repeating: "  ", count: depth) + String(describing: type(of: layer))
  if NSStringFromClass(type(of: layer)) == "CALayerHost" {
    line += " contextId=\(layer.value(forKey: "contextId") ?? "?")"
  }
  line += " \(Int(layer.bounds.width))x\(Int(layer.bounds.height))"
  print(line)
  if depth < maxDepth {
    for sub in layer.sublayers ?? [] { dump(sub, depth + 1, maxDepth) }
  }
}

// MARK: Is WebKit painting?

/// Every layer with contents, with its frame in `root`'s coordinates.
func painted(_ layer: CALayer, in root: CALayer, into out: inout [(CGRect, CALayer)]) {
  if layer.contents != nil { out.append((layer.convert(layer.bounds, to: root), layer)) }
  for sub in layer.sublayers ?? [] { painted(sub, in: root, into: &out) }
}

/// A token that changes whenever the layer's backing is drawn into: the IOSurface seed if the
/// contents are an IOSurface, else the identity of the contents object.
func paintToken(_ layer: CALayer) -> String {
  guard let c = layer.contents else { return "none" }
  let cf = c as CFTypeRef
  if CFGetTypeID(cf) == IOSurfaceGetTypeID() {
    let surface = unsafeBitCast(cf, to: IOSurfaceRef.self)
    return "s\(IOSurfaceGetID(surface)):\(IOSurfaceGetSeed(surface))"
  }
  return "\(String(describing: type(of: c)))@\(Unmanaged.passUnretained(c as AnyObject).toOpaque())"
}

// MARK: Windows

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let args = Set(CommandLine.arguments.dropFirst())
let there = args.contains("there")
let screen = NSScreen.main!.visibleFrame

func borderless(_ frame: NSRect) -> NSPanel {
  let p = NSPanel(
    contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
    defer: false)
  p.level = .floating
  p.isOpaque = false
  p.backgroundColor = .clear
  p.hasShadow = false
  p.collectionBehavior = [.fullScreenAuxiliary]
  p.contentView!.wantsLayer = true
  return p
}

// A: the webview, twice as wide as what is on screen (region 2 hangs off the right edge), or with
// `onscreen` entirely on screen and almost invisible.
let aFrame =
  args.contains("onscreen")
  ? NSRect(x: screen.midX - W, y: screen.minY + 20, width: 2 * W, height: H)
  : NSRect(x: screen.maxX - W, y: screen.midY - H / 2, width: 2 * W, height: H)
let a = borderless(aFrame)
// As in the design: the one real webview is always on the current Space (it follows you), so
// WebKit never sees it hidden; B shows other Spaces' regions.
a.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
if args.contains("onscreen") { a.alphaValue = 0.02 }
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 2 * W, height: H))
web.setValue(false, forKey: "drawsBackground")
if args.contains("occlusion") {
  let sel = NSSelectorFromString("_setWindowOcclusionDetectionEnabled:")
  if web.responds(to: sel) {
    web.perform(sel, with: false)
    print("window occlusion detection off")
  } else {
    print("no _setWindowOcclusionDetectionEnabled: on this WebKit")
  }
}
a.contentView!.addSubview(web)
a.orderFrontRegardless()
web.loadHTMLString(page, baseURL: nil)

// B: no webview, just a layer hosting A's context, shifted so region 2 shows.
let b = borderless(NSRect(x: screen.minX + 40, y: screen.midY - H / 2, width: W, height: H))
b.alphaValue = 0
b.orderFrontRegardless()

DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
  print("--- A's layer tree")
  dump(a.contentView!.superview?.layer ?? a.contentView!.layer!)

  guard let (_, contextId) = layerContext(a.contentView!.layer!), contextId != 0 else {
    print("FAIL: no CAContext id for A's layers")
    exit(1)
  }
  print("A's context id: \(contextId)")
  guard let hostClass = NSClassFromString("CALayerHost") as? CALayer.Type else {
    print("FAIL: no CALayerHost")
    exit(1)
  }
  let host = hostClass.init()
  host.setValue(NSNumber(value: contextId), forKey: "contextId")
  // A's root layer spans both regions; shift it left so B's frame shows region 2.
  host.frame = NSRect(x: -W, y: 0, width: 2 * W, height: H)
  let container = b.contentView!.layer!
  container.masksToBounds = true
  container.addSublayer(host)

  if args.contains("paint") {
    b.orderOut(nil)
    let root = a.contentView!.layer!
    var last: [ObjectIdentifier: String] = [:]
    var round = 0
    Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { _ in
      var layers: [(CGRect, CALayer)] = []
      painted(root, in: root, into: &layers)
      var changed = [false, false]
      var kinds = Set<String>()
      for (frame, layer) in layers {
        let token = paintToken(layer)
        kinds.insert(token.hasPrefix("s") ? "IOSurface" : String(token.split(separator: "@")[0]))
        let id = ObjectIdentifier(layer)
        // Region 1 is x < W, region 2 is x >= W, in A's (unflipped) layer coordinates.
        if last[id] != nil && last[id] != token {
          if frame.minX < W { changed[0] = true }
          if frame.maxX > W { changed[1] = true }
        }
        last[id] = token
      }
      round += 1
      if round > 1 {
        print("  \(round * 2) s: \(layers.count) painted layers (\(kinds.sorted().joined(separator: ", "))); region 1 repainted: \(changed[0]), region 2 repainted: \(changed[1])")
      }
      if round >= 25 { exit(0) }
    }
    return
  }
  if there {
    let (current, others) = spaces()
    guard let target = others.first else {
      print("FAIL: there is only one Space on this display")
      exit(1)
    }
    let n = b.windowNumber
    addWindowsToSpaces(cid, [n] as CFArray, [target] as CFArray)
    removeWindowsFromSpaces(cid, [n] as CFArray, [current] as CFArray)
    print("B placed on Space \(target) (now on \(spacesOf(n))); A is on every Space")
    print("Switch to Space \(target) and pan back and forth.")
  } else {
    print("B is on this Space, at the left of the screen.")
  }
  b.alphaValue = 1
  print("Ctrl-C to quit.")
  let started = Date()
  Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { _ in
    web.evaluateJavaScript("[n, document.visibilityState, document.hasFocus()].join(' ')") { r, e in
      print(String(format: "%5.0f s: page frame %@", Date().timeIntervalSince(started), (r as? String) ?? "error \(e.map { "\($0)" } ?? "")"))
    }
  }
}

app.run()
