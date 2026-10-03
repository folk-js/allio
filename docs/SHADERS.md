# Screen shaders

Screen shaders capture a region of the screen and draw it back, in place, through a WGSL fragment shader. Capture, shading and presentation all stay on the GPU; the webview only declares the shader and sends values. macOS 14 or later only, and it needs the Screen Recording permission.

```ts
const fx = allio.shader({
  region: { x: 100, y: 100, w: 800, h: 500 },
  uniforms: { radius: "f32" },
  values: { radius: 160 },
  wgsl: /* wgsl */ `
    @fragment fn fs(in: VsOut) -> @location(0) vec4f {
      let d = distance(u.region.xy + in.uv * u.region.zw, u.mouse);
      return textureSample(screen, samp, in.uv) * smoothstep(u.radius, 0.0, d);
    }`,
});

fx.set({ radius: 200 });                          // typed from the declaration
fx.region = { x: 0, y: 0, w: 1512, h: 982 };      // move or resize
fx.wgsl = newSource;                              // live edit; a bad edit keeps the old shader
fx.onerror = (message) => console.warn(message);  // WGSL diagnostics
fx.dispose();
```

## What the shader sees

You write only the fragment function `fs`. Allio prepends a prelude with the vertex stage, the bindings and a `Uniforms` struct, so the same source should also run in browser WebGPU given the same prelude (not yet tried).

| Name | Type | Meaning |
| --- | --- | --- |
| `in.uv` | `vec2f` | Position in the region, `(0, 0)` top-left to `(1, 1)` bottom-right |
| `screen` | `texture_2d<f32>` | The captured region |
| `samp` | `sampler` | Linear, clamped. Use with `textureSample(screen, samp, uv)` |
| `u.resolution` | `vec2f` | Output size in pixels |
| `u.time` | `f32` | Seconds since the shader started |
| `u.mouse` | `vec2f` | Cursor position in screen points, top-left origin |
| `u.region` | `vec4f` | The region as `(x, y, w, h)` in screen points |
| `u.frame` | `f32` | Number of frames drawn so far |
| `u.state_size` | `vec2f` | Size of the `state` texture in cells (see [Stateful shaders](#stateful-shaders)) |

The built-ins are filled natively when each frame is drawn, so `u.mouse` and `u.time` are never a message behind. Your own uniforms are declared by name and type: `f32`, `vec2f`, `vec3f`, `vec4f`, or `vec4f[N]` (an array of N `vec4f`, passed as 4N numbers). Built-in names can't be redeclared, and all uniforms together are limited to 4 KB.

The output is premultiplied alpha: return alpha 0 to let what is underneath show through.

## Window geometry

Declare these uniforms and the host fills them natively as windows move, with no round trip to the page, so they are as fresh as `u.mouse`:

- `windows: "vec4f[N]"`: the on-screen windows as `(x, y, w, h)` in screen points, frontmost first. Unused entries are zero, so a loop can stop at the first `r.z <= 0.0`. Windows beyond N are left out.
- `focused: "f32"`: the index of the focused window in that list, or -1.

```wgsl
for (var i = 0; i < 24; i++) {
  let r = u.windows[i];
  if (r.z <= 0.0) { break; }
  // r.xy is the window's top-left, r.zw its size
}
```

## Seeing behind windows

`hide` chooses what is left out of `screen`, so a shader can show what is behind a window. Our own windows are always left out.

- `"none"` (default): the screen as it is.
- `{ windows: [id, …] }`: the screen without those windows (ids as in `allio.windows`). Change it live with `fx.hide = …`.
- `"all"`: the desktop, the Dock and the menu bar; every application's ordinary windows are left out. Applications that open windows later are left out only once something refreshes the filter.

### Two captures: `behind`

Declare a second capture with the `behind` option (a `Hide`, like `hide`) and the shader gets a second texture, `behind`, next to `screen`. With `screen` as it is and `behind` without a window, a shader can draw the window's own pixels and what is behind it at the same time. Change it live with `fx.behind = …`. It can't be added or removed once the shader is running.

A shader that returns transparent pixels (`vec4f(0.0)`) leaves the real screen showing through, so it can cut a hole in one place and touch nothing else. This is how `xray` and `lava` work. Colour is premultiplied, so `vec4f(rgb, 0.0)` adds light without dimming, and the real screen is never copied or delayed where you draw nothing.

## Stateful shaders

A shader that defines a second function remembers between frames:

```wgsl
@fragment fn sim(in: VsOut) -> @location(0) vec4f { … }   // runs once per frame, before fs
@fragment fn fs(in: VsOut) -> @location(0) vec4f { … }    // draws
```

`state` is a texture of cells covering the region, `cell` screen points on a side (default 4, set with the `cell` option). In `sim`, `state` is the previous step and the returned `vec4f` becomes the next one: `in.pos.xy` is the cell being written, and `textureLoad(state, vec2i(x, y), 0)` reads any cell exactly. In `fs`, `state` is the latest step, and `textureSampleLevel(state, samp, in.uv, 0.0)` reads it smoothly. It starts as zeros. Cells hold four 32-bit floats. Read them with `textureLoad` (exact); smooth sampling of a float texture isn't guaranteed on every GPU. Reading outside the texture is undefined, so check bounds.

`sim` runs `steps` times per drawn frame (default 1, at most 8), so a simulation's speed follows the display's refresh rate. Raise `steps` when you shrink `cell`, to keep things moving at the same speed in points per second.

Read the state from the page with `await fx.probe(x, y)`: it returns the cell under a screen point as `[r, g, b, a]`, as `sim` last wrote it. That is how `lava` knows whether the cursor is over a burnt-out hole.

## How it behaves

- **Latest wins.** `set()` and `region` updates made within one animation frame are merged into one message, and the native side overwrites its state instead of queueing. The shader redraws on every vsync with whatever state it has.
- **Client owns the state.** The client pushes its complete set of shaders whenever shaders are added or removed, and whenever the socket opens. Reconnecting needs no special handling.
- **Lifetime follows the connection.** When a client's websocket closes, its shaders are removed. Switching overlays or reloading a page cleans up by itself.
- **Errors.** A shader that fails to compile or has bad initial values is rejected before any window appears, and `fx.error` holds the diagnostics. If an edit of `fx.wgsl` fails, the previous version keeps running. Setting an undeclared uniform or the wrong number of components throws at the call site.
- **Our own windows are not captured**, so shaders can sit under an overlay without feeding back.
- **One display.** A shader stays on the display its region started on. Window placement assumes the primary display is the first one in the system's list.

## Wire protocol

Two messages, both over the same websocket as the accessibility API:

- `shaders_set { shaders: { [id]: ShaderSpec } }` is the complete desired state. Applying it twice changes nothing. The reply lists per-shader errors.
- `shader_patch { id, values?, region? }` is the hot path for uniform values and region changes.

`ShaderSpec` and `Region` are generated from Rust (`npm run typegen`).

## Demos

Pick them from the tray menu. Their shaders are in `src-web/shaders/`; a test builds every one, and others run `xray`, `blobs` and `lava` on the GPU and check the pixels.

- **shader**: a magnifying, rippling lens around the cursor, with controls for the region.
- **aura**: a glowing halo around every window that pulses, drifts in colour and brightens near the cursor. Uses `windows`, `focused` and `u.mouse`, and draws additively.
- **blobs**: classic monochrome metaballs. A few blobs wander the screen as particles in the simulation state: they keep apart from each other, are shoved out of windows and drawn to window edges, where they cling. Windows and the cursor are metaballs in the same field, so a blob near one fuses with it in a bridge of goo. Uses `windows`, `sim`.
- **xray**: a hole punched through the window under the cursor, modelled as a quarter-arc of a circle. Between the outer circle and the inner one the window's surface is flat at the outer edge and curves smoothly down until it runs straight through at the inner edge; inside is what is behind the window. The surface normal at each point sets the lighting (ambient plus diffuse from the top-left, and a bright line where the surface turns edge-on), and the window's own pixels are taken from where the curved surface lies: `warp` 1 squeezes the texture toward the hole by arc length (as it would look painted on the rim), 0 leaves it flat, -1 stretches it the other way. `thickness` is the rim's width. Uses `windows` and both captures (`screen`, and `behind` without the hovered window).
- **lava**: click and hold in empty space and lava pours from the cursor, lands on windows, pools, and eats through them, leaving holes onto the desktop. What a window is made of is read from its colour, and decides how it burns: white, green and red things are fuel (flames, and fire spreads through them much faster than the lava eats), blue things are water (steam, and lava touching them cools to stone), very dark things are coal (slow, glowing embers), and everything else is metal (heats through red to white-hot and conducts heat slowly). Clicks on windows still go to the windows, but a burnt-out hole counts as empty space. Drawn one 3-point cell at a time for a pixel-art look. Uses everything: a `sim` with window geometry as solid ground, a four-channel `RGBA32Float` state, `behind`, `cell` and `probe`.

Each demo's options are in `src-web/shaders/<name>.json`, read by both the page and the tests.

## Crates

- `allio-shader`: the implementation. Spec types, uniform layout and WGSL to MSL translation (naga) are portable; capture and rendering are macOS (`ScreenCaptureKit` frames wrapped as Metal textures without copying). `cargo run -p allio-shader --example native` runs a shader standalone.
- `src-tauri/src/shaders.rs`: reconciles a client's declared shaders with the live ones.
