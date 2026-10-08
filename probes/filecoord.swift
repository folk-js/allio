// Probe: talking to document apps through file coordination instead of their UI.
//
// NSFileCoordinator is system-wide: every NSDocument app registers its open documents as file
// presenters with filecoordinationd. A coordinated *read* from any process first asks every other
// presenter of that file to save unsaved changes (`savePresentedItemChanges`). A coordinated
// *write* makes them relinquish the file and then tells them it changed, and NSDocument reverts to
// the new contents (silently if it had no unsaved edits, with a prompt if it did).
//
// Run:  swift probes/filecoord.swift read  <path>          coordinated read: flush the app's edits, print the file
//       swift probes/filecoord.swift write <path> <text>   coordinated write: replace the file, app should reload
//       swift probes/filecoord.swift watch <path>          be a presenter: log what other processes do to the file
//
// What to try (TextEdit, plain text mode):
//   printf 'hello\n' > /tmp/fc.txt && open -a TextEdit /tmp/fc.txt
//   type into the window without saving, then `read /tmp/fc.txt`: does the output have your edits?
//   `write /tmp/fc.txt "from outside"`: does the window update? With unsaved edits, what does it ask?
//   run `watch` in another terminal while doing the above, and while editing/saving in TextEdit.
// The path for an open document can come from AX: windows have AXDocument (a file URL).

import Foundation

setvbuf(stdout, nil, _IOLBF, 0)
let args = CommandLine.arguments
guard args.count >= 3 else {
  print("usage: filecoord read|write|watch <path> [text]")
  exit(2)
}
let url = URL(fileURLWithPath: args[2]).standardizedFileURL

func stamp(_ url: URL) -> String {
  let attrs = try? FileManager.default.attributesOfItem(atPath: url.path)
  let date = attrs?[.modificationDate] as? Date
  let size = attrs?[.size] as? Int
  return "mtime=\(date.map { "\($0.timeIntervalSince1970)" } ?? "-") size=\(size ?? -1)"
}

func time(_ label: String, _ body: () -> Void) {
  let start = Date()
  body()
  print("\(label) took \(Int(Date().timeIntervalSince(start) * 1000)) ms")
}

switch args[1] {
case "read":
  print("before: \(stamp(url))")
  var error: NSError?
  time("coordinated read") {
    NSFileCoordinator(filePresenter: nil).coordinate(readingItemAt: url, options: [], error: &error) { url in
      print("after:  \(stamp(url))")
      print("---")
      print((try? String(contentsOf: url, encoding: .utf8)) ?? "<not utf8 text; \(stamp(url))>")
      print("---")
    }
  }
  if let error { print("error: \(error)") }

case "write":
  let text = args.count > 3 ? args[3] : "written by filecoord at \(Date())\n"
  var error: NSError?
  time("coordinated write") {
    NSFileCoordinator(filePresenter: nil).coordinate(writingItemAt: url, options: [], error: &error) { url in
      do { try text.write(to: url, atomically: false, encoding: .utf8) } catch { print("write failed: \(error)") }
    }
  }
  if let error { print("error: \(error)") }
  print("now: \(stamp(url))")

case "watch":
  final class Presenter: NSObject, NSFilePresenter {
    let presentedItemURL: URL?
    let presentedItemOperationQueue = OperationQueue()
    init(_ url: URL) { presentedItemURL = url }

    func log(_ s: String) { print("[\(Date().formatted(date: .omitted, time: .standard))] \(s)") }

    // Someone wants to read: we'd save our edits here. Logging shows when readers coordinate.
    func savePresentedItemChanges(completionHandler: @escaping @Sendable (Error?) -> Void) {
      log("asked to save (someone is coordinating a read or write)")
      completionHandler(nil)
    }
    func relinquishPresentedItem(toReader reader: @escaping @Sendable ((@Sendable () -> Void)?) -> Void) {
      log("relinquish to reader")
      reader { self.log("reader done") }
    }
    func relinquishPresentedItem(toWriter writer: @escaping @Sendable ((@Sendable () -> Void)?) -> Void) {
      log("relinquish to writer")
      writer { self.log("writer done") }
    }
    func presentedItemDidChange() { log("did change: \(stamp(presentedItemURL!))") }
    func presentedItemDidMove(to newURL: URL) { log("moved to \(newURL.path)") }
    func presentedItemDidGain(_ version: NSFileVersion) { log("gained version \(version)") }
    func presentedItemDidLose(_ version: NSFileVersion) { log("lost version \(version)") }
    func accommodatePresentedItemDeletion(completionHandler: @escaping @Sendable (Error?) -> Void) {
      log("about to be deleted")
      completionHandler(nil)
    }
  }
  let presenter = Presenter(url)
  NSFileCoordinator.addFilePresenter(presenter)
  print("presenting \(url.path) (\(stamp(url))); Ctrl-C to quit")
  RunLoop.main.run()

default:
  print("unknown command \(args[1])")
  exit(2)
}
