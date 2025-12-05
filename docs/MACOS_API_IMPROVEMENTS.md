# macOS Accessibility API Improvements

This document outlines planned improvements to our macOS accessibility implementation, based on research into available APIs and patterns.

## Overview

| Improvement                      | Effort | Value  | Status             |
| -------------------------------- | ------ | ------ | ------------------ |
| Batch attribute fetching         | Low    | High   | Planned            |
| Action discovery                 | Low    | Medium | Planned            |
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

## 2. Action Discovery

### Problem

When an action fails, we give a generic error. Users don't know what actions ARE available.

### Solution

Expose available actions per element and improve error messages.

#### API Addition

```typescript
// New RPC method
axio.actions(elementId: string): Promise<string[]>
// Returns: ["AXPress", "AXShowMenu", "AXRaise", ...]
```

#### Improved Errors

```typescript
// Before
Error: Action failed

// After
Error: Action "AXIncrement" not supported for element "search-field-123"
       Available actions: ["AXPress", "AXConfirm", "AXCancel"]
```

#### Optional: Include in Element Data

```typescript
interface AXElement {
  // ... existing fields
  actions?: string[]; // Populated on demand or during discovery
}
```

### Implementation

1. Add `get_actions(element_id)` RPC handler
2. Use `AXUIElementCopyActionNames` to fetch available actions
3. Update error types to include available actions on failure
4. Optionally batch-fetch actions during discovery

### Available macOS Actions

```
kAXPressAction           - Click/activate
kAXIncrementAction       - Increase value (sliders, steppers)
kAXDecrementAction       - Decrease value
kAXConfirmAction         - Confirm/submit
kAXCancelAction          - Cancel operation
kAXShowMenuAction        - Open context menu
kAXRaiseAction           - Bring window to front
kAXPickAction            - Pick from list/menu
```

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
│ System-wide observer for:                                       │
│   • AXFocusedWindowChanged → know which window is active        │
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
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ TIER 3: Explicit Watch (user-requested, current behavior)       │
├─────────────────────────────────────────────────────────────────┤
│ axio.watch(elementId) → persistent subscription                 │
│ Used for: ports demo, tracking specific elements                │
│ User manages lifecycle via unwatch()                            │
└─────────────────────────────────────────────────────────────────┘
```

### New Events

```typescript
// Tier 1 events (always available)
"focus:element" → {
  windowId: string,
  elementId: string,
  element: AXElement,
  previousElementId?: string
}

"selection:text" → {
  windowId: string,
  text: string,
  elementId?: string,
  range?: { start: number, length: number }
}

// Existing events (unchanged)
"element:changed" → { element: AXElement }  // From Tier 2 auto-watch or Tier 3 explicit
```

### Cost Analysis

| Tier   | Observers        | Subscriptions          | When      |
| ------ | ---------------- | ---------------------- | --------- |
| Tier 1 | 1 per app        | 2 per app              | Always    |
| Tier 2 | (reuse Tier 1)   | +2 for focused element | Auto      |
| Tier 3 | (reuse existing) | N per watched element  | On demand |

**Total overhead**: ~2 extra notifications per tracked app. Essentially free.

### Client State Updates

```typescript
class AXIO {
  // New state
  focusedElement: AXElement | null = null;
  selectedText: string = "";

  // Existing (unchanged)
  windows: Map<string, AXWindow>;
  elements: Map<string, AXElement>;
  activeWindow: string | null;
  focusedWindow: string | null;
}
```

### Implementation Steps

1. **Add app-level observer setup**
   - When first window of an app is tracked, create app-level observer
   - Subscribe to `AXFocusedUIElementChanged` and `AXSelectedTextChanged`
2. **Handle focus change notifications**
   - On focus change: emit `focus:element`, update `focusedElement`
   - If new focus is text element: auto-watch for `ValueChanged`
   - If old focus was text element: auto-unwatch
3. **Handle selection change notifications**
   - Read `AXSelectedText` and `AXSelectedTextRange`
   - Emit `selection:text` event
4. **Update client to track new state**

---

## 4. Selected Text Tracking

### Behavior

- Track the currently selected text across all applications
- Only ONE selection exists system-wide at a time
- Fires when user selects text in any tracked app

### API

```typescript
// State
axio.selectedText: string  // Current selection (empty if none)

// Event
"selection:text" → {
  windowId: string,
  text: string,
  elementId?: string,  // Element containing selection, if known
  range?: { start: number, length: number }
}
```

### Use Cases

- Show selected text in overlay
- "Define", "Search", "Translate" actions on selection
- Copy-paste assistance
- Context-aware AI features

### Implementation

Part of Tier 1 observers - subscribe to `AXSelectedTextChanged` at app level.

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

// Cross-platform action enum
enum Action {
  Click, // macOS: AXPress, Windows: Invoke
  Focus, // macOS: AXRaise, Windows: SetFocus
  Expand, // macOS: AXPress, Windows: Expand
  Collapse,
  ScrollIntoView,
  ShowMenu, // macOS: AXShowMenu
  Increment, // macOS: AXIncrement, Windows: RangeValue
  Decrement,
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
2. **Action discovery** - Small API addition, better DX
3. **Tier 1: App-level observers** - Foundation for live tracking
4. **Selection tracking** - Builds on Tier 1
5. **Tier 2: Auto-watch** - Smart defaults for text fields

---

## Appendix: macOS Notification Reference

### Notifications We Use Now

- `AXValueChanged` - Element value changed (text fields)
- `AXTitleChanged` - Element title changed (windows)
- `AXUIElementDestroyed` - Element removed from UI

### Notifications for Live Tracking

- `AXFocusedUIElementChanged` - Focus moved to new element
- `AXSelectedTextChanged` - Text selection changed

### Other Available Notifications

```
AXFocusedWindowChangedNotification   - Window focus changed
AXSelectedChildrenChangedNotification - Selection in list/table
AXSelectedCellsChangedNotification   - Table cell selection
AXSelectedRowsChangedNotification    - Table row selection
AXResizedNotification                - Element resized
AXMovedNotification                  - Element moved
AXLayoutChangedNotification          - Layout reflow
AXMenuOpenedNotification             - Menu appeared
AXMenuClosedNotification             - Menu closed
AXCreatedNotification                - New element (unreliable)
AXAnnouncementRequestedNotification  - Screen reader announcement
```

### System-Wide Element Limitations

`AXUIElementCreateSystemWide()` only supports:

- `AXFocusedUIElementChanged`
- `AXFocusedWindowChanged`

All other notifications require subscribing to specific elements.
