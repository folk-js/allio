# Spaces: one document, many screens

Status: implemented (macOS), from the probes in `probes/` (`spaces.swift`, `portals.swift`,
`layers.swift`). Demos: **stickies**, **ports** (wires across Spaces), **windows-debug**.

## Goal

allio should treat Spaces (desktops and full-screen apps) the way a web page treats the screen:
one program, one DOM, one state, that happens to be shown on several screens at once. UI that
belongs to something stays with it, on whichever Space it is, all the time, sliding in and out
with that Space when you swipe. Nothing about this should make authoring harder than today, and
it must stay light.

Not goals (for now): moving other apps' windows between Spaces (impossible with SIP on),
pointing into a window on another Space, multiple displays (the design leaves room for them).

## What the probes established (macOS 15, SIP on)

| | |
| --- | --- |
| Spaces, their order, kind, the current one, and which Spaces each window is on | SkyLight reads, live; current Space changes arrive as `NSWorkspace.activeSpaceDidChangeNotification` |
| Our own window on a chosen Space, including full screen, sliding with it | `SLSAddWindowsToSpaces`, *after* AppKit has committed ordering the window in (earlier and it snaps back to the current Space) |
| One live webview shown on several Spaces | Each Space's content is a compositing layer of the same page; a small window per Space shows its layer through a `CAPortalLayer` (`sourceContextId` + `CALayerGetRenderId`). Live at 60 fps, slides with its Space |
| Input on every Space | The webview's window is on every Space at full alpha (a window at alpha 0 gets no clicks) and can become key; it shows nothing itself because the webview sits in a container at opacity 0 (portals ignore their source's opacity) |
| Cost | ~50 MB (web content) for one Space, ~67 MB for five. Laying Spaces out side by side in one big document cost ~43 MB per Space instead: WebKit tiles by viewport area, painted or not |
| Windows on other Spaces | Accessibility handles taken while a window was visible keep working from any Space. Discovering windows allio has never seen is partial (remote-token scan finds most, not Music or TV) |

Identity: a **desktop** is a place (display + uuid, the first desktop's is empty, so position
for that one). A **full-screen Space** is a state of a window: it exists only while the window is
full screen and gets a new id each time, so it is named by its window.

## Design

### For authors

Two ideas: **presence** says where a window is; **belonging** says what a piece of UI is about.

```ts
window.presence     // "here" (on the current Space) | "elsewhere" | "minimised" | "hidden"
window.spaces       // ids of the Spaces it is on
allio.spaces        // { id, display, kind: "desktop" | "fullscreen", index, current }[]
allio.on("spaces:changed", …)
allio.windowAt(x, y) // the frontmost window that is here, at a point
allio.zOrder        // the windows that are here, front to back
```

```ts
const belonging = new AllioBelonging(allio); // like AllioPassthrough: opt in per page
belonging.spaceOf("window:4521");          // the Space it would be shown on, or null
```

```html
<div ax-on="window:4521">…</div>  <!-- lives on that window's Space, all the time -->
<div ax-on="element:88">…</div>   <!-- … on its element's window's Space -->
<div ax-on="space:1295">…</div>   <!-- … on that Space -->
<div>…</div>                      <!-- with you: on every Space (today's behaviour) -->
```

- Positions are screen coordinates, as today: an element on Space 2 is placed where it should
  appear on Space 2. Pages keep computing window-relative positions from window data. Owned
  elements are moved into their Space's container (full screen, fixed), so position them
  absolutely and don't rely on where they were in the DOM.
- Windows persist when they leave the current Space: same object, same elements and observers,
  `presence` changes. Nothing churns when the user switches Spaces. A window allio has never
  seen on screen is added only if it is on a Space (off-screen windows on no Space are mostly
  not real windows), and accessibility handles are only looked up for windows that are here.
- `allio.windows` has every window; code that hit-tests or draws over what is on screen checks
  `presence === "here"` or uses `allio.windowAt` / `allio.zOrder`, which only have windows that
  are here.
- When an element's owner is on no Space at all (minimised, hidden, closed), the client marks it
  `ax-away`. A default stylesheet hides it; CSS can restyle it instead. `ax-away` is a plain
  attribute: passthrough sees it only through CSS like anything else.
- Pointer and keyboard input reach the current Space's content only. Elsewhere, content is live
  but not interactive (you can't point there anyway).
- Existing demos change nothing: unowned content is "with you", which is what they are today,
  except that they now also appear over full-screen apps.

### How it maps onto the mechanism

The page is one webview in an overlay window on every Space, which takes all input but doesn't
draw itself: macOS holds a window that is on every Space still during a swipe, so its content
would pop out and back in. Instead every Space has a **mirror**: a click-through window placed on
that Space (once AppKit has ordered it in), showing the page through two `CAPortalLayer`s: the
whole page, and on top, that Space's container. (The page portal's source is the webview
layer's child, not the webview's layer: that one is `geometryFlipped`, bridging AppKit's
bottom-up coordinates to WebKit's top-down ones, and a portal doesn't carry its source's own
flip, so the page came out upside down.) So "with you" UI is on every Space and slides
with each one, and Space UI is on its own Space. The overlay hides its drawing (its webview's
superview at opacity 0; portals don't take on their sources' ancestors' opacity) only once the
current Space's mirror shows the page; without `CAPortalLayer` it keeps drawing itself.

Space content lives in `#allio-spaces`, a wrapper at opacity 0.0001 (invisible in the page
portal), holding one container per Space (`#allio-space-<id>`, composited, opacity
`1 - marker/100000`). The client (`AllioBelonging`) keeps every `ax-on` element in the container
of the Space its owner is on, makes every container but the current one `inert`, and declares
the containers to the host (`space_layers_set`). The host (`src-tauri/src/spaces.rs`) rescans
for the containers' layers four times a second (name and marker must agree) and rebinds a
portal whose layer was replaced. Mirrors are made for the Spaces of the overlay's display.

Because Space UI is drawn above the whole page in each mirror, it is above "with you" UI
whatever their CSS `z-index`, matching hit-testing (the wrapper is on top of the page).

Window polling (`crates/allio`) lists every window, with `presence` and `spaces` (`SkyLight`
reads, cached per window: refreshed when it appears or comes on or off screen, and every 0.5 s).
Enumeration costs about what the on-screen-only list did (~3 ms, nearly all the window list call).

Shaders and the pointer field keep their own native windows, which join every Space, so for
now they still pop out and in during a swipe.

## Not yet

- Spaces on displays other than the overlay's (the main display): containers cover the overlay's
  viewport, and mirrors are made for every Space but positioned like the overlay.
- Windows allio has never seen on screen have no accessibility handle (discovering them through
  remote tokens, as the probe did, is possible but partial).
- Occlusion (`AllioOcclusion`) only knows the windows that are here; UI on other Spaces is not
  clipped by the windows in front of its window there.

## Risks and fallbacks

- **Private API**: SkyLight (reads, placing our windows), `CAPortalLayer` and
  `CALayerGetRenderId`, a layer's `context`, WebKit's `_setWindowOcclusionDetectionEnabled:`.
  All long-lived, none documented. If `SkyLight` is missing, `spaces` is empty and everything is
  "with you". If `CAPortalLayer` is missing, the host logs it and Space content isn't shown; a
  fallback (the overlay showing the current Space's container itself, losing only the slide
  during a swipe) is not built yet.
- **Finding each container's layer** (`probes/layers.swift`). Two independent signals, which
  must agree: WebKit's layer name, which contains the element's id, and an opacity marker the
  client sets on each container (`1 - k/100000`, read back exactly, renders exactly like 1).
  Under content changes, resizes, 200 extra composited elements and same-sized decoys, both
  agreed and nothing was ambiguous. WebKit replaces a container's layer only when it moves in
  the DOM or comes back from `display: none`; a rescan rebinds it. So the client owns the
  containers (creates them once, never moves or hides them), and the host rescans when a portal's
  layer is gone and cheaply on a timer. If WebKit stops naming layers, the opacity marker alone
  still works; if the two ever disagree, that is a detectable error, not a wrong portal.
- **Memory** grows with content per Space (each container's painted elements), not with the
  number of Spaces. A page with heavy content on every Space pays for it.
- **Unverified**: multiple displays, "Displays have separate Spaces" off, Stage Manager,
  Mission Control's previews, Spaces created or removed while running.
