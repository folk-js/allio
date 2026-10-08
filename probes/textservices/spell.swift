// Probe: a system spell checker as a feed of other apps' text, and as an annotation channel back.
//
// Cocoa text views (and Chromium, which defers to NSSpellChecker on macOS) send text to whichever
// spell server is chosen for the language. Unified checking (`check:offset:types:...`) returns
// NSTextCheckingResults, and those are not only misspellings: grammar results carry a user-facing
// description that the host app shows in its own UI, and correction/replacement results may be
// applied automatically by apps that have those substitutions turned on.
//
// Questions this answers:
//   - Which apps send text here, how much (word, paragraph, document?), and how often?
//   - Do they use the unified `check` entry point or only `findMisspelledWord`? With which types?
//   - Does `offset` locate the chunk in the document?
//   - Are our grammar annotations drawn, with our message, in TextEdit / Notes / Mail / Chromium?
//   - Do correction/replacement results get applied (i.e. is this a write channel)?
//
// Build/install: see build.sh. Then System Settings > Keyboard > Text Input > Edit… > Spelling, pick
// "en (Allio)". Log: tail -f ~/Library/Logs/allio-spell.log
//
// Behaviour: flags "allio" as misspelled (suggests "Allio"), puts a grammar annotation on
// "substrate" with a custom message, and offers a correction "teh" -> "the".

import AppKit

let logURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/allio-spell.log")
FileManager.default.createFile(atPath: logURL.path, contents: nil)
let logHandle = try! FileHandle(forWritingTo: logURL)
logHandle.seekToEndOfFile()

func log(_ s: String) {
  let line = "[\(Date().formatted(date: .omitted, time: .standard))] \(s)\n"
  logHandle.write(line.data(using: .utf8)!)
}

/// Frontmost app at the time of the call: the spell server is not told who is asking.
func front() -> String { NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?" }

func preview(_ s: String) -> String {
  let flat = s.replacingOccurrences(of: "\n", with: "⏎")
  return flat.count > 160 ? String(flat.prefix(160)) + "…(\(s.count) chars)" : flat
}

func ranges(of word: String, in s: String) -> [NSRange] {
  let ns = s as NSString
  var out: [NSRange] = []
  var search = NSRange(location: 0, length: ns.length)
  while true {
    let r = ns.range(of: word, options: [.caseInsensitive], range: search)
    if r.location == NSNotFound { break }
    out.append(r)
    search = NSRange(location: NSMaxRange(r), length: ns.length - NSMaxRange(r))
  }
  return out
}

final class Delegate: NSObject, NSSpellServerDelegate {
  func spellServer(
    _ sender: NSSpellServer, findMisspelledWordIn stringToCheck: String, language: String,
    wordCount: UnsafeMutablePointer<Int>, countOnly: Bool
  ) -> NSRange {
    log("findMisspelled front=\(front()) lang=\(language) countOnly=\(countOnly) \"\(preview(stringToCheck))\"")
    wordCount.pointee = stringToCheck.split(whereSeparator: \.isWhitespace).count
    if countOnly { return NSRange(location: NSNotFound, length: 0) }
    return ranges(of: "allio", in: stringToCheck).first ?? NSRange(location: NSNotFound, length: 0)
  }

  func spellServer(_ sender: NSSpellServer, suggestGuessesForWord word: String, inLanguage language: String) -> [String]? {
    log("guesses front=\(front()) \"\(word)\"")
    return word.lowercased() == "allio" ? ["Allio"] : []
  }

  func spellServer(
    _ sender: NSSpellServer, check stringToCheck: String, offset: Int, types checkingTypes: NSTextCheckingTypes,
    options: [String: Any]? = nil, orthography: NSOrthography?, wordCount: UnsafeMutablePointer<Int>
  ) -> [NSTextCheckingResult]? {
    let opts = options?.keys.map { $0 }.sorted().joined(separator: ",") ?? ""
    log("check front=\(front()) offset=\(offset) types=\(checkingTypes) opts=[\(opts)] \"\(preview(stringToCheck))\"")
    wordCount.pointee = stringToCheck.split(whereSeparator: \.isWhitespace).count
    var results: [NSTextCheckingResult] = []
    for r in ranges(of: "allio", in: stringToCheck) {
      results.append(.spellCheckingResult(range: NSRange(location: r.location + offset, length: r.length)))
    }
    for r in ranges(of: "substrate", in: stringToCheck) {
      let range = NSRange(location: r.location + offset, length: r.length)
      let detail: [String: Any] = [
        NSGrammarRange: NSValue(range: NSRange(location: 0, length: r.length)),
        NSGrammarUserDescription: "allio was here: an annotation drawn by the host app",
        NSGrammarCorrections: ["material", "proto-substrate"],
      ]
      results.append(.grammarCheckingResult(range: range, details: [detail]))
    }
    for r in ranges(of: "teh", in: stringToCheck) {
      let range = NSRange(location: r.location + offset, length: r.length)
      results.append(.correctionCheckingResult(range: range, replacementString: "the", alternativeStrings: []))
    }
    return results
  }

  func spellServer(
    _ sender: NSSpellServer, checkGrammarIn stringToCheck: String, language: String?,
    details: AutoreleasingUnsafeMutablePointer<NSArray?>?
  ) -> NSRange {
    log("checkGrammar front=\(front()) \"\(preview(stringToCheck))\"")
    return NSRange(location: NSNotFound, length: 0)
  }

  func spellServer(_ sender: NSSpellServer, didLearnWord word: String, inLanguage language: String) {
    log("learned \"\(word)\"")
  }

  func spellServer(
    _ sender: NSSpellServer, recordResponse response: Int, toCorrection correction: String, forWord word: String,
    language: String
  ) {
    log("response \(response) to correction \"\(word)\" -> \"\(correction)\"")
  }
}

let delegate = Delegate()
let server = NSSpellServer()
server.delegate = delegate
guard server.registerLanguage("en", byVendor: "Allio") else {
  log("registerLanguage failed")
  exit(1)
}
log("spell server running (pid \(getpid()))")
server.run()
