# Tauri Overlay Improvements

This document tracks improvements and open questions specific to the Tauri overlay application (distinct from the core AXIO accessibility layer - see `REFACTOR.md`).

---

## Overlay File Handling

### Current State

- Overlays are discovered by scanning `src-web/overlays/*.html` at startup
- Path resolution uses fragile parent directory traversal
- No way to load external URLs or files outside the overlays directory

### Improvements Needed

**1. More Robust Path Resolution**

- Don't rely on executable path structure
- Use Tauri's resource directory APIs
- Support both development and production paths cleanly

**2. Flexible Overlay Sources**

- Load from local files (absolute paths)
- Load from URLs (http/https)
- Tray menu should support "Open URL..." or "Open File..." options

**3. Hot Reload**

- Watch overlays directory for changes
- Auto-refresh when files change (dev mode)

---

## Tray Improvements

### Current State

- Static menu built once at startup
- Shows overlay filenames only
- Clickthrough toggle with emoji

### Improvements Needed

**1. Dynamic Menu Updates**

- Refresh overlay list when files change
- Show checkmark next to current overlay
- Show connection status (how many WS clients)

**2. Quick Actions**

- "Reload Current Overlay"
- "Open DevTools"
- "Copy WebSocket URL"

---

## Window Type / Focus Behavior

### The Problem

Currently, clicking on the overlay brings the Tauri app to the foreground and steals focus from the target application. This breaks the UX where you want to interact with the overlay while keeping the target app focused.

### The Solution: NSPanel with NonActivating Mask

**Yes, this IS possible on macOS!**

```swift
// In native macOS (for reference)
let panel = NSPanel(...)
panel.styleMask.insert(.nonactivatingPanel)
panel.level = .floating
panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
```

**In Tauri/Rust:**

Options to implement this:

1. **Use `window-vibrancy` or similar crate** that provides NSPanel support

2. **Direct Objective-C via `objc` crate:**

```rust
use objc::{msg_send, sel, sel_impl, class};

unsafe {
    let ns_window: *mut Object = window.ns_window() as *mut Object;

    // Convert to panel-like behavior
    let _: () = msg_send![ns_window, setLevel: 3]; // NSFloatingWindowLevel

    // Set non-activating (this is the key part)
    // styleMask |= NSWindowStyleMask.nonactivatingPanel (1 << 7)
    let current_mask: u64 = msg_send![ns_window, styleMask];
    let _: () = msg_send![ns_window, setStyleMask: current_mask | (1 << 7)];
}
```

3. **Tauri plugin or PR** - might be worth contributing upstream

### Research Needed

- Does Tauri v2 support this natively?
- Does `window-vibrancy` help here?
- What are the implications for click-through behavior?
- Test with actual overlay interactions

---

## State Persistence

### What to Persist

- Last selected overlay (file path or URL)

### Implementation

- Use Tauri's `tauri-plugin-store` or similar
- Store in app data directory
- Load on startup, save on change

```rust
// Example structure
#[derive(Serialize, Deserialize)]
struct AppState {
    last_overlay: Option<String>,  // URL or file path
}
```

---

## Building & Bundling

### Current State

- Development builds work via `cargo tauri dev`
- Production bundling not tested/documented

### Needed

**1. Production Build**

```bash
cargo tauri build
```

- Test on macOS (signed? notarized?)
- Document required certificates for distribution

**2. Asset Bundling**

- How are overlay HTML files bundled?
- Are they in Resources or embedded?
- Does Vite build get included correctly?

**3. Auto-Update**

- Consider `tauri-plugin-updater` for future
- GitHub releases integration

**4. CI/CD**

- GitHub Actions for building releases
- Artifact signing

---

## WebSocket Discovery

### The Problem

Currently, the WebSocket server binds to `127.0.0.1:3030`. If that port is taken:

- Server fails to start
- No fallback
- Overlay can't connect
- No way for external tools to find the server

### Options

**Option A: Dynamic Port + File Advertisement**

```rust
// Bind to port 0 (OS assigns available port)
let listener = TcpListener::bind("127.0.0.1:0").await?;
let port = listener.local_addr()?.port();

// Write to well-known location
let port_file = dirs::runtime_dir()
    .unwrap_or_else(|| dirs::cache_dir().unwrap())
    .join("axio.port");
fs::write(&port_file, port.to_string())?;
```

Clients read the port file to discover the server.

**Option B: mDNS/Bonjour Service Advertisement**

```rust
// Advertise via mDNS
let service = ServiceDaemon::new()?;
service.register(
    "_axio._tcp",
    "AXIO Accessibility Server",
    port,
    &[("version", "1.0")]
)?;
```

Clients discover via mDNS query. More robust but adds dependency.

**Option C: Tauri IPC (for bundled overlay only)**

The Tauri webview can get the port directly from Rust via IPC:

```rust
#[tauri::command]
fn get_websocket_port() -> u16 {
    WS_PORT.load(Ordering::Relaxed)
}
```

```typescript
// In overlay
const port = await invoke("get_websocket_port");
const ws = new WebSocket(`ws://localhost:${port}/ws`);
```

- Use dynamic port binding
- Write port to file for external discovery
- Use Tauri IPC for bundled overlays (faster, more reliable)

---

## Additional Suggestions

### 1. Multi-Monitor Support

- Overlay should span or be placeable on any monitor
- Consider separate overlay windows per monitor

### 2. Keyboard Shortcuts

- Global shortcut to toggle overlay visibility (already have Cmd+Shift+E for clickthrough)
- Shortcut to cycle overlays
- Shortcut to reload

### 3. Development Experience

- `--overlay <path>` CLI flag for quick testing
- DevTools always accessible in dev builds
- Better error display when overlay fails to load

---

## Priority

1. **WebSocket discovery** - Currently broken if port is taken
2. **Window focus behavior** - Core UX issue
3. **State persistence** - Quality of life
4. **Build/bundle** - Needed for distribution
5. **Tray improvements** - Nice to have
6. **Overlay flexibility** - Nice to have
