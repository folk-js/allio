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
