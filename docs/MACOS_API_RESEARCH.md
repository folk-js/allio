# macOS Accessibility API Research

## CFHash and CFEqual for Element Identity

### Finding

`CFHash` returns a hash based on **local data** within the `AXUIElement` struct - no IPC involved.

`CFEqual` also operates on **local data** - it compares internal tokens, not remote state.

### Implication

We can use `ElementHandle` as a HashMap key with `Hash` and `Eq` implemented:

- `Hash`: Use cached `CFHash` value (computed once at construction)
- `Eq`: Fast path compares hashes, collision resolution uses `CFEqual` (still local, no IPC)

This replaced the fragile `hash_to_element: HashMap<u64, ElementId>` index with a robust `handle_to_id: HashMap<Handle, ElementId>`.

---

## AXUIElement Internal Structure

### Finding

An `AXUIElement` contains:

1. **PID** - Process ID of the owning application
2. **Internal token** - Unique identifier within that process

The (PID, token) pair uniquely identifies an element. This is what `CFEqual` compares.

### Implication

- Same hash can appear in different processes (different elements)
- Same hash can appear for different elements in the same process (rare but possible)
- `CFEqual` resolves both cases correctly without IPC

---

## AXUIElementGetPid

### Finding

`AXUIElementGetPid` extracts the PID from local `AXUIElement` data - no IPC.

### Implication

We can cache PID at `ElementHandle` construction time and use it freely.

---

## Window Lookup Attributes

### Finding

Two attributes provide O(1) window lookup from any element:

| Attribute                         | Returns                                                                         |
| --------------------------------- | ------------------------------------------------------------------------------- |
| `kAXWindowAttribute` (`AXWindow`) | The containing window element (role = `AXWindowRole`)                           |
| `kAXTopLevelUIElementAttribute`   | Window, sheet, or drawer (roles: `AXWindowRole`, `AXSheetRole`, `AXDrawerRole`) |

### Usage

```swift
var windowRef: CFTypeRef?
AXUIElementCopyAttributeValue(element, kAXWindowAttribute as CFString, &windowRef)
```

### Implication

Window ID can be derived from any element handle via one FFI call - no parent chain walking needed. This is more reliable than the current fallback approach.

**Status**: Not yet implemented. See FUTURE_CLEANUPS.md.

---

## Destruction Notification Reliability

### Finding

`NSAccessibilityUIElementDestroyedNotification` is **NOT fully reliable**:

- Not all apps properly post it
- Crashes won't send it
- Some dynamic UI doesn't notify

### Implication

Polling provides necessary backup. Could add periodic "liveness checks" (call cheap attribute, treat failure as destruction).

---

## Screen Configuration

### Finding

Display changes can be detected via:

- `NSApplicationDidChangeScreenParametersNotification` (AppKit)
- Core Graphics display reconfiguration callbacks

### Implication

Currently we cache screen size with `OnceLock` assuming it never changes. Could add listener to invalidate cache on display changes.

**Status**: Low priority.

---

## Writability

Findings from `axprobe` experiments (raw output in `probes/`) and reading the AppKit,
WebKit and Chromium implementations. Writes here are direct AX operations only: no
focus-stealing, selection tricks, synthetic input, or pressing UI open.

### Finding: `AXError::Success` does not mean a write happened

AppKit returns `Success` for `AXUIElementSetAttributeValue` on attributes it doesn't
let you set (TextEdit `NSColorWell` `AXValue`: success, no change, any CF type).
`AXUIElementIsAttributeSettable` was accurate in every case tested.

**Implication**: judge writability by settability (per element, from the app), never by
role tables or the set's return code.

### Finding: `AXReplaceRangeWithText` — range edits without focus

Undocumented parameterized attribute (AppKit selector `accessibilityReplaceRange:withText:`).
Parameter is a `CFDictionary`:

```text
{ "AXReplacementRange": AXValue(CFRange), "AXReplacementText": CFString }  →  CFBoolean
```

Keys found next to the attribute name in AppKit's constant table (dyld shared cache strings).
Every AppKit element *advertises* it; it only does something where implemented:

| Implementation | Works? |
|---|---|
| AppKit `NSTextView` (TextEdit) | Yes — verified, preserves surrounding styling, adjusts caret |
| WebKit (Safari, `WKWebView` apps): inputs, textareas, contenteditable | Yes per source (`AccessibilityObject::replaceTextInRange`) |
| Chromium / Electron | No (not implemented) |
| Numbers cells | No (returns `false`) |

Replacement text must be a plain `CFString`: attributed strings are rejected (`-25201`),
so styled writes aren't possible this way. CGColor can't be sent as a set value at all.

### Finding: what each engine routes from AX writes

**AppKit** (`NSAccessibility` setters): `AXValue` where the control implements a setter
(text fields/views, sliders…; not `NSColorWell`), `AXSelected`, `AXSelectedRows`/
`Columns`/`Cells`/`Children`, `AXDisclosing`/`AXExpanded`, `AXFocused`, selected text
range(s), window `AXPosition`/`AXSize`/`AXMain`/`AXMinimized`/`AXFullScreen`.

**WebKit** (`WebAccessibilityObjectWrapperMac.mm` `_accessibilitySetValue:forAttribute:`):
`AXValue` string (text inputs, textareas, editable via `Editor::insertText`) or number
(sliders, progress), `AXSelected`, `AXSelectedChildren`, `AXSelectedRows` (trees/tables),
`AXExpanded`/`AXDisclosing`, `AXARIAGrabbed`, selected text marker range. Plus
`accessibilityReplaceRange:withText:` and `AXTextOperation` (multi-range replace/case ops
over text marker ranges; keys `AXTextOperationMarkerRanges`, `AXTextOperationType`
= `TextOperationReplace|ReplacePreserveCase|Capitalize|Lowercase|Uppercase|Select`,
`AXTextOperationReplacementString`, `AXTextOperationIndividualReplacementStrings`).

**Chromium/Electron** (`ax_platform_node_cocoa.mm`, Blink `OnNativeSetValueAction`):
`AXValue` → `kSetValue` on `<input>` text fields, `<textarea>` (dispatching `input` +
`change` events, so frameworks see it), `contenteditable` (`setInnerText` — flattens
rich content), sliders; `AXSelectedText` → `kReplaceSelectedText`; focus; selected text
range. `AXTextOperation` exists but is behind `kMacAccessibilityTextOperation`
(disabled by default). No range replace.

### Finding: actions are a write channel

Standard: `AXIncrement`/`AXDecrement` (adjustables), `AXConfirm`, `AXPick`, `AXDelete`,
`AXOpen`, `AXCancel`. **Custom actions** are app-declared semantic operations (SwiftUI
`.accessibilityAction(named:)`, UIKit custom actions); they appear in the action list as
`"Name:<label>\nTarget:…\nSelector:…"` and are performed by that full string. Reminders
rows expose Delete, Flag, Details, Indent, Move Up/Down/To Top/To Bottom.

### Finding: AppKit constants worth exploring

From AppKit's constant table: `AXAllowedValues`, `AXLabelValue`, `AXDateTimeComponents`,
`AXUserInputLabels`, `AXEdited`, `AXAttributedValueForStringAttribute` (param),
`AXResultsForSearchPredicate` (param; native tree search with `AXSearchKey`,
`AXStartElement`, `AXDirection`, `AXResultsLimit`, `AXSearchText`), `AXHighlightTextRanges`.

### Finding: color wells are read-only; the Colors panel is writable

Tested in-process (no IPC): `NSColorWell` in every style (`default`, `minimal`,
`expanded`) reports `AXValue` not settable and ignores `accessibilitySetValue:` with a
string *or an `NSColor`*; its only action is `AXPress`. SwiftUI `ColorPicker` wraps it;
WebKit/Chromium `<input type=color>` aren't settable either.

The shared `NSColorPanel` *is* AX-writable: RGB/HSB sliders take numeric `AXValue`
directly; the component and "Hex Color #" text fields take a string followed by
`AXConfirm` (no change without the confirm). The panel then applies the color to the
active well / selected text as usual. Requires the panel to be open.

### Finding: some text fields only commit on `AXConfirm`

Reminders (Catalyst) title fields accept `AXValue` and show it, but the change does not
survive relaunch when the field isn't being edited. Fields advertise `AXConfirm`.

### Open

- Allio does not use Apple Events / AppleScript (not Apple-oriented by design), so apps
  whose AX writes are closed (Numbers cells) stay read-only for now.
- SwiftUI / Catalyst setter coverage is undocumented; needs probing per control.
