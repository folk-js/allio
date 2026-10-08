/**
 * WinCuts: cut part of the screen out and put it anywhere, live and usable. Pointing into a cut
 * puts the real cursor over the matching point of its source, so hovering, clicking, dragging and
 * scrolling act on the real thing (the system cursor is hidden meanwhile and this page draws the
 * pointer where it appears).
 *
 * A cut's source is anchored one of three ways:
 * - to the screen: a fixed rectangle;
 * - to a window: a rectangle that moves with its window;
 * - to an element: the bounds of an accessibility element, followed as it moves, resizes and
 *   scrolls. The part of it that is out of view (scrolled away, outside its window) is drawn as
 *   fog, and can't be clicked through.
 *
 * Cuts of a window (or of an element) draw from that window's own pixels, so they stay visible
 * while it's covered. Pointing into a covered part still acts on whatever covers the source there
 * (see docs/POINTER.md for what was tried), unless the sources are parked: moved onto a backstage
 * display nobody sees, where nothing covers them.
 */
import type { AX, Cut, Rect, TypedElement } from "allio";
import manifest from "../shaders/cuts.json";
import wgsl from "../shaders/cuts.wgsl?raw";
import { connect, declared, drawPointer, panel, screen, source } from "./shader-demo";

const { allio, passthrough } = connect();

const MAX = 8;
const GAP = 24;
const BAR = 18;

type Anchor =
  | { kind: "screen"; rect: Rect }
  | { kind: "window"; window: AX.WindowId; offset: Rect }
  | { kind: "element"; element: AX.ElementId; clip: AX.ElementId | null; window: AX.WindowId };

/** Where an anchor's source is now, and which part of it is in view (null: gone). */
interface Source {
  rect: Rect;
  visible: Rect | null;
  /** For a window's cut: the same, in the window's own points (from its top-left). The shader
   * reads window pixels with these, so the cut doesn't depend on where the window is and stays
   * still while the window moves. */
  local?: { rect: Rect; visible: Rect | null };
}

interface Placed {
  anchor: Anchor;
  label: string;
  /** Top-left of the shown rect. Its size is the source's, times `scale`. */
  at: { x: number; y: number };
  scale: number;
  /** The last source seen, kept when the anchor is gone. */
  source: Source;
  ui: Chrome;
}

const cuts: Placed[] = [];

const fx = allio.shader({ region: screen(), wgsl: source(manifest, wgsl), ...declared(manifest) });
const field = allio.pointer();
drawPointer(allio);

const showError = panel(
  "Cuts",
  "Cut out a region of the screen or of a window, or pick an element (↑ ↓ to widen or narrow, Esc to cancel). Drag a cut by its bar, resize it from the corner, and point into it to use what it shows.",
  [
    { button: "Cut screen region", onClick: () => selectRegion(false) },
    { button: "Cut window region", onClick: () => selectRegion(true) },
    { button: "Cut element", onClick: () => selectElement() },
    { toggle: "park sources", value: false, onChange: (on) => void park(on) },
    { button: "Clear", onClick: () => [...cuts].forEach(remove) },
  ]
);
fx.onerror = showError;

// --- Backstage: the windows cuts are taken from, moved onto a display nobody sees ---
//
// Parked windows keep rendering and nothing covers them, so their cuts stay live and clicks
// through them land. The pointer never appears on the backstage display; it only acts there,
// through cuts. Unparking puts each window back where it was.

/** The backstage display while parking, in screen points. */
let backstage: Rect | null = null;
/** Where each parked window was. */
const homes = new Map<AX.WindowId, { x: number; y: number }>();
/** Space between parked windows, and room left at the top for a menu bar. */
const PARK_GAP = 24;
const PARK_TOP = 44;

async function park(on: boolean) {
  if (on) {
    backstage = await allio.backstage(true).catch((e: Error) => (showError(e.message), null));
    await parkAll();
  } else {
    await Promise.all([...homes].map(([id, home]) => allio.moveWindow(id, home.x, home.y).catch(() => {})));
    homes.clear();
    backstage = null;
    await allio.backstage(false).catch(() => {});
  }
  invalidate();
}

/** Moves every source window not yet parked onto the backstage, left to right in rows. */
async function parkAll() {
  if (!backstage) return;
  const b = backstage;
  let x = b.x + PARK_GAP;
  let y = b.y + PARK_TOP;
  let row = 0;
  for (const id of new Set(cuts.map((c) => windowOf(c.anchor)).filter((w) => w !== null))) {
    const w = allio.windows.get(id);
    if (!w || homes.has(id)) continue;
    if (x + w.bounds.w > b.x + b.w && x > b.x + PARK_GAP) {
      x = b.x + PARK_GAP;
      y += row + PARK_GAP;
      row = 0;
    }
    homes.set(id, { x: w.bounds.x, y: w.bounds.y });
    await allio.moveWindow(id, x, y).catch((e: Error) => showError(e.message));
    x += w.bounds.w + PARK_GAP;
    row = Math.max(row, w.bounds.h);
  }
}

/** Puts a window back if no cut is taken from it any more. */
function unparkUnused() {
  const used = new Set(cuts.map((c) => windowOf(c.anchor)));
  for (const [id, home] of homes) {
    if (used.has(id)) continue;
    homes.delete(id);
    void allio.moveWindow(id, home.x, home.y).catch(() => {});
  }
}

document.head.append(
  Object.assign(document.createElement("style"), {
    textContent: `
      .cut-select { position: fixed; inset: 0; cursor: crosshair; }
      .cut-bar { height: ${BAR}px; cursor: grab; }
      .cut-bar:active { cursor: grabbing; }
      .cut-bar .close { position: absolute; right: 0; top: 0; width: 18px; height: 18px; cursor: default; }
      .cut-corner { width: 20px; height: 10px; cursor: nwse-resize; }
      .cut-caption { position: fixed; display: none; }
      .cut-block { position: fixed; }
      .cut-block > div { position: absolute; }`,
  })
);

// --- Anchors ---

function intersect(a: Rect, b: Rect): Rect {
  const x = Math.max(a.x, b.x);
  const y = Math.max(a.y, b.y);
  return { x, y, w: Math.max(0, Math.min(a.x + a.w, b.x + b.w) - x), h: Math.max(0, Math.min(a.y + a.h, b.y + b.h) - y) };
}

function resolve(anchor: Anchor): Source | null {
  switch (anchor.kind) {
    case "screen":
      return { rect: anchor.rect, visible: anchor.rect };
    case "window": {
      const w = allio.windows.get(anchor.window);
      if (!w) return null;
      const rect = { ...anchor.offset, x: w.bounds.x + anchor.offset.x, y: w.bounds.y + anchor.offset.y };
      const own = { x: 0, y: 0, w: w.bounds.w, h: w.bounds.h };
      return {
        rect,
        visible: intersect(rect, w.bounds),
        local: { rect: anchor.offset, visible: intersect(anchor.offset, own) },
      };
    }
    case "element": {
      const rect = allio.get(anchor.element)?.bounds;
      const w = allio.windows.get(anchor.window);
      if (!rect || !w) return null;
      const view = anchor.clip === null ? null : allio.get(anchor.clip)?.bounds;
      const visible = intersect(view ? intersect(rect, view) : rect, w.bounds);
      // Element bounds are on screen; in window points they only change when it scrolls or
      // reflows, not when the window moves.
      const local = (r: Rect) => ({ ...r, x: r.x - w.bounds.x, y: r.y - w.bounds.y });
      return { rect, visible, local: { rect: local(rect), visible: local(visible) } };
    }
  }
}

/** The topmost window containing a point. */
const windowAt = (x: number, y: number) => allio.windowAt(x, y);

/** The nearest scroll area around an element: what decides which part of it is in view. */
async function scrollAreaOf(el: TypedElement): Promise<TypedElement | null> {
  let current = el;
  for (let i = 0; i < 20; i++) {
    const parent = await allio.parent(current.id).catch(() => null);
    if (!parent) return null;
    if (parent.role === "scrollarea") return parent;
    current = parent;
  }
  return null;
}

function describe(el: TypedElement): string {
  const name = el.label || el.description || (typeof el.value === "string" ? el.value : "");
  return name ? `${el.role} · ${name}` : el.role;
}

// --- Making cuts ---

/** A full-screen layer that takes the pointer (and the keyboard) while choosing a source. */
function chooser(cleanup = () => {}): { layer: HTMLElement; done: () => void } {
  const layer = Object.assign(document.createElement("div"), { className: "cut-select" });
  layer.setAttribute("ax-io", "opaque");
  document.body.append(layer);
  passthrough.mode = "opaque";
  const done = () => {
    layer.remove();
    passthrough.mode = "auto";
    removeEventListener("keydown", onKey);
    cleanup();
  };
  const onKey = (e: KeyboardEvent) => e.key === "Escape" && done();
  addEventListener("keydown", onKey);
  return { layer, done };
}

/** The next drag chooses a rectangle, anchored to the screen or to the window it starts in. */
function selectRegion(inWindow: boolean) {
  if (cuts.length >= MAX) return;
  const { layer, done } = chooser();
  const outline = Object.assign(document.createElement("div"), { className: "outline" });
  layer.addEventListener("pointerdown", (down) => {
    down.preventDefault(); // no text selection while choosing
    layer.setPointerCapture(down.pointerId);
    layer.append(outline);
    const rect = (e: PointerEvent): Rect => ({
      x: Math.min(down.clientX, e.clientX),
      y: Math.min(down.clientY, e.clientY),
      w: Math.abs(e.clientX - down.clientX),
      h: Math.abs(e.clientY - down.clientY),
    });
    layer.onpointermove = (e) => place(outline, rect(e));
    layer.onpointerup = (e) => {
      done();
      const r = rect(e);
      if (r.w < 8 || r.h < 8) return;
      const w = inWindow ? windowAt(down.clientX, down.clientY) : null;
      if (w) {
        const offset = { ...r, x: r.x - w.bounds.x, y: r.y - w.bounds.y };
        add({ kind: "window", window: w.id, offset }, w.app_name || w.title);
      } else {
        add({ kind: "screen", rect: r }, "Screen");
      }
    };
  });
}

/** Hovering highlights the element under the pointer; ↑ and ↓ walk to its parent and back. */
function selectElement() {
  if (cuts.length >= MAX) return;
  const { layer, done } = chooser(() => removeEventListener("keydown", onKey));
  const outline = Object.assign(document.createElement("div"), { className: "outline" });
  const caption = Object.assign(document.createElement("div"), { className: "chrome caption" });
  layer.append(outline, caption);

  let chain: TypedElement[] = [];
  let level = 0;
  let point = { x: 0, y: 0 };
  let asking = false;

  const show = () => {
    const el = chain[level];
    outline.style.display = caption.style.display = el?.bounds ? "block" : "none";
    if (!el?.bounds) return;
    place(outline, el.bounds);
    caption.textContent = describe(el);
    const below = el.bounds.y + el.bounds.h + 6;
    Object.assign(caption.style, {
      left: `${el.bounds.x}px`,
      top: `${below + 20 < innerHeight ? below : el.bounds.y - 26}px`,
    });
  };

  const ask = async () => {
    asking = true;
    const p = point;
    const el = await allio.elementAt(p.x, p.y).catch(() => null);
    asking = false;
    if (el && el.id !== chain[0]?.id) {
      chain = [el];
      level = 0;
      show();
    }
    if (p !== point) void ask();
  };
  layer.onpointermove = (e) => {
    point = { x: e.clientX, y: e.clientY };
    if (!asking) void ask();
  };

  const onKey = async (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") level = Math.max(0, level - 1);
    if (e.key === "ArrowUp") {
      if (level + 1 < chain.length) level++;
      else if (chain[level]) {
        const parent = await allio.parent(chain[level].id).catch(() => null);
        if (parent?.bounds && parent.role !== "window" && parent.role !== "application") {
          chain.push(parent);
          level++;
        }
      }
    }
    show();
  };
  addEventListener("keydown", onKey);

  layer.onclick = async () => {
    done();
    const el = chain[level];
    if (!el?.bounds) return;
    const clip = await scrollAreaOf(el);
    void allio.observe(el.id, { depth: 1, wait_between_ms: 60 }).catch(() => {});
    if (clip) void allio.observe(clip.id, { depth: 1, wait_between_ms: 200 }).catch(() => {});
    add({ kind: "element", element: el.id, clip: clip?.id ?? null, window: el.window_id }, describe(el));
  };
}

function add(anchor: Anchor, label: string) {
  const source = resolve(anchor);
  if (!source) return;
  cuts.push({ anchor, label, at: beside(source.rect), scale: 1, source, ui: chrome() });
  if (backstage) void parkAll();
  invalidate();
}

function remove(cut: Placed) {
  const i = cuts.indexOf(cut);
  if (i < 0) return;
  cuts.splice(i, 1);
  for (const el of Object.values(cut.ui)) el.remove();
  if (cut.anchor.kind === "element") {
    void allio.unobserve(cut.anchor.element).catch(() => {});
    if (cut.anchor.clip !== null) void allio.unobserve(cut.anchor.clip).catch(() => {});
  }
  unparkUnused();
  invalidate();
}

/** Somewhere near the source that keeps it uncovered: to the right, else left, else below. */
function beside(s: Rect): { x: number; y: number } {
  if (s.x + 2 * s.w + GAP <= innerWidth) return { x: s.x + s.w + GAP, y: s.y };
  if (s.x - s.w - GAP >= 0) return { x: s.x - s.w - GAP, y: s.y };
  return { x: s.x, y: Math.min(s.y + s.h + GAP + BAR, innerHeight - s.h) };
}

// --- Chrome. The shader draws it, with the cut, so the two move together; these elements only
// take the pointer: a bar to drag by (with a close button), a corner to resize from, and a layer
// that keeps clicks out of fog. The label shows as a caption while the pointer is on the bar. ---

interface Chrome {
  bar: HTMLElement;
  corner: HTMLElement;
  block: HTMLElement;
  hole: HTMLElement;
  caption: HTMLElement;
}

/** The cut whose close cross is under the pointer, for the shader to light it; -1 if none. */
let closeLit = -1;

function chrome(): Chrome {
  const bar = Object.assign(document.createElement("div"), { className: "hit cut-bar" });
  const close = document.createElement("div");
  close.className = "close";
  bar.append(close);
  const corner = Object.assign(document.createElement("div"), { className: "hit cut-corner" });
  const caption = Object.assign(document.createElement("div"), { className: "chrome caption cut-caption" });
  // Fog can't be clicked through: the block takes the pointer except over the hole, the part
  // in view.
  const block = Object.assign(document.createElement("div"), { className: "cut-block" });
  const hole = document.createElement("div");
  block.append(hole);
  bar.setAttribute("ax-io", "opaque");
  corner.setAttribute("ax-io", "opaque");
  block.setAttribute("ax-io", "opaque");
  hole.setAttribute("ax-io", "transparent");
  document.body.append(block, bar, corner, caption);

  const ui = { bar, corner, block, hole, caption };
  const owner = () => cuts.find((c) => c.ui === ui);
  close.onclick = () => {
    const cut = owner();
    if (cut) remove(cut);
  };
  close.onpointerenter = () => ((closeLit = cuts.findIndex((c) => c.ui === ui)), invalidate());
  close.onpointerleave = () => ((closeLit = -1), invalidate());
  bar.onpointerenter = () => !holding && (caption.style.display = "block");
  bar.onpointerleave = () => (caption.style.display = "none");
  drag(bar, owner, (cut, dx, dy, start) => (cut.at = { x: start.at.x + dx, y: start.at.y + dy }));
  drag(corner, owner, (cut, dx) => {
    const w = cut.source.rect.w;
    cut.scale = Math.max(24 / Math.max(w, 1), (start(cut).scale * w + dx) / Math.max(w, 1));
  });
  return ui;
}

/** While the page's own chrome is being dragged, the pointer acts where it appears: otherwise a
 * quick drag reaches the cut before it moves, the pointer is carried into the source, and the drag
 * (and any text there) goes with it. */
let holding = false;

const starts = new WeakMap<Placed, { at: { x: number; y: number }; scale: number }>();
const start = (cut: Placed) => starts.get(cut)!;

function drag(
  el: HTMLElement,
  owner: () => Placed | undefined,
  move: (cut: Placed, dx: number, dy: number, start: { at: { x: number; y: number }; scale: number }) => void
) {
  el.addEventListener("pointerdown", (down) => {
    const cut = owner();
    if (!cut || (down.target as HTMLElement).classList.contains("close")) return;
    down.preventDefault(); // no text selection while dragging
    cut.ui.caption.style.display = "none";
    starts.set(cut, { at: { ...cut.at }, scale: cut.scale });
    el.setPointerCapture(down.pointerId);
    passthrough.mode = "opaque";
    holding = true;
    invalidate();
    el.onpointermove = (e) => {
      move(cut, e.clientX - down.clientX, e.clientY - down.clientY, start(cut));
      invalidate();
    };
    el.onpointerup = () => {
      el.onpointermove = el.onpointerup = null;
      passthrough.mode = "auto";
      holding = false;
      invalidate();
    };
  });
}

function place(el: HTMLElement, r: Rect) {
  Object.assign(el.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
}

// --- Keeping everything in step ---

let queued = false;
function invalidate() {
  if (queued) return;
  queued = true;
  requestAnimationFrame(render);
}

allio.on("window:changed", invalidate);
allio.on("window:removed", invalidate);
allio.on("element:changed", invalidate);
allio.on("element:removed", invalidate);

/** The window a cut's pixels come from, if it is one window's (else they come from the screen). */
const windowOf = (a: Anchor): AX.WindowId | null => (a.kind === "screen" ? null : a.window);

/** What the window sources w0..w7 show, as last sent, to send only changes. */
let lastSources = "";
let lastAnimate: boolean | undefined;

function render() {
  queued = false;
  const slots = new Array(MAX * 16).fill(0);
  const mapped: Cut[] = [];

  // One window source per window that cuts are taken from; covered windows still show.
  const windows = [...new Set(cuts.map((c) => windowOf(c.anchor)).filter((w) => w !== null))].slice(0, MAX);
  const sources = Object.fromEntries(
    Array.from({ length: MAX }, (_, k) => [`w${k}`, k < windows.length ? { window: windows[k] } : null])
  );
  if (JSON.stringify(sources) !== lastSources) {
    lastSources = JSON.stringify(sources);
    fx.sources = sources;
  }
  let fogged = false;

  cuts.forEach((cut, i) => {
    const now = resolve(cut.anchor);
    if (now) cut.source = now;
    else
      cut.source = {
        rect: cut.source.rect,
        visible: null,
        local: cut.source.local && { rect: cut.source.local.rect, visible: null },
      };
    const { rect: src, visible } = cut.source;

    const shown = { ...cut.at, w: src.w * cut.scale, h: src.h * cut.scale };
    const seen = visible && visible.w > 0 && visible.h > 0 ? visible : null;
    const seenShown = seen && {
      x: shown.x + (seen.x - src.x) * cut.scale,
      y: shown.y + (seen.y - src.y) * cut.scale,
      w: seen.w * cut.scale,
      h: seen.h * cut.scale,
    };

    const win = windowOf(cut.anchor);
    const origin = win === null ? -1 : windows.indexOf(win);
    // Window pixels are read in the window's own points (see `local`); the screen in screen points.
    const from = origin >= 0 && cut.source.local ? cut.source.local : { rect: src, visible };
    const vis = from.visible ?? { x: 0, y: 0, w: -1, h: 0 };
    slots.splice(i * 16, 16, ...[shown, from.rect, vis].flatMap((r) => [r.x, r.y, r.w, r.h]), origin, 0, 0, 0);
    if (seen && seenShown) mapped.push({ shown: seenShown, source: { ...seen } });
    fogged ||= !seen || seen.w < src.w - 0.5 || seen.h < src.h - 0.5;

    const { bar, corner, block, hole, caption } = cut.ui;
    caption.textContent = visible ? cut.label : `${cut.label} (gone)`;
    Object.assign(caption.style, { left: `${shown.x}px`, top: `${shown.y - BAR - 26}px` });
    place(bar, { x: shown.x, y: shown.y - BAR, w: Math.max(shown.w, 56), h: BAR });
    Object.assign(corner.style, { left: `${shown.x + shown.w - 20}px`, top: `${shown.y + shown.h}px` });
    place(block, shown);
    hole.style.display = seenShown ? "block" : "none";
    if (seenShown) place(hole, { ...seenShown, x: seenShown.x - shown.x, y: seenShown.y - shown.y });
  });

  // Fog drifts, so draw every frame only while some cut shows fog; otherwise only on change.
  if (fogged !== lastAnimate) {
    lastAnimate = fogged;
    fx.animate = fogged;
  }
  fx.set({ cuts: slots, count: cuts.length, ui: [closeLit, 0, 0, 0] });
  field.set({ cuts: holding ? [] : mapped, away: backstage ? [{ ...backstage }] : [] });
}

invalidate();
