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
 * The source has to be the frontmost thing on screen where you point, as for any real click.
 */
import type { AX, Cut, Rect, TypedElement } from "allio";
import manifest from "../shaders/cuts.json";
import wgsl from "../shaders/cuts.wgsl?raw";
import { connect, declared, drawPointer, panel, screen } from "./shader-demo";

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

const fx = allio.shader({ region: screen(), wgsl, ...declared(manifest) });
const field = allio.pointer();
drawPointer(allio);

const showError = panel(
  "Cuts",
  "Cut out a region of the screen or of a window, or pick an element (↑ ↓ to widen or narrow, Esc to cancel). Drag a cut by its bar, resize it from the corner, and point into it to use what it shows.",
  [
    { button: "Cut screen region", onClick: () => selectRegion(false) },
    { button: "Cut window region", onClick: () => selectRegion(true) },
    { button: "Cut element", onClick: () => selectElement() },
    { button: "Clear", onClick: () => [...cuts].forEach(remove) },
  ]
);
fx.onerror = showError;

document.head.append(
  Object.assign(document.createElement("style"), {
    textContent: `
      .cut-select { position: fixed; inset: 0; cursor: crosshair; }
      .cut-bar { height: ${BAR}px; border-radius: 5px 5px 0 0; border-bottom: none; }
      .cut-bar .label {
        position: absolute; left: 7px; top: 4.5px; max-width: calc(50% - 26px);
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
      }
      .cut-bar .grip { position: absolute; left: 50%; top: 5px; width: 28px; height: 8px; margin-left: -14px; }
      .cut-bar .close { position: absolute; right: 0; top: 0; }
      .cut-corner { width: 20px; height: 10px; border-radius: 0 0 5px 5px; border-top: none; cursor: nwse-resize; }
      .cut-corner .grip { position: absolute; inset: 1px 4px 2px; cursor: inherit; }
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
      return { rect, visible: intersect(rect, w.bounds) };
    }
    case "element": {
      const rect = allio.get(anchor.element)?.bounds;
      const w = allio.windows.get(anchor.window);
      if (!rect || !w) return null;
      const view = anchor.clip === null ? null : allio.get(anchor.clip)?.bounds;
      return { rect, visible: intersect(view ? intersect(rect, view) : rect, w.bounds) };
    }
  }
}

/** The topmost window containing a point. */
const windowAt = (x: number, y: number) =>
  [...allio.windows.values()]
    .sort((a, b) => a.z_index - b.z_index)
    .find(({ bounds: b }) => x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h) ?? null;

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
  invalidate();
}

/** Somewhere near the source that keeps it uncovered: to the right, else left, else below. */
function beside(s: Rect): { x: number; y: number } {
  if (s.x + 2 * s.w + GAP <= innerWidth) return { x: s.x + s.w + GAP, y: s.y };
  if (s.x - s.w - GAP >= 0) return { x: s.x - s.w - GAP, y: s.y };
  return { x: s.x, y: Math.min(s.y + s.h + GAP + BAR, innerHeight - s.h) };
}

// --- Chrome: a bar to drag by (with a label and a close button), a corner to resize from, and
// a layer that keeps clicks out of fog ---

interface Chrome {
  bar: HTMLElement;
  label: HTMLElement;
  corner: HTMLElement;
  block: HTMLElement;
  hole: HTMLElement;
}

function chrome(): Chrome {
  const bar = Object.assign(document.createElement("div"), { className: "chrome cut-bar" });
  const label = Object.assign(document.createElement("span"), { className: "label" });
  const grip = Object.assign(document.createElement("div"), { className: "grip" });
  const close = Object.assign(document.createElement("div"), { className: "close" });
  bar.append(label, grip, close);
  const corner = Object.assign(document.createElement("div"), { className: "chrome cut-corner" });
  corner.append(Object.assign(document.createElement("div"), { className: "grip" }));
  // Fog can't be clicked through: the block takes the pointer except over the hole, the part
  // in view.
  const block = Object.assign(document.createElement("div"), { className: "cut-block" });
  const hole = document.createElement("div");
  block.append(hole);
  bar.setAttribute("ax-io", "opaque");
  corner.setAttribute("ax-io", "opaque");
  block.setAttribute("ax-io", "opaque");
  hole.setAttribute("ax-io", "transparent");
  document.body.append(block, bar, corner);

  const ui = { bar, label, corner, block, hole };
  const owner = () => cuts.find((c) => c.ui === ui);
  close.onclick = () => {
    const cut = owner();
    if (cut) remove(cut);
  };
  drag(bar, owner, (cut, dx, dy, start) => (cut.at = { x: start.at.x + dx, y: start.at.y + dy }));
  drag(corner, owner, (cut, dx) => {
    const w = cut.source.rect.w;
    cut.scale = Math.max(24 / Math.max(w, 1), (start(cut).scale * w + dx) / Math.max(w, 1));
  });
  return ui;
}

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
    starts.set(cut, { at: { ...cut.at }, scale: cut.scale });
    el.setPointerCapture(down.pointerId);
    passthrough.mode = "opaque";
    el.onpointermove = (e) => {
      move(cut, e.clientX - down.clientX, e.clientY - down.clientY, start(cut));
      invalidate();
    };
    el.onpointerup = () => {
      el.onpointermove = el.onpointerup = null;
      passthrough.mode = "auto";
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

function render() {
  queued = false;
  const slots = new Array(MAX * 12).fill(0);
  const mapped: Cut[] = [];

  cuts.forEach((cut, i) => {
    const now = resolve(cut.anchor);
    if (now) cut.source = now;
    else cut.source = { rect: cut.source.rect, visible: null };
    const { rect: src, visible } = cut.source;

    const shown = { ...cut.at, w: src.w * cut.scale, h: src.h * cut.scale };
    const seen = visible && visible.w > 0 && visible.h > 0 ? visible : null;
    const seenShown = seen && {
      x: shown.x + (seen.x - src.x) * cut.scale,
      y: shown.y + (seen.y - src.y) * cut.scale,
      w: seen.w * cut.scale,
      h: seen.h * cut.scale,
    };

    const vis = visible ?? { x: 0, y: 0, w: -1, h: 0 };
    slots.splice(i * 12, 12, ...[shown, src, vis].flatMap((r) => [r.x, r.y, r.w, r.h]));
    if (seen && seenShown) mapped.push({ shown: seenShown, source: { ...seen } });

    const { bar, label, corner, block, hole } = cut.ui;
    label.textContent = visible ? cut.label : `${cut.label} (gone)`;
    place(bar, { x: shown.x, y: shown.y - BAR, w: Math.max(shown.w, 56), h: BAR });
    Object.assign(corner.style, { left: `${shown.x + shown.w - 20}px`, top: `${shown.y + shown.h}px` });
    place(block, shown);
    hole.style.display = seenShown ? "block" : "none";
    if (seenShown) place(hole, { ...seenShown, x: seenShown.x - shown.x, y: seenShown.y - shown.y });
  });

  fx.set({ cuts: slots, count: cuts.length });
  field.set({ cuts: mapped });
}

invalidate();
