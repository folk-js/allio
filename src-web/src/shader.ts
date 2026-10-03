/** A native screen shader with live controls. Pixels never touch the webview: this page only
 * declares a WGSL shader, a region, and uniform values. */
import { Allio, AllioPassthrough } from "allio";

const p = new URLSearchParams(location.search);
const num = (k: string, d: number) => Number(p.get(k) ?? d);

const region = { x: num("x", 100), y: num("y", 100), w: num("w", 800), h: num("h", 500) };
const params = { radius: num("radius", 180), strength: num("strength", 0.7) };

// Built-ins (filled natively each frame): u.resolution, u.time, u.mouse (screen points), u.region (x,y,w,h).
const wgsl = /* wgsl */ `
@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let pt = u.region.xy + in.uv * u.region.zw;           // this pixel, in screen points
  let d = pt - u.mouse;
  let r = length(d);
  let dir = d / max(r, 0.001);
  let k = 1.0 - smoothstep(0.0, u.radius, r);       // 1 at the cursor, 0 at the lens edge
  let k2 = k * k;

  // Lens: sample closer to the cursor (magnify), plus a ripple travelling outward.
  let ripple = sin(r * 0.12 - u.time * 6.0) * k2 * u.strength * 6.0;
  let src = u.mouse + d * (1.0 - u.strength * 0.6 * k2) + dir * ripple;
  let uv = (src - u.region.xy) / u.region.zw;

  // Chromatic split along the displacement.
  let off = dir * k * u.strength * 4.0 / u.region.zw;
  let c = vec3f(
    textureSample(screen, samp, uv + off).r,
    textureSample(screen, samp, uv).g,
    textureSample(screen, samp, uv - off).b);
  return vec4f(c, 1.0);
}`;

// --- ui ---------------------------------------------------------------------------------------

const allio = new Allio();
const passthrough = new AllioPassthrough(allio);

const fx = allio.shader({
  region: { ...region },
  wgsl,
  uniforms: { radius: "f32", strength: "f32" },
  values: params,
});
const status = document.getElementById("status")!;
fx.onerror = (e) => (status.textContent = e ?? "");

allio.connect();

const frame = document.getElementById("frame")!;
const layout = () =>
  Object.assign(frame.style, { left: `${region.x}px`, top: `${region.y}px`, width: `${region.w}px`, height: `${region.h}px` });
layout();

type Control = { key: string; min: number; max: number; step: number; get: () => number; set: (v: number) => void };
const refreshers: (() => void)[] = [];

const controls: Control[] = [
  ...(["x", "y", "w", "h"] as const).map((k) => ({
    key: k,
    min: k === "w" || k === "h" ? 64 : 0,
    max: k === "x" || k === "w" ? innerWidth : innerHeight,
    step: 1,
    get: () => region[k],
    set: (v: number) => {
      region[k] = v;
      moved();
    },
  })),
  { key: "radius", min: 20, max: 500, step: 1, get: () => params.radius, set: (v) => ((params.radius = v), fx.set({ radius: v })) },
  { key: "strength", min: 0, max: 1, step: 0.01, get: () => params.strength, set: (v) => ((params.strength = v), fx.set({ strength: v })) },
];

for (const c of controls) {
  const label = document.createElement("label");
  const input = Object.assign(document.createElement("input"), { type: "range", min: c.min, max: c.max, step: c.step });
  const out = document.createElement("output");
  const refresh = () => {
    input.value = String(c.get());
    out.textContent = String(Math.round(c.get() * 100) / 100);
  };
  input.oninput = () => c.set(Number(input.value)) ?? refresh();
  label.append(Object.assign(document.createElement("span"), { textContent: c.key }), input, out);
  document.getElementById("controls")!.append(label);
  refreshers.push(refresh);
  refresh();
}

function moved() {
  layout();
  refreshers.forEach((r) => r());
  fx.region = { ...region };
}

document.getElementById("show")!.onchange = (e) =>
  frame.classList.toggle("hidden", !(e.target as HTMLInputElement).checked);

/** Pointer drag that keeps the overlay capturing events until released. */
function drag(el: Element, onMove: (dx: number, dy: number, start: typeof region) => void) {
  el.addEventListener("pointerdown", (e) => {
    const ev = e as PointerEvent;
    const start = { ...region };
    const [sx, sy] = [ev.clientX, ev.clientY];
    (el as HTMLElement).setPointerCapture(ev.pointerId);
    passthrough.mode = "opaque";
    const move = (m: Event) => {
      const pe = m as PointerEvent;
      onMove(pe.clientX - sx, pe.clientY - sy, start);
      moved();
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      passthrough.mode = "auto";
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  });
}
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
drag(frame.querySelector(".grip")!, (dx, dy, s) => {
  region.x = clamp(s.x + dx, 0, innerWidth - region.w);
  region.y = clamp(s.y + dy, 0, innerHeight - region.h);
});
drag(frame.querySelector(".corner")!, (dx, dy, s) => {
  region.w = clamp(s.w + dx, 64, innerWidth - region.x);
  region.h = clamp(s.h + dy, 64, innerHeight - region.y);
});
