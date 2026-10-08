/**
 * Magnetic pointer: the controls of whatever window is under the pointer become sticky, found
 * through accessibility (no pixels or guesses), and the control you're on
 * lifts like a key under a finger.
 *
 * Targets come from the window's accessibility tree, observed so they follow scrolling and
 * relayout. The pointer field (native) does the sticking; the shader draws the lift.
 * Both use the same rule for which target the pointer is on: the smallest one it's inside,
 * counting `reach`.
 */
import { AX, type Target } from "allio";
import manifest from "../shaders/magnet.json";
import wgsl from "../shaders/magnet.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const { allio } = connect();

const settings = { gain: 0.45, reach: 6, on: true };

const fx = allio.shader({ region: screen(), wgsl, ...declared(manifest), values: { hl: [0, 0, 0, 0], alpha: 0 } });
const field = allio.pointer();

const showError = panel("Magnetic pointer", "Buttons, links, tabs and checkboxes in the window under the pointer are sticky: the pointer slows down over them.", [
  { range: "stickiness", min: 0.1, max: 1, step: 0.05, value: settings.gain, onInput: (v) => ((settings.gain = v), push()) },
  { range: "reach", min: 0, max: 20, step: 1, value: settings.reach, onInput: (v) => ((settings.reach = v), push()) },
  { toggle: "magnets", value: settings.on, onChange: (v) => ((settings.on = v), push()) },
]);
fx.onerror = showError;

// --- Targets, from the accessibility tree ---

const MAGNETIC: AX.Role[] = ["button", "link", "checkbox", "radiobutton", "switch", "popupbutton", "tab", "menuitem", "stepper"];
/** Bigger than this isn't a control you aim at. */
const MAX_SIDE = 420;
const MIN_SIDE = 6;
/** Tree depth to observe below the window root. */
const DEPTH = 30;

let windowId: AX.WindowId | null = null;
let rootId: AX.ElementId | null = null;
let rects: AX.Bounds[] = [];

function collect() {
  if (windowId === null) return void (rects = []);
  rects = allio
    .getWindowElements(windowId)
    .filter((el) => MAGNETIC.includes(el.role) && el.bounds)
    .map((el) => el.bounds!)
    .filter((b) => b.w >= MIN_SIDE && b.h >= MIN_SIDE && b.w <= MAX_SIDE && b.h <= MAX_SIDE);
  push();
}

function push() {
  const targets: Target[] = settings.on
    ? rects.map((rect) => ({ rect, gain: settings.gain, reach: settings.reach }))
    : [];
  field.set({ targets });
}

async function follow(next: AX.WindowId | null) {
  if (next === windowId) return;
  const previous = rootId;
  windowId = next;
  rootId = null;
  rects = [];
  push();
  if (previous !== null) allio.unobserve(previous).catch(() => {});
  if (next === null) return;

  const root = await allio.windowRoot(next).catch(() => null);
  if (!root || windowId !== next) return;
  rootId = root.id;
  await allio.observe(root.id, { depth: DEPTH, wait_between_ms: 300 }).catch(() => {});
  collect();
}

allio.on("subtree:changed", ({ root_id }) => root_id === rootId && collect());
allio.on("window:changed", ({ window }) => window.id === windowId && collect());

const windowAt = (x: number, y: number) => allio.windowAt(x, y)?.id ?? null;

// --- The lift: which target the pointer is on, animated towards ---

let mouse = { x: 0, y: 0 };
allio.on("mouse:position", (p) => {
  mouse = p;
  void follow(windowAt(p.x, p.y));
});

/** The smallest target containing `p`, counting reach: the same rule as the native field. */
function targetAt(p: { x: number; y: number }): AX.Bounds | null {
  const r = settings.reach;
  let best: AX.Bounds | null = null;
  for (const b of rects) {
    const inside = p.x >= b.x - r && p.x < b.x + b.w + r && p.y >= b.y - r && p.y < b.y + b.h + r;
    if (inside && (!best || b.w * b.h < best.w * best.h)) best = b;
  }
  return best;
}

const hl = [0, 0, 0, 0];
let alpha = 0;

function frame() {
  const target = settings.on ? targetAt(mouse) : null;
  if (target) {
    const goal = [target.x, target.y, target.w, target.h];
    const fresh = alpha < 0.05; // appear in place rather than sliding in from the last control
    for (let i = 0; i < 4; i++) hl[i] = fresh ? goal[i] : hl[i] + (goal[i] - hl[i]) * 0.35;
  }
  alpha += ((target ? 1 : 0) - alpha) * 0.25;
  if (Math.abs(alpha - (target ? 1 : 0)) < 0.002) alpha = target ? 1 : 0;
  // Only send changes: the shader draws only when something changed.
  const key = `${hl.map((v) => v.toFixed(2))} ${alpha.toFixed(3)}`;
  if (key !== sent) {
    sent = key;
    fx.set({ hl: [...hl] as [number, number, number, number], alpha });
  }
  requestAnimationFrame(frame);
}
let sent = "";
requestAnimationFrame(frame);
