/**
 * Warp brush: push a window around like a liquify brush, to bend it out of the way of something
 * behind it. The deformation belongs to the window and moves with it; nothing else is warped.
 * Turn the brush off and the window works as drawn: the pointer acts on the real window under
 * whatever part of it you point at.
 */
import { panel } from "./shader-demo";
import { COLS, ROWS, deformer, flatGrid, gridOffset, rectOf, windowAt } from "./warp-demo";

const d = deformer();
const { allio, passthrough } = d;
const brush = { size: 160, strength: 0.7, on: true };

const showError = panel("Warp", "With the brush on, drag across a window to push it around. Turn the brush off to use the window as it is drawn.", [
  { toggle: "brush", value: brush.on, onChange: (on) => setBrush(on) },
  { range: "size", min: 40, max: 400, step: 1, value: brush.size, onInput: (v) => (brush.size = v) },
  { range: "strength", min: 0.1, max: 1, step: 0.05, value: brush.strength, onInput: (v) => (brush.strength = v) },
  { button: "Reset", onClick: () => d.grid && ((d.grid = flatGrid()), d.changed()) },
]);
d.fx.onerror = showError;
const panelEl = document.querySelector(".demo-panel")!;
d.chrome = () => [rectOf(panelEl)];

// While the brush is on, a layer under the panel takes the pointer, and the pointer acts where it
// appears (the deformation is being edited, not used).
const layer = Object.assign(document.createElement("div"), { className: "brush-layer" });
layer.setAttribute("ax-io", "opaque");
const ring = Object.assign(document.createElement("div"), { className: "outline round" });
layer.append(ring);
document.body.prepend(layer);
document.head.append(
  Object.assign(document.createElement("style"), {
    textContent: `.brush-layer { position: fixed; inset: 0; cursor: crosshair; }`,
  })
);

function setBrush(on: boolean) {
  brush.on = on;
  layer.style.display = on ? "block" : "none";
  passthrough.mode = on ? "opaque" : "auto";
  d.acting = !on;
  d.changed();
}
setBrush(true);

layer.addEventListener("pointermove", (e) => {
  const r = brush.size / 2;
  Object.assign(ring.style, { left: `${e.clientX - r}px`, top: `${e.clientY - r}px`, width: `${2 * r}px`, height: `${2 * r}px` });
});

layer.addEventListener("pointerdown", (down) => {
  down.preventDefault(); // no text selection while brushing
  const p = { x: down.clientX, y: down.clientY };
  const current = d.target?.bounds;
  const near = current && p.x > current.x - 120 && p.x < current.x + current.w + 120 && p.y > current.y - 120 && p.y < current.y + current.h + 120;
  if (!near) {
    const w = windowAt(allio, p.x, p.y);
    if (!w) return;
    d.setTarget(w.id);
  }
  d.grid ??= flatGrid();
  layer.setPointerCapture(down.pointerId);
  let last = p;
  layer.onpointermove = (e) => {
    const next = { x: e.clientX, y: e.clientY };
    push(last, next);
    last = next;
  };
  layer.onpointerup = () => (layer.onpointermove = layer.onpointerup = null);
});

/**
 * Pushes the window along a brush stroke from `from` to `to` (a forward warp): whatever was shown
 * at q - δ·w(q) is shown at q, where w falls off with distance from the brush. Long strokes are
 * split up so the window never folds over itself.
 */
function push(from: { x: number; y: number }, to: { x: number; y: number }) {
  const w = d.target?.bounds;
  const g = d.grid;
  if (!w || !g) return;
  const sigma = brush.size / 2;
  const total = { x: to.x - from.x, y: to.y - from.y };
  const steps = Math.max(1, Math.ceil(Math.hypot(total.x, total.y) / (sigma / 4)));
  for (let s = 0; s < steps; s++) {
    const t = (s + 0.5) / steps;
    const centre = { x: from.x + total.x * t - w.x, y: from.y + total.y * t - w.y };
    const delta = { x: total.x / steps, y: total.y / steps };
    const old = { ...g, offsets: [...g.offsets] };
    for (let row = 0; row < ROWS; row++) {
      for (let col = 0; col < COLS; col++) {
        const qx = -g.margin + (col * (w.w + 2 * g.margin)) / (COLS - 1);
        const qy = -g.margin + (row * (w.h + 2 * g.margin)) / (ROWS - 1);
        const weight = brush.strength * Math.exp(-((qx - centre.x) ** 2 + (qy - centre.y) ** 2) / (2 * sigma * sigma));
        if (weight < 1e-3) continue;
        const sx = qx - delta.x * weight;
        const sy = qy - delta.y * weight;
        const [ox, oy] = gridOffset(old, w.w, w.h, sx, sy);
        const k = 2 * (row * COLS + col);
        // The map at q becomes the old map at q - δ·w: (q - δ·w) + old(q - δ·w).
        g.offsets[k] = ox - delta.x * weight;
        g.offsets[k + 1] = oy - delta.y * weight;
      }
    }
  }
  d.changed();
}
