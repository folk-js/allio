// Probe: find the WebKit layer of a given element, robustly. Result: by WebKit's layer name
// (it contains the element's id) and, independently, by an opacity marker (1 - k/100000) that
// renders exactly like 1. Both agree under every step; the layer is only replaced when the
// container moves in the DOM or returns from display:none, and a rescan rebinds it.
//
// Portals need the Core Animation layer of each Space's container element, but WebKit doesn't
// say which layer is whose. Ruled out: a `transform-origin` z marker (WebKit drops it) and a size
// marker (WebKit clips fixed-position layers to the viewport).
//
// The page puts marked containers among decoys (same size, unmarked, composited), then a script
// does what real pages do: change content, resize, move in the DOM, add and remove composited
// children, hide and show, animate. After each step the host rescans the layer tree and reports,
// per marked container: found exactly once? same layer as before (or replaced)?
//
//   swift probes/layers.swift      runs by itself, prints a report, exits. Nothing to look at.

import AppKit
import WebKit

setvbuf(stdout, nil, _IOLBF, 0)

let markers: [Int] = [1001, 1002, 1003]

let page = """
  <html><head><style>
    body { margin: 0; background: transparent; font: 14px -apple-system }
    .box { position: fixed; inset: 0; will-change: transform; pointer-events: none }
    .note { position: absolute; left: 40px; top: 40px; width: 200px; height: 120px;
            background: #c55; will-change: transform; color: white; padding: 8px }
  </style></head><body>
    <div class=box id=decoy1></div>
    <div class=box id=m1001 style="opacity: 0.99999"><div class=note>one</div></div>
    <div class=box id=decoy2><div class=note>decoy</div></div>
    <div class=box id=m1002 style="opacity: 0.99998"></div>
    <div class=box id=m1003 style="opacity: 0.99997"><div class=note>three</div></div>
  <script>
    const $ = (id) => document.getElementById(id);
    let spinning = null;
    const steps = [
      ['load', () => {}],
      ['change text', () => { $('m1001').querySelector('.note').textContent = 'changed ' + Date.now(); }],
      ['add a composited child to an empty container', () => {
        const n = document.createElement('div'); n.className = 'note'; n.textContent = 'new';
        $('m1002').append(n); }],
      ['remove it again', () => { $('m1002').replaceChildren(); }],
      ['resize the page', () => { document.body.style.width = '700px'; }],
      ['move a container in the DOM', () => { document.body.prepend($('m1003')); }],
      ['change a child opacity', () => { $('m1001').querySelector('.note').style.opacity = '0.5'; }],
      ['visibility hidden', () => { $('m1001').style.visibility = 'hidden'; }],
      ['visibility visible', () => { $('m1001').style.visibility = 'visible'; }],
      ['display none', () => { $('m1002').style.display = 'none'; }],
      ['display back', () => { $('m1002').style.display = ''; }],
      ['many new composited elements', () => {
        for (let i = 0; i < 200; i++) {
          const n = document.createElement('div'); n.className = 'note';
          n.style.left = (i * 7) + 'px'; n.style.top = (i * 3) + 'px'; n.textContent = i;
          $('m1003').append(n);
        } }],
      ['remove them', () => { $('m1003').replaceChildren(); }],
      ['animate a child (transform)', () => {
        const n = $('m1001').querySelector('.note'); let a = 0;
        spinning = setInterval(() => { n.style.transform = 'translateX(' + (a++ % 100) + 'px)'; }, 16); }],
      ['stop animating', () => { clearInterval(spinning); }],
      
      ['window resize', () => {}],
      ['remove a container', () => { $('m1002').remove(); }],
    ];
    function step(i) { if (i < steps.length) { steps[i][1](); return steps[i][0]; } return null; }
  </script></body></html>
  """

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSPanel(
  contentRect: NSRect(x: 60, y: 60, width: 800, height: 500),
  styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
window.isOpaque = false
window.backgroundColor = .clear
window.alphaValue = 0.01  // barely there: this probe reports, it isn't for looking at
window.contentView!.wantsLayer = true
let web = WKWebView(frame: window.contentView!.bounds)
web.autoresizingMask = [.width, .height]
web.setValue(false, forKey: "drawsBackground")
web.perform(NSSelectorFromString("_setWindowOcclusionDetectionEnabled:"), with: false)
window.contentView!.addSubview(web)
window.orderFrontRegardless()
web.loadHTMLString(page, baseURL: nil)

@_silgen_name("CALayerGetRenderId") func layerRenderId(_ layer: CALayer) -> UInt64

/// What WebKit's name for an element's layer looks like: `… id='<id>' …`, plus " (anchor)" on
/// the companion layer it uses for transform-origin.
func isLayer(of id: String, _ l: CALayer, anchor: Bool) -> Bool {
  guard let n = l.name, n.contains("id='\(id)'") else { return false }
  return n.hasSuffix("(anchor)") == anchor
}

/// Layers by marker, found two ways: by WebKit's name (the element's id) and by anchorPointZ.
func scan() -> (byName: [Int: [CALayer]], byZ: [Int: [CALayer]], layers: Int, named: Int) {
  var byName: [Int: [CALayer]] = [:]
  var byZ: [Int: [CALayer]] = [:]
  var count = 0
  var named = 0
  func walk(_ l: CALayer) {
    count += 1
    if l.name != nil { named += 1 }
    for m in markers where isLayer(of: "m\(m)", l, anchor: false) { byName[m, default: []].append(l) }
    // Second signal: the container's opacity is 1 - (marker - 1000) / 100000, which no other
    // layer has and which renders exactly like 1.
    let k = Int(((1 - Double(l.opacity)) * 100_000).rounded())
    if l.opacity < 1, let m = markers.first(where: { $0 - 1000 == k }) {
      byZ[m, default: []].append(l)
    }
    for s in l.sublayers ?? [] { walk(s) }
  }
  walk(window.contentView!.layer!)
  return (byName, byZ, count, named)
}

/// Where the transform-origin z went, for the first marked container.
func describeMarked() {
  func walk(_ l: CALayer, _ depth: Int) {
    if let n = l.name, n.contains("id='m1001'") {
      print("  \(n.hasSuffix("(anchor)") ? "anchor layer" : "element layer"): parent \(l.superlayer?.name?.prefix(40) ?? "-"), "
        + "anchorPoint \(l.anchorPoint) z \(l.anchorPointZ), zPosition \(l.zPosition), "
        + "transform m43 \(l.transform.m43), sublayerTransform m43 \(l.sublayerTransform.m43), "
        + "sublayers \(l.sublayers?.count ?? 0), contents \(l.contents != nil)")
    }
    for s in l.sublayers ?? [] { walk(s, depth + 1) }
  }
  walk(window.contentView!.layer!, 0)
}

var last: [Int: UInt64] = [:]
var problems = 0
var replaced = 0

func report(_ name: String) {
  let (byName, byZ, count, named) = scan()
  var parts: [String] = []
  for m in markers {
    let ls = byName[m] ?? []
    var s = "\(m): "
    switch ls.count {
    case 0: s += "absent"
    case 1:
      let id = layerRenderId(ls[0])
      if let before = last[m], before != id {
        s += "REPLACED"
        replaced += 1
      } else {
        s += "same"
      }
      last[m] = id
    default:
      s += "AMBIGUOUS (\(ls.count))"
      problems += 1
    }
    if name == "load", let l = ls.first {
      print("  \(m) element layer opacity \(l.opacity)")
    }
    let bySize = byZ[m] ?? []
    if bySize.count == 1 && ls.count == 1 {
      s += bySize[0] === ls[0] ? ", opacity agrees" : ", OPACITY DISAGREES"
      if bySize[0] !== ls[0] { problems += 1 }
    } else if !(bySize.isEmpty && ls.isEmpty) {
      s += ", opacity finds \(bySize.count)"
      if bySize.count != ls.count { problems += 1 }
    }
    parts.append(s)
  }
  print("\(name.padding(toLength: 46, withPad: " ", startingAt: 0)) \(count) layers, \(named) named | " + parts.joined(separator: " | "))

}

var index = 0
func next() {
  web.evaluateJavaScript("step(\(index))") { result, _ in
    guard let name = result as? String else {
      print("\nproblems (ambiguous or signals disagree): \(problems), layer replaced (a rescan rebinds): \(replaced)")
      exit(0)
    }
    if name == "window resize" {
      window.setContentSize(NSSize(width: 900, height: 560))
    }
    index += 1
    // Let WebKit commit its layer tree (UI-side compositing) before looking.
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
      report(name)
      next()
    }
  }
}

DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { next() }
DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
  print("timed out")
  exit(1)
}
app.run()
