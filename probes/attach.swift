// Probe: UI attached to another app's window by z-order, not by clipping.
//
// A borderless, non-activating panel at the normal window level is ordered directly above (or
// below) a target window with `orderWindow(_:relativeTo:)`, which takes any window number, even
// another process's. The window server then does all occlusion: windows in front of the target
// cover the panel exactly (rounded corners, shadows, menus, sheets) with nothing computed by us.
//
// Run:  swift probes/attach.swift            (target: the window under the pointer after 3 s)
//       swift probes/attach.swift below      (the panel goes behind the target instead: a glow)
//
// What to try: move other windows over the target; drag the target; click the target so its app
// comes to the front (the panel has to be re-ordered, and you may see a frame where it isn't);
// switch Spaces. Ctrl-C to quit.

import AppKit

let below = CommandLine.arguments.contains("below")

/// On-screen windows, front to back, as the window server lists them.
func windowList() -> [[String: Any]] {
  CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]] ?? []
}

func bounds(_ info: [String: Any]) -> CGRect? {
  guard let dict = info[kCGWindowBounds as String] as? NSDictionary else { return nil }
  return CGRect(dictionaryRepresentation: dict)
}

func number(_ info: [String: Any]) -> Int {
  info[kCGWindowNumber as String] as? Int ?? 0
}

/// Global (top-left origin) to AppKit (bottom-left origin) coordinates.
func appKit(_ r: CGRect) -> NSRect {
  let h = NSScreen.screens.first?.frame.height ?? 0
  return NSRect(x: r.minX, y: h - r.maxY, width: r.width, height: r.height)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let panel = NSPanel(
  contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
  defer: false)
panel.level = .normal  // same level as app windows, or it would float above all of them
panel.isOpaque = false
panel.backgroundColor = .clear
panel.hasShadow = false
panel.ignoresMouseEvents = true
panel.collectionBehavior = [.managed, .fullScreenAuxiliary]

let view = NSView()
view.wantsLayer = true
panel.contentView = view

let label = NSTextField(labelWithString: "attached")
label.font = .systemFont(ofSize: 13, weight: .medium)
label.textColor = .black
label.wantsLayer = true
label.layer?.backgroundColor = NSColor(white: 0.9, alpha: 1).cgColor
label.layer?.cornerRadius = 6

if below {
  // Behind the window: a glow that only shows around its edges.
  view.layer?.backgroundColor = NSColor.systemPink.withAlphaComponent(0.55).cgColor
  view.layer?.cornerRadius = 24
} else {
  view.addSubview(label)
}

var target = 0
var reorders = 0

func follow() {
  let list = windowList()
  guard let index = list.firstIndex(where: { number($0) == target }), let r = bounds(list[index])
  else {
    panel.orderOut(nil)
    return
  }
  let frame = below ? r.insetBy(dx: -24, dy: -24) : CGRect(x: r.minX + 12, y: r.minY + 10, width: 150, height: 26)
  panel.setFrame(appKit(frame), display: true)
  label.frame = NSRect(x: 0, y: 0, width: frame.width, height: frame.height)
  label.stringValue = "  attached to \(list[index][kCGWindowOwnerName as String] as? String ?? "?")"

  // Re-order only when we're not already right next to the target (the list is front to back).
  let ours = panel.windowNumber
  let neighbour = below ? index + 1 : index - 1
  let placed = list.indices.contains(neighbour) && number(list[neighbour]) == ours
  if !placed {
    panel.order(below ? .below : .above, relativeTo: target)
    reorders += 1
    print("re-ordered (\(reorders))")
  }
}

print("Point at a window; attaching in 3 seconds...")
DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
  let mouse = NSEvent.mouseLocation
  let h = NSScreen.screens.first?.frame.height ?? 0
  let point = CGPoint(x: mouse.x, y: h - mouse.y)
  let pick = windowList().first {
    ($0[kCGWindowLayer as String] as? Int) == 0 && (bounds($0)?.contains(point) ?? false)
  }
  guard let pick else {
    print("No window under the pointer")
    exit(1)
  }
  target = number(pick)
  print("Attached \(below ? "below" : "above") window \(target) (\(pick[kCGWindowOwnerName as String] ?? "?"))")
  Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { _ in follow() }
  follow()
}

app.run()
