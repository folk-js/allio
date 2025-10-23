# Cleanup Summary: Window Type Separation

## Changes Made

### Overview

Separated `Window` from `AXNode` by removing the fake conversion that treated windows as accessibility nodes. Windows and accessibility nodes are now properly distinct types with their own purposes.

### Rust Changes

#### `src-tauri/src/windows.rs`

- **Removed**: `WindowInfo::to_ax_node()` method (56 lines removed)
  - This was faking windows as AXNodes with empty paths and synthetic children_count
- **Removed**: Unused imports (`AXNode`, `AXRole`, `Bounds`, `Position`, `Size`)
- **Updated**: `WindowUpdatePayload` now contains `Vec<WindowInfo>` instead of `Vec<AXNode>`
- **Simplified**: `window_polling_loop()` no longer converts windows to AXNodes before broadcasting

#### `src-tauri/src/websocket.rs`

- **Updated**: Initial window state broadcast now sends `WindowInfo` directly
- **Removed**: Conversion logic from `WindowInfo` to `AXNode` (filter_map call removed)

### TypeScript Changes

#### `src-web/src/axio.ts`

- **Added**: New `Window` interface with proper window metadata:
  ```typescript
  export interface Window {
    readonly id: string; // System window ID
    readonly title: string; // Window title
    readonly app_name: string; // Application name
    readonly x: number; // X position
    readonly y: number; // Y position
    readonly w: number; // Width
    readonly h: number; // Height
    readonly focused: boolean; // Is this window focused?
    readonly process_id: number; // PID for accessing accessibility tree
  }
  ```
- **Updated**: `AXIO.windows` type changed from `AXNode[]` to `Window[]`
- **Updated**: `AXIO.focused` type changed from `AXNode | null` to `Window | null`
- **Updated**: `onWindowUpdate()` callback now receives `Window[]` instead of `AXNode[]`
- **Removed**: Code that tried to attach `setValue()` and `getChildren()` methods to windows (they're not nodes)
- **Simplified**: Window update handling in `handleMessage()` (no more method attachment)

#### `src-web/src/windows-debug.ts`

- **Updated**: Import changed from `AXNode` to `Window`
- **Removed**: `formatValue()` function (no longer needed)
- **Updated**: `renderWindows()` function signature and implementation
  - Now displays window-specific properties: `id`, `app_name`, `process_id`, `focused`
  - Shows position as `(x, y)` and size as `w × h`
  - Removed accessibility-specific properties: `role`, `path`, `pid`, `value`, `description`, `subrole`, `enabled`, `selected`, `bounds`, `children_count`

#### `src-web/src/sand.ts`

- **Updated**: `#collectShapeData()` method
  - Changed from `win.bounds.position.x/y` to `win.x/y`
  - Changed from `win.bounds.size.width/height` to `win.w/h`
  - Removed `if (!win.bounds)` check (windows always have dimensions)
- **Updated**: `#isPointInWindow()` method
  - Simplified from bounds-based checks to direct property access
  - Changed from `win.bounds.position.x` to `win.x` (and similar for all coordinates)

## Conceptual Improvements

### Before

```
WindowInfo (x-win crate)
    ↓
to_ax_node() [FAKE CONVERSION]
    ↓
AXNode (with role="window", empty path, synthetic data)
    ↓
Sent via WebSocket
    ↓
Treated as AXNode in TypeScript
```

### After

```
WindowInfo (x-win crate)
    ↓
Sent directly via WebSocket
    ↓
Window type in TypeScript

Separate from:

AXUIElement (macOS accessibility)
    ↓
get_ax_tree_by_pid() [REAL CONVERSION]
    ↓
AXNode (with real accessibility data)
    ↓
Sent via WebSocket
    ↓
AXNode in TypeScript
```

## Benefits

1. **Clarity**: Windows and accessibility nodes are conceptually different

   - Windows: OS-level window metadata (title, position, size, app name)
   - AXNodes: Accessibility tree elements (role, value, children, interactive properties)

2. **Correctness**: No more fake accessibility data

   - Previously: Faked `children_count` by querying accessibility API for windows
   - Now: Windows are just windows, get accessibility tree separately via `process_id`

3. **Type Safety**: TypeScript now reflects the actual data structure

   - `Window` has `x`, `y`, `w`, `h` (simple integers)
   - `AXNode` has `bounds` with `Position` and `Size` (structured objects)
   - No confusion between the two

4. **Simplicity**: Removed unnecessary conversion code
   - ~56 lines removed from `windows.rs`
   - Simplified WebSocket payload construction
   - Clearer frontend code (no more treating windows as nodes)

## Mental Model

```
Windows = Entry points to applications
  ├─ Metadata: title, app name, position, size
  ├─ Identifier: process_id (for accessing accessibility tree)
  └─ Use case: UI overlays, window tracking, positioning

AXNodes = Accessibility tree elements
  ├─ Hierarchy: parent-child relationships via paths
  ├─ Semantics: roles, values, states
  └─ Use case: UI automation, screen reading, interaction
```

## Files Modified

### Rust

- `src-tauri/src/windows.rs` (removed ~60 lines, simplified)
- `src-tauri/src/websocket.rs` (simplified broadcast logic)

### TypeScript

- `src-web/src/axio.ts` (added Window type, updated AXIO class)
- `src-web/src/windows-debug.ts` (rewrote to use Window type)
- `src-web/src/sand.ts` (updated to use Window properties)

### Documentation

- `TODO.md` (organized tasks into sections)

## Testing

- ✅ Rust code compiles (`cargo check`)
- ✅ No TypeScript linter errors
- ✅ All window property accesses updated correctly
