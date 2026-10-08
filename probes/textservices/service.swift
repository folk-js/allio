// Probe: the Services menu as a structured, clipboard-free channel into another app's selection.
//
// When a Service is invoked, the requesting app serialises its selection onto a *private*
// pasteboard in whichever of our NSSendTypes it can produce, and if the service has NSReturnTypes
// it replaces the selection with what we put back. The user's clipboard is untouched. Services
// live in every app's menu bar (app menu > Services), so they can be pressed through AX like any
// other menu item: no keystrokes, no focus stealing beyond what the selection already implies.
//
// Questions this answers:
//   - Which pasteboard types does each app hand over for a selection (RTF? HTML? images? files?)
//   - Does the replacement keep formatting when we return RTF?
//   - Which apps have working Services at all (Chromium/Electron? Catalyst? SwiftUI TextEditor?)
//   - Can allio trigger it with AXPress on the app's Services submenu item, unopened?
//
// Two items: "Allio: Inspect Selection" (read only) and "Allio: Uppercase Selection" (round trip,
// keeps attributes when RTF is offered). Build/install: see build.sh.
// Log: tail -f ~/Library/Logs/allio-service.log

import AppKit

let logURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/allio-service.log")
FileManager.default.createFile(atPath: logURL.path, contents: nil)
let logHandle = try! FileHandle(forWritingTo: logURL)
logHandle.seekToEndOfFile()

func log(_ s: String) {
  let line = "[\(Date().formatted(date: .omitted, time: .standard))] \(s)\n"
  logHandle.write(line.data(using: .utf8)!)
}

func describe(_ pboard: NSPasteboard) {
  log("front=\(NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?") pasteboard=\(pboard.name.rawValue)")
  for type in pboard.types ?? [] {
    let data = pboard.data(forType: type)
    var line = "  \(type.rawValue) \(data?.count ?? 0) bytes"
    if let s = pboard.string(forType: type), type == .string || type == .html {
      line += " \"\(s.prefix(200).replacingOccurrences(of: "\n", with: "⏎"))\""
    }
    log(line)
  }
}

final class Provider: NSObject {
  @objc(inspect:userData:error:)
  func inspect(_ pboard: NSPasteboard, userData: String?, error: AutoreleasingUnsafeMutablePointer<NSString?>) {
    log("inspect")
    describe(pboard)
  }

  @objc(uppercase:userData:error:)
  func uppercase(_ pboard: NSPasteboard, userData: String?, error: AutoreleasingUnsafeMutablePointer<NSString?>) {
    log("uppercase")
    describe(pboard)
    if let rtf = pboard.data(forType: .rtf),
      let attributed = try? NSMutableAttributedString(data: rtf, options: [.documentType: NSAttributedString.DocumentType.rtf], documentAttributes: nil)
    {
      let whole = NSRange(location: 0, length: attributed.length)
      attributed.enumerateAttributes(in: whole) { _, range, _ in
        attributed.replaceCharacters(in: range, with: attributed.attributedSubstring(from: range).string.uppercased())
      }
      let out = try? attributed.data(from: whole, documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf])
      pboard.clearContents()
      pboard.setData(out, forType: .rtf)
      pboard.setString(attributed.string, forType: .string)
      log("  returned rtf + string")
    } else if let s = pboard.string(forType: .string) {
      pboard.clearContents()
      pboard.setString(s.uppercased(), forType: .string)
      log("  returned string")
    } else {
      error.pointee = "no text on the pasteboard"
    }
  }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let provider = Provider()
app.servicesProvider = provider
NSUpdateDynamicServices()
log("service provider running (pid \(getpid()))")
app.run()
