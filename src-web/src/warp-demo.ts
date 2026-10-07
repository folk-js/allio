/**
 * Shared by the window deformation demos (brush, transform): one window drawn deformed, with the
 * pointer agreeing.
 *
 * A deformation maps a point where the window appears to the point of the real window shown
 * there, in window-local points: first `affine`, then `grid`. The warp shader draws it and the
 * pointer field acts through it (allio-pointer's `Warp`); both follow the window as it moves.
 * Where the deformation carves the window away, what was behind it shows, and clicks there are
 * dropped by the host (the real window is still there).
 */
import type { AX, Grid, Rect } from "allio";
import manifest from "../shaders/warp.json";
import wgsl from "../shaders/warp.wgsl?raw";
import { connect, declared, drawPointer, screen, source } from "./shader-demo";

/** `[a, b, c, d, tx, ty]`: (x, y) goes to (a·x + b·y + tx, c·x + d·y + ty). */
export type Affine = [number, number, number, number, number, number];
export const IDENTITY: Affine = [1, 0, 0, 1, 0, 0];

/** Grid points across and down; the shader holds 320. */
export const COLS = 20;
export const ROWS = 16;
/** How far past each edge of the window the grid reaches, so edges can be pushed outwards. */
export const MARGIN = 120;
const MAX_ABOVE = 8;

export function flatGrid(): Grid {
  return { margin: MARGIN, cols: COLS, rows: ROWS, offsets: new Array(2 * COLS * ROWS).fill(0) };
}

/** The grid's displacement at window-local point `p` of a `w` x `h` window. Same as the shader's. */
export function gridOffset(g: Grid, w: number, h: number, x: number, y: number): [number, number] {
  const last = [g.cols - 1, g.rows - 1];
  const fx = ((x + g.margin) / (w + 2 * g.margin)) * last[0];
  const fy = ((y + g.margin) / (h + 2 * g.margin)) * last[1];
  if (fx < 0 || fy < 0 || fx > last[0] || fy > last[1]) return [0, 0];
  const col = Math.min(Math.floor(fx), last[0] - 1);
  const row = Math.min(Math.floor(fy), last[1] - 1);
  const tx = fx - col;
  const ty = fy - row;
  const at = (c: number, r: number, axis: number) => g.offsets[2 * (r * g.cols + c) + axis];
  const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
  return [0, 1].map((axis) =>
    lerp(lerp(at(col, row, axis), at(col + 1, row, axis), tx), lerp(at(col, row + 1, axis), at(col + 1, row + 1, axis), tx), ty)
  ) as [number, number];
}

/** The topmost window containing a point. */
export function windowAt(allio: { windows: Map<AX.WindowId, AX.Window> }, x: number, y: number): AX.Window | null {
  return (
    [...allio.windows.values()]
      .sort((a, b) => a.z_index - b.z_index)
      .find(({ bounds: b }) => x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h) ?? null
  );
}

export interface Deformer {
  readonly allio: ReturnType<typeof connect>["allio"];
  readonly passthrough: ReturnType<typeof connect>["passthrough"];
  readonly fx: ReturnType<ReturnType<typeof connect>["allio"]["shader"]>;
  /** The deformed window, if any. */
  readonly target: AX.Window | null;
  affine: Affine;
  grid: Grid | null;
  /** Deform this window from now on (and leave the previous one alone). */
  setTarget(id: AX.WindowId | null): void;
  /** Whether the pointer acts through the deformation (off while editing it). */
  acting: boolean;
  /** Two knobs to draw on top (screen points, x1 y1 x2 y2), or null. */
  knobs: [number, number, number, number] | null;
  /** Our own clickable chrome: the pointer acts where it appears there. */
  chrome: () => Rect[];
  /** Call after changing `grid` or `acting` (changes to `affine` are noticed by themselves). */
  changed(): void;
  /** Called every frame, after the target window has been read. */
  onFrame?: (w: AX.Window | null) => void;
}

export function deformer(): Deformer {
  const { allio, passthrough } = connect();
  const fx = allio.shader({ region: screen(), wgsl: source(manifest, wgsl), ...declared(manifest) });
  const field = allio.pointer();
  drawPointer(allio);

  let targetId: AX.WindowId | null = null;
  let dirty = true;
  let lastShader = "";
  let lastField = "";

  const d: Deformer = {
    allio,
    passthrough,
    fx,
    get target() {
      return targetId === null ? null : allio.windows.get(targetId) ?? null;
    },
    affine: [...IDENTITY],
    grid: null,
    acting: true,
    knobs: null,
    chrome: () => [],
    setTarget(id) {
      if (id === targetId) return;
      targetId = id;
      d.affine = [...IDENTITY];
      d.grid = null;
      fx.behind = id === null ? "none" : { windows: [id] };
      dirty = true;
    },
    changed() {
      dirty = true;
    },
  };

  const frame = () => {
    requestAnimationFrame(frame);
    const w = d.target;
    d.onFrame?.(w);

    // Windows in front of the target, and our own chrome, are left alone by both shader and pointer.
    const above: Rect[] = w
      ? [...allio.windows.values()].filter((o) => o.z_index < w.z_index).map((o) => o.bounds)
      : [];
    above.push(...d.chrome());

    const win = w?.bounds ?? { x: 0, y: 0, w: 0, h: 0 };
    const [a, b, c, dd, tx, ty] = d.affine;
    const flatAbove = new Array(MAX_ABOVE * 4).fill(0);
    above.slice(0, MAX_ABOVE).forEach((r, i) => flatAbove.splice(i * 4, 4, r.x, r.y, r.w, r.h));
    const shaderKey = JSON.stringify([win, d.affine, flatAbove, d.knobs]);
    if (dirty || shaderKey !== lastShader) {
      lastShader = shaderKey;
      fx.set({
        win: [win.x, win.y, win.w, win.h],
        affine: [a, b, c, dd],
        shift: [tx, ty, d.grid?.margin ?? 0, d.grid ? 1 : 0],
        dims: [d.grid?.cols ?? 0, d.grid?.rows ?? 0, Math.min(above.length, MAX_ABOVE), d.knobs ? 1 : 0],
        knobs: d.knobs ?? [0, 0, 0, 0],
        above: flatAbove,
        ...(dirty ? { grid: [...(d.grid?.offsets ?? []), ...new Array(640 - (d.grid?.offsets.length ?? 0)).fill(0)] } : {}),
      });
    }

    const warps = w && d.acting ? [{ window: { ...win }, affine: d.affine, grid: d.grid ?? undefined, above }] : [];
    const fieldKey = JSON.stringify([warps.length ? [win, d.affine, above] : null, dirty]);
    if (dirty || fieldKey !== lastField) {
      lastField = fieldKey;
      field.set({ warps });
    }
    dirty = false;
  };
  requestAnimationFrame(frame);

  allio.on("window:removed", ({ window_id }) => window_id === targetId && d.setTarget(null));
  return d;
}

/** Where an element is on screen, for `chrome`. */
export function rectOf(el: Element): Rect {
  const r = el.getBoundingClientRect();
  return { x: r.x, y: r.y, w: r.width, h: r.height };
}
