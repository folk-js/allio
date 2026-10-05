# Pointer fields

A pointer field decides how real mouse motion moves the pointer, and where on the real screen the pointer acts. It exists so that shader-drawn worlds (cuts, lenses, any displacement) agree with pointer interaction: whatever is drawn under the pointer is what it clicks, hovers, drags and scrolls.

The host keeps a **visual pointer** V that the hand moves, and puts the **real cursor** at R = S(V), where S is the field's map: the point of the real screen that is drawn at V. When R ≠ V the system cursor is hidden and the page draws the pointer at V (`drawPointer` in `src-web/src/shader-demo.ts`). Nothing is synthesised: a session event tap rewrites real moves and warps the real cursor, so clicks, drags, hover, scrolling and the keyboard are the real thing.

```ts
const field = allio.pointer({ gain: 0.5 });              // everything at half speed
field.set({ targets: [{ rect, gain: 0.4, reach: 6 }] }); // sticky buttons
field.set({ cuts: [{ shown, source }] });                // part of the screen drawn elsewhere
field.set({ lenses: [{ x, y, r: 150, mag: 3 }] });       // a fixed magnifier
const { x, y, hidden } = await allio.pointerState();     // where to draw the pointer
field.dispose();
```

| Part | Moves V | Maps V to R |
| --- | --- | --- |
| `gain` | All motion is multiplied by it (0.05–8). | |
| `targets` | Motion inside `rect` (grown by `reach`) is multiplied by its `gain`; the smallest target wins. | |
| `cuts` | | Inside `shown`, R is the matching point of `source` (scaled if the sizes differ). Later cuts are on top. |
| `lenses` | | Inside the lens, R is the point the lens magnifies there. Within `r * LENS_FLAT` that is `mag` times closer to the centre; out to `r` it eases back. |
| `warps` | | A deformed window. In window-local points V goes through `affine`, then `grid` (a displacement field over the window grown by `margin`); if that lands inside the window, R is there. `above` lists what is in front of the window, where nothing is mapped. |

Cuts come first, then warps, then lenses. A shader that draws a cut, lens or warp must use the same map; `cuts.wgsl`, `lens.wgsl` and `warp.wgsl` do, and tests in both crates pin the same numbers.

## How it behaves

- **Moving the real cursor** is done by posting a marked mouse event (of the same kind, so drags stay drags) from an event source that doesn't suppress local events. `CGWarpMouseCursorPosition` freezes the hardware for about 0.25 s after every warp; `CGAssociateMouseAndMouseCursorPosition` didn't prevent that from a background app, and `CGSetLocalEventsSuppressionInterval` is marked no longer supported.
- **Our moves race the hardware's.** Events already on their way when we move the cursor were measured from where it was. Because our move travels through the same stream, the stream says where the cursor was before each event: the previous event's position, or where our move put it if it came through in between (`tracker.rs`). Event delta fields can't be used: after the cursor is moved they include the move.
- **Escape hatch.** ⌘⇧Esc lets go of the pointer until the page next changes its fields.
- **Hiding the cursor** while another app is frontmost needs one private call (`CGSSetConnectionProperty`, `SetsCursorInBackground`), made once, the first time the cursor has to hide.
- **Client owns the state.** The complete set of fields is pushed at most once per frame when it changes, and when the socket opens; several fields combine. When the page's connection closes the fields are dropped, the tap removed and the cursor shown, so a reload or crash never leaves the pointer reshaped. No fields, or fields that change nothing, means no tap.
- **Carved-away windows can't be clicked through.** Where a warp has moved a window off part of its real footprint, what was behind it shows, but the real window is still there. Presses, their releases and scrolls there are dropped by the tap rather than landing on the window by surprise.
- **Real clicks need a real target.** Through a cut, the click lands on whatever is frontmost at the source point. If another window covers the source there, that window gets it.

## Demos

- **cuts** (WinCuts, Tan et al., CHI 2004): cut part of the screen out and place it anywhere, scaled if you like; point into it to use the real thing. A cut's source is anchored to the screen, to a window (it follows the window), or to an accessibility element (it follows the element's bounds as it moves, resizes and scrolls; the part out of view is drawn as fog and can't be clicked through, and a removed element leaves the cut fogged and marked gone). Only the visible part of a cut goes to the pointer field.
- **lens**: a fixed magnifier you can put over a toolbar or small text. Inside it the pointer acts on what it appears to be over, so it moves more slowly over the real screen (a pointing lens, Ramos et al., CHI 2007, falling out of the map rather than being a speed setting).
- **warp**: a liquify-style brush. Drag across a window to push it around (to make room for something behind it); the deformation belongs to the window and moves with it. Turn the brush off to use the window as drawn.
- **transform**: pick a window, then turn and scale it by its knobs. It still works as drawn.
- **magnet**: buttons, links, tabs and checkboxes of the window under the pointer, found through accessibility, are sticky (semantic pointing, Blanch et al., CHI 2004), and the one you're on lifts.

## Crates

- `allio-pointer`: spec types, the field and the tracker (portable, pure, unit tested), and the macOS event tap.
- `src-tauri/src/pointer.rs`: merges a client's declared fields, keeps the tap in step, answers `pointer_state`.
