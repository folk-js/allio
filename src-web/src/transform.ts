/**
 * Rotate and scale a window. It keeps working as drawn: point at any part of the turned, shrunken
 * window and the pointer acts on the real window there. What the window no longer covers shows
 * what's behind it (but can't be clicked: the real window is still there).
 */
import { panel } from "./shader-demo";
import { deformer, rectOf, windowAt } from "./warp-demo";

const d = deformer();
const { allio, passthrough } = d;
let angle = 0;
let scale = 1;

const showError = panel("Transform", "Pick a window, then turn it by the top knob and scale it by the corner knob.", [
  { button: "Pick window", onClick: () => pick() },
  { button: "Reset", onClick: () => ((angle = 0), (scale = 1), apply()) },
]);
d.fx.onerror = showError;
const panelEl = document.querySelector(".demo-panel")!;

function knob(cursor: string): HTMLElement {
  const el = Object.assign(document.createElement("div"), { className: "chrome knob" });
  el.append(Object.assign(document.createElement("div"), { className: "grip" }));
  el.style.cursor = cursor;
  el.setAttribute("ax-io", "opaque");
  document.body.append(el);
  return el;
}
const turn = knob("grab");
const size = knob("nwse-resize");
d.chrome = () => [rectOf(panelEl), ...(d.target ? [rectOf(turn), rectOf(size)] : [])];

/** Where a point of the real window (window-local) is drawn, on screen. */
function drawnAt(x: number, y: number): { x: number; y: number } {
  const w = d.target!.bounds;
  const [cx, cy] = [w.w / 2, w.h / 2];
  const [dx, dy] = [(x - cx) * scale, (y - cy) * scale];
  const [cos, sin] = [Math.cos(angle), Math.sin(angle)];
  return { x: w.x + cx + cos * dx - sin * dy, y: w.y + cy + sin * dx + cos * dy };
}

/** The map back, from where the window is drawn to the real window: the inverse turn and scale. */
function apply() {
  const w = d.target?.bounds;
  if (!w) return;
  const [cos, sin] = [Math.cos(angle) / scale, Math.sin(angle) / scale];
  const [cx, cy] = [w.w / 2, w.h / 2];
  const [a, b, c, dd] = [cos, sin, -sin, cos];
  d.affine = [a, b, c, dd, cx - (a * cx + b * cy), cy - (c * cx + dd * cy)];
}

d.onFrame = (win) => {
  turn.style.display = size.style.display = win ? "block" : "none";
  if (!win) return;
  const w = win.bounds;
  apply(); // the window may have been resized
  const top = drawnAt(w.w / 2, -24 / scale);
  const corner = drawnAt(w.w + 10 / scale, w.h + 10 / scale);
  Object.assign(turn.style, { left: `${top.x}px`, top: `${top.y}px` });
  Object.assign(size.style, { left: `${corner.x}px`, top: `${corner.y}px` });
};

function drag(el: HTMLElement, move: (x: number, y: number, centre: { x: number; y: number }) => void) {
  el.addEventListener("pointerdown", (down) => {
    const w = d.target?.bounds;
    if (!w) return;
    el.setPointerCapture(down.pointerId);
    passthrough.mode = "opaque";
    el.onpointermove = (e) => move(e.clientX, e.clientY, { x: w.x + w.w / 2, y: w.y + w.h / 2 });
    el.onpointerup = () => {
      el.onpointermove = el.onpointerup = null;
      passthrough.mode = "auto";
    };
  });
}
drag(turn, (x, y, c) => {
  angle = Math.atan2(y - c.y, x - c.x) + Math.PI / 2;
  apply();
});
drag(size, (x, y, c) => {
  const w = d.target!.bounds;
  scale = Math.min(1.5, Math.max(0.15, Math.hypot(x - c.x, y - c.y) / Math.hypot(w.w / 2 + 10, w.h / 2 + 10)));
  apply();
});

/** The next click chooses the window to transform. */
function pick() {
  const layer = Object.assign(document.createElement("div"), { className: "pick-layer" });
  Object.assign(layer.style, { position: "fixed", inset: "0", cursor: "crosshair" });
  layer.setAttribute("ax-io", "opaque");
  document.body.prepend(layer);
  passthrough.mode = "opaque";
  layer.onclick = (e) => {
    layer.remove();
    passthrough.mode = "auto";
    const w = windowAt(allio, e.clientX, e.clientY);
    if (!w) return;
    d.setTarget(w.id);
    angle = 0;
    scale = 1;
    apply();
  };
}
