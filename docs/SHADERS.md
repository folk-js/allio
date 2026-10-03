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

The built-ins are filled natively when each frame is drawn, so `u.mouse` and `u.time` are never a message behind. Your own uniforms are declared by name and type: `f32`, `vec2f`, `vec3f` or `vec4f`. Built-in names can't be redeclared.

The output is premultiplied alpha: return alpha 0 to let what is underneath show through.

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

## Crates

- `allio-shader`: the implementation. Spec types, uniform layout and WGSL to MSL translation (naga) are portable; capture and rendering are macOS (`ScreenCaptureKit` frames wrapped as Metal textures without copying). `cargo run -p allio-shader --example native` runs a shader standalone.
- `src-tauri/src/shaders.rs`: reconciles a client's declared shaders with the live ones.
