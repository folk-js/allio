# macOS Accessibility API Improvements

This document outlines planned improvements to our macOS accessibility implementation, based on research into available APIs and patterns.

## Overview

| Improvement                      | Effort | Value  | Status             |
| -------------------------------- | ------ | ------ | ------------------ |
| Batch attribute fetching         | Low    | High   | Planned            |
| Actions on elements              | Low    | Medium | Planned            |
| Live tracking (tiered observers) | Medium | High   | Planned            |
| Selected text tracking           | Medium | Medium | Planned            |
| AccessKit read+write spec        | High   | High   | Future exploration |

---

## 1. Batch Attribute Fetching

### Problem

Currently, `discover_children` fetches ~10 attributes per element with individual IPC calls:

```rust
let role = element.attribute(kAXRoleAttribute)?;    // IPC call
let title = element.attribute(kAXTitleAttribute)?;  // IPC call
let value = element.attribute(kAXValueAttribute)?;  // IPC call
// ... 7 more IPC calls
```

### Solution

Use `AXUIElementCopyMultipleAttributeValues` to batch:

```rust
let attrs = [
    kAXRoleAttribute,
    kAXTitleAttribute,
    kAXValueAttribute,
    kAXDescriptionAttribute,
    kAXPositionAttribute,
    kAXSizeAttribute,
    kAXChildrenAttribute,
    kAXEnabledAttribute,
    kAXFocusedAttribute,
    kAXActionsAttribute,  // Include actions in batch!
];
let values = element.copy_multiple_attribute_values(&attrs, CopyMultipleAttributeOptions::StopOnError)?;
// 1 IPC call for all attributes
```

### Expected Impact

- **10x fewer IPC calls** per element during discovery
- Noticeable speedup for large UI trees
- No API changes needed (internal optimization)

### Implementation

1. Add helper function `batch_fetch_attributes(element, attrs) -> HashMap<String, CFType>`
2. Update `build_element()` to use batch fetching
3. Handle partial failures gracefully (some attributes may not exist)

---

## 2. Actions on Elements

### Problem

When an action fails, we give a generic error. Users don't know what actions ARE available.

### Solution

Always include available actions in element data. The list is typically short (1-3 actions), so fetching is cheap and we get it "for free" with batch fetching.

### Type Design

Map macOS actions to our own platform-agnostic enum (like we do with `AXRole`):

```rust
// Rust: Platform-agnostic action enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum AXAction {
    Press,       // macOS: kAXPressAction, Windows: Invoke
    ShowMenu,    // macOS: kAXShowMenuAction
    Increment,   // macOS: kAXIncrementAction, Windows: RangeValue.Increment
    Decrement,   // macOS: kAXDecrementAction, Windows: RangeValue.Decrement
    Confirm,     // macOS: kAXConfirmAction
    Cancel,      // macOS: kAXCancelAction
    Raise,       // macOS: kAXRaiseAction, Windows: SetFocus
    Pick,        // macOS: kAXPickAction
}

impl AXAction {
    /// Map from macOS action string to our enum
    pub fn from_macos(s: &str) -> Option<Self> {
        match s {
            "AXPress" => Some(Self::Press),
            "AXShowMenu" => Some(Self::ShowMenu),
            "AXIncrement" => Some(Self::Increment),
            "AXDecrement" => Some(Self::Decrement),
            "AXConfirm" => Some(Self::Confirm),
            "AXCancel" => Some(Self::Cancel),
            "AXRaise" => Some(Self::Raise),
            "AXPick" => Some(Self::Pick),
            _ => None,  // Unknown actions ignored
        }
    }

    /// Map to macOS action string for performing
    pub fn to_macos(&self) -> &'static str {
        match self {
            Self::Press => "AXPress",
            Self::ShowMenu => "AXShowMenu",
            Self::Increment => "AXIncrement",
            Self::Decrement => "AXDecrement",
            Self::Confirm => "AXConfirm",
            Self::Cancel => "AXCancel",
            Self::Raise => "AXRaise",
            Self::Pick => "AXPick",
        }
    }
}
```

### Element Data

```typescript
interface AXElement {
  // ... existing fields
  actions: AXAction[]; // Always present, may be empty
}

type AXAction =
  | "Press"
  | "ShowMenu"
  | "Increment"
  | "Decrement"
  | "Confirm"
  | "Cancel"
  | "Raise"
  | "Pick";
```

### Improved Errors

```typescript
// Before
Error: Action failed

// After
Error: Action "Increment" not supported for element "search-field-123"
       Available actions: ["Press", "Confirm", "Cancel"]
```

### Implementation

1. Add `AXAction` enum to `types.rs` with macOS mapping in `platform/macos.rs`
2. Batch-fetch actions with other attributes during discovery
3. Add `actions: Vec<AXAction>` to `AXElement`
4. Update `perform_action()` to include available actions in errors

---

## 3. Live Tracking (Tiered Observers)

### Problem

Current approach requires explicit `watch()` calls for each element. Users must know what to watch upfront.

### Insight

macOS supports **app-level notifications** that fire when ANY element in that app changes focus. This is essentially "free" global awareness.

### Solution: Three-Tier Observer Strategy

```
┌─────────────────────────────────────────────────────────────────┐
│ TIER 1: Global Awareness (always on, near-zero cost)            │
├─────────────────────────────────────────────────────────────────┤
│ Per-app observer for:                                           │
│   • AXFocusedUIElementChanged → know what element is focused    │
│   • AXSelectedTextChanged → know when text selection changes    │
│                                                                 │
│ Note: Focused WINDOW comes from existing polling loop.          │
│ (Future: could replace polling with events for initial state)   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ TIER 2: Auto-Watch on Focus (smart, automatic)                  │
├─────────────────────────────────────────────────────────────────┤
│ When a text element gains focus:                                │
│   → Auto-subscribe to AXValueChanged                            │
│   → Auto-unsubscribe when focus leaves                          │
│                                                                 │
│ Roles: TextField, TextArea, ComboBox, SearchField               │
│ Result: "Active" text field is always watched                   │
│                                                                 │
│ Why this matters: The focused element is often the one being    │
│ edited, so auto-watching it gives you "live" updates for free!  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ TIER 3: Explicit Watch (user-requested, current behavior)       │
├─────────────────────────────────────────────────────────────────┤
│ axio.watch(elementId) → persistent subscription                 │
│ Used for: ports demo, tracking specific non-focused elements    │
│ User manages lifecycle via unwatch()                            │
└─────────────────────────────────────────────────────────────────┘
```

### Events

We add two new events for Tier 1 awareness. These follow our existing `type:action` naming pattern:

```typescript
// Focus changed to a new element
"focus:element" → {
  windowId: string;
  elementId: string;
  element: AXElement;        // Full element data
  previous?: {               // Previous focus (if known)
    elementId: string;
    element?: AXElement;
  };
}

// Text selection changed
"selection:changed" → {
  windowId: string;
  elementId: string;         // Element containing the selection
  text: string;              // The actual selected text
  range: {                   // Position within the element's text
    start: number;
    length: number;
  };
}
```

Note: `element:changed` continues to fire for watched elements (Tier 2 auto-watched or Tier 3 explicit).

### Client State

```typescript
class AXIO {
  // New state for Tier 1
  focusedElement: AXElement | null = null;
  selection: {
    text: string;
    elementId: string;
    range: { start: number; length: number };
  } | null = null;

  // Existing (unchanged)
  windows: Map<string, AXWindow>;
  elements: Map<string, AXElement>;
  activeWindow: string | null;
  focusedWindow: string | null; // From polling, not new observer
}
```

### Cost Analysis

| Tier   | Observers        | Subscriptions          | When      |
| ------ | ---------------- | ---------------------- | --------- |
| Tier 1 | 1 per app        | 2 per app              | Always    |
| Tier 2 | (reuse Tier 1)   | +2 for focused element | Auto      |
| Tier 3 | (reuse existing) | N per watched element  | On demand |

**Total overhead**: ~2 extra notifications per tracked app. Essentially free.

### Implementation Steps

1. **Add app-level observer setup**

   - When first window of an app is tracked, create app-level observer
   - Subscribe to `AXFocusedUIElementChanged` and `AXSelectedTextChanged` on the app element

2. **Handle focus change notifications**

   - On focus change callback: build element, emit `focus:element`
   - If new focus is text element: auto-subscribe to `ValueChanged`
   - If previous focus was text element: auto-unsubscribe

3. **Handle selection change notifications**

   - Read `AXSelectedText` attribute (the actual string)
   - Read `AXSelectedTextRange` attribute (start + length)
   - Emit `selection:changed` event

4. **Update client to track new state**
   - Add `focusedElement` and `selection` properties
   - Update on corresponding events

### Future: Event-Driven Window Tracking

Currently we poll for window changes via `x-win`. In the future, we could:

- Poll once on startup for initial state
- Use `AXWindowCreated`, `AXUIElementDestroyed`, etc. for updates

This would make window tracking fully event-driven. Deferred for now since polling works well enough.

---

## 4. Selected Text Tracking

(Covered in Tier 1 of Live Tracking above)

### Summary

- **State**: `axio.selection: { text, elementId, range } | null`
- **Event**: `selection:changed` with full text and range
- **Scope**: System-wide (only ONE selection exists at a time)

### Use Cases

- Show selected text in overlay
- "Define", "Search", "Translate" actions on selection
- Copy-paste assistance
- Context-aware AI features

---

## 5. Future: AccessKit Read+Write Spec

### Context

AccessKit provides cross-platform accessibility types but is **read-only** (for exposing UI to assistive tech). No standard exists for **consuming** accessibility across platforms.

### Potential Approach

Extend AccessKit's schema with write operations:

```typescript
// AccessKit-style (read-only)
interface Node {
  role: Role;
  name?: string;
  value?: string;
}

// Extended for read+write
interface WritableNode extends Node {
  writable: boolean;
  actions: Action[];

  setValue?(value: string): Promise<void>;
  performAction?(action: Action): Promise<void>;
}
```

### Challenges

- Different platforms have different capabilities
- Windows uses "patterns" (ValuePattern, InvokePattern) not actions
- Linux AT-SPI is similar to macOS but less mature

### Status

Deferred for future exploration. Would benefit the broader ecosystem.

---

## Implementation Order

1. **Batch attribute fetching** - Pure optimization, no API changes
2. **Actions on elements** - Add `AXAction` enum, include in element data
3. **Tier 1: App-level observers** - Foundation for live tracking
4. **Selection tracking** - Builds on Tier 1, add `selection:changed` event
5. **Tier 2: Auto-watch** - Smart defaults for focused text fields

---

## Appendix: macOS Notification Reference

### Notifications We Use Now

- `AXValueChanged` - Element value changed (text fields)
- `AXTitleChanged` - Element title changed (windows)
- `AXUIElementDestroyed` - Element removed from UI

### Notifications for Live Tracking (Tier 1)

- `AXFocusedUIElementChanged` - Focus moved to new element (on app element)
- `AXSelectedTextChanged` - Text selection changed (on app element)

### Other Available Notifications

```
AXFocusedWindowChangedNotification    - Window focus changed (we use polling instead)
AXSelectedChildrenChangedNotification - Selection in list/table
AXSelectedCellsChangedNotification    - Table cell selection
AXSelectedRowsChangedNotification     - Table row selection
AXResizedNotification                 - Element resized
AXMovedNotification                   - Element moved
AXLayoutChangedNotification           - Layout reflow
AXMenuOpenedNotification              - Menu appeared
AXMenuClosedNotification              - Menu closed
AXCreatedNotification                 - New element (unreliable)
AXAnnouncementRequestedNotification   - Screen reader announcement
```

### System-Wide Element Limitations

`AXUIElementCreateSystemWide()` only supports:

- `AXFocusedUIElementChanged`
- `AXFocusedWindowChanged`

We don't use system-wide element - focused window comes from polling, focused element comes from per-app observers.
