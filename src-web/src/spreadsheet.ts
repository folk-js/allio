/**
 * Spreadsheet → chart.
 *
 * Finds a table in the focused window (Numbers, Finder list views, any AXTable),
 * observes it, rebuilds the grid from each cell's row/column position, and charts
 * its numeric columns live next to the window. Read-only: the source app is never
 * written to.
 */

import { Allio, AX, type TypedElement } from "allio";

const allio = new Allio();

const dom = {
  panel: document.getElementById("panel")!,
  pulse: document.getElementById("pulse")!,
  title: document.getElementById("title")!,
  subtitle: document.getElementById("subtitle")!,
  chart: document.getElementById("chart") as unknown as SVGSVGElement,
  legend: document.getElementById("legend")!,
};

const PANEL_W = 380;
const CHART_W = 352;
const CHART_H = 190;
const MAX_SERIES = 5;
/** Table discovery: breadth-first from the window root, bounded. */
const SEARCH_MAX_NODES = 600;
/** Cells sit at table → row → cell (→ text, in some apps). */
const OBSERVE_DEPTH = 4;
/** Scans per window before giving up (Numbers needs a moment to build its table). */
const MAX_SCANS = 3;

interface Watched {
  windowId: AX.WindowId;
  tableId: AX.ElementId;
}

let watched: Watched | null = null;
let scanning: AX.WindowId | null = null;
/** Windows scanned without finding a table, and how many times. */
const misses = new Map<AX.WindowId, number>();
let renderQueued = false;

// --- Discovery ---

const TABLE_ROLES: AX.Role[] = ["table", "tree"];
/** Subtrees that never contain document tables. */
const SKIP_ROLES: AX.Role[] = ["toolbar", "menubar", "menu"];

async function findTable(windowId: AX.WindowId): Promise<TypedElement | null> {
  const root = await allio.windowRoot(windowId).catch(() => null);
  if (!root) return null;

  const found: TypedElement[] = [];
  const queue: TypedElement[] = [root];
  let visited = 0;
  while (queue.length && visited < SEARCH_MAX_NODES) {
    const el = queue.shift()!;
    visited++;
    if (TABLE_ROLES.includes(el.role)) {
      found.push(el);
      continue; // don't descend into tables while searching
    }
    if (SKIP_ROLES.includes(el.role)) continue;
    const kids = await allio.children(el.id).catch(() => []);
    queue.push(...kids);
  }
  // Prefer real tables, then the one with the most rows.
  found.sort(
    (a, b) =>
      Number(b.role === "table") - Number(a.role === "table") ||
      (b.row_count ?? 0) - (a.row_count ?? 0)
  );
  return found[0] ?? null;
}

async function onFocusChanged() {
  const win = allio.focused;
  if (!win || win.id === watched?.windowId || win.id === scanning) return;
  if ((misses.get(win.id) ?? 0) >= MAX_SCANS) return hidePanel();

  scanning = win.id;
  const table = await findTable(win.id);
  scanning = null;
  if (allio.focused?.id !== win.id) return; // focus moved on while scanning

  await stopWatching();
  if (!table) {
    misses.set(win.id, (misses.get(win.id) ?? 0) + 1);
    hidePanel();
    return;
  }
  misses.delete(win.id);
  watched = { windowId: win.id, tableId: table.id };
  await allio.observe(table.id, { depth: OBSERVE_DEPTH, wait_between_ms: 250 });
  queueRender();
}

async function stopWatching() {
  if (!watched) return;
  const { tableId } = watched;
  watched = null;
  await allio.unobserve(tableId).catch(() => {});
}

// --- Grid extraction ---

/** iWork appends the cell kind to header cell titles ("A, Row header cell"). */
const HEADER_SUFFIX = /(^|,\s*)(Column header cell|Row header cell)$/;

function cleanText(s: string): string {
  let out = s.trim();
  while (HEADER_SUFFIX.test(out)) out = out.replace(HEADER_SUFFIX, "").trim();
  return out;
}

function valueText(el: TypedElement): string | null {
  const v = el.value;
  if (v == null) return null;
  if (typeof v === "object") return null; // colors aren't cell content
  return String(v);
}

/** A cell's text: its own value/label, else its first descendant that has one. */
function cellText(cell: TypedElement, depth = 0): string {
  const own = valueText(cell) ?? cell.label;
  if (own != null && own !== "") return cleanText(own);
  if (depth > 2) return "";
  for (const child of allio.getChildren(cell)) {
    const t = cellText(child, depth + 1);
    if (t) return t;
  }
  return "";
}

function collectCells(tableId: AX.ElementId): TypedElement[] {
  const cells = new Map<AX.ElementId, TypedElement>();
  const stack: [AX.ElementId, number][] = [[tableId, 0]];
  while (stack.length) {
    const [id, depth] = stack.pop()!;
    const el = allio.get(id);
    if (!el) continue;
    if (el.role === "cell" && el.row_index != null && el.column_index != null) {
      cells.set(el.id, el);
      continue;
    }
    if (depth < 2) for (const c of el.children ?? []) stack.push([c, depth + 1]);
  }
  return [...cells.values()];
}

function buildGrid(tableId: AX.ElementId): string[][] {
  const grid: string[][] = [];
  for (const cell of collectCells(tableId)) {
    const r = cell.row_index!;
    const c = cell.column_index!;
    (grid[r] ??= [])[c] = cellText(cell);
  }
  const width = Math.max(0, ...grid.map((row) => row?.length ?? 0));
  return Array.from(grid, (row) => Array.from({ length: width }, (_, c) => row?.[c] ?? ""));
}

// --- Chart model ---

function parseNumber(s: string): number | null {
  const t = s.replace(/[\s,$€£¥%]/g, "");
  if (!/^-?(\d+\.?\d*|\.\d+)(e-?\d+)?$/i.test(t)) return null;
  return Number(t);
}

interface Series {
  name: string;
  values: (number | null)[];
}

interface ChartModel {
  labels: string[];
  series: Series[];
}

function toChart(grid: string[][]): ChartModel | null {
  if (grid.length < 2) return null;
  const cols = grid[0].length;
  const isNumeric = (s: string) => s !== "" && parseNumber(s) !== null;

  // Header row: a first row with no numbers above rows that have some.
  const hasHeader = !grid[0].some(isNumeric) && grid.slice(1).some((r) => r.some(isNumeric));
  const body = hasHeader ? grid.slice(1) : grid;

  const numericShare = (c: number) => {
    const filled = body.filter((r) => r[c] !== "");
    return filled.length ? filled.filter((r) => isNumeric(r[c])).length / filled.length : 0;
  };
  const seriesCols = [...Array(cols).keys()].filter((c) => numericShare(c) >= 0.6).slice(0, MAX_SERIES);
  if (!seriesCols.length) return null;

  // Labels: the first mostly-text column, else row numbers.
  const labelCol = [...Array(cols).keys()].find(
    (c) => !seriesCols.includes(c) && body.some((r) => r[c] !== "")
  );

  const rows = body.filter((r) => seriesCols.some((c) => isNumeric(r[c])));
  return {
    labels: rows.map((r, i) => (labelCol != null && r[labelCol]) || String(i + 1)),
    series: seriesCols.map((c) => ({
      name: (hasHeader && grid[0][c]) || `Column ${String.fromCharCode(65 + (c % 26))}`,
      values: rows.map((r) => parseNumber(r[c])),
    })),
  };
}

// --- Rendering ---

const SVG = "http://www.w3.org/2000/svg";

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>) {
  const el = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
}

function niceMax(v: number): number {
  if (v <= 0) return 1;
  const mag = 10 ** Math.floor(Math.log10(v));
  return [1, 2, 2.5, 5, 10].map((m) => m * mag).find((m) => m >= v)!;
}

function drawChart(model: ChartModel) {
  const pad = { l: 34, r: 6, t: 6, b: 22 };
  const w = CHART_W - pad.l - pad.r;
  const h = CHART_H - pad.t - pad.b;
  const all = model.series.flatMap((s) => s.values).filter((v): v is number => v != null);
  const lo = Math.min(0, ...all);
  const hi = niceMax(Math.max(...all, 0));
  const y = (v: number) => pad.t + h - ((v - lo) / (hi - lo || 1)) * h;
  const n = model.labels.length;
  const line = n > 16; // many points read better as lines

  dom.chart.replaceChildren();

  for (let i = 0; i <= 4; i++) {
    const v = lo + ((hi - lo) * i) / 4;
    dom.chart.append(svg("line", { x1: pad.l, x2: pad.l + w, y1: y(v), y2: y(v), stroke: "var(--grid)" }));
    const label = svg("text", { x: pad.l - 5, y: y(v) + 3, "text-anchor": "end" });
    label.textContent = Number(v.toPrecision(3)).toLocaleString();
    dom.chart.append(label);
  }

  const step = w / Math.max(n, 1);
  model.series.forEach((s, si) => {
    const color = `var(--s${si})`;
    if (line) {
      const pts = s.values
        .map((v, i) => (v == null ? null : `${pad.l + step * (i + 0.5)},${y(v)}`))
        .filter(Boolean)
        .join(" ");
      dom.chart.append(svg("polyline", { points: pts, fill: "none", stroke: color, "stroke-width": 1.75 }));
      return;
    }
    const groupW = step * 0.78;
    const barW = groupW / model.series.length;
    s.values.forEach((v, i) => {
      if (v == null) return;
      const x = pad.l + step * i + (step - groupW) / 2 + barW * si;
      const top = Math.min(y(v), y(0));
      dom.chart.append(
        svg("rect", { x, y: top, width: Math.max(barW - 1, 1), height: Math.abs(y(v) - y(0)), fill: color, rx: 1.5 })
      );
    });
  });

  // X labels, thinned to fit.
  const every = Math.ceil(n / Math.floor(w / 42));
  model.labels.forEach((text, i) => {
    if (i % every) return;
    const label = svg("text", { x: pad.l + step * (i + 0.5), y: CHART_H - 6, "text-anchor": "middle" });
    label.textContent = text.length > 8 ? `${text.slice(0, 7)}…` : text;
    dom.chart.append(label);
  });

  dom.legend.replaceChildren(
    ...model.series.map((s, si) => {
      const span = document.createElement("span");
      span.style.setProperty("--swatch", `var(--s${si})`);
      span.textContent = s.name;
      return span;
    })
  );
}

function positionPanel(win: AX.Window) {
  const { x, y, w, h } = win.bounds;
  const fitsRight = x + w + 16 + PANEL_W <= window.innerWidth;
  const left = fitsRight ? x + w + 16 : x + w - PANEL_W - 16;
  const top = fitsRight ? y + 48 : y + h - dom.panel.offsetHeight - 16;
  Object.assign(dom.panel.style, { left: `${left}px`, top: `${Math.max(top, 0)}px` });
}

function hidePanel() {
  dom.panel.style.display = "none";
}

function flashPulse() {
  dom.pulse.classList.add("on");
  requestAnimationFrame(() => requestAnimationFrame(() => dom.pulse.classList.remove("on")));
}

function render() {
  renderQueued = false;
  const win = watched && allio.windows.get(watched.windowId);
  const table = watched && allio.get(watched.tableId);
  if (!win || !table) return hidePanel();

  const grid = buildGrid(table.id);
  const model = toChart(grid);
  dom.title.textContent = `${win.app_name ?? "Table"} — ${table.label ?? table.description ?? "table"}`;
  dom.subtitle.textContent = model
    ? `${model.labels.length} rows · ${model.series.length} numeric column${model.series.length === 1 ? "" : "s"} · live`
    : `${grid.length}×${grid[0]?.length ?? 0} cells · no numeric columns to chart`;
  dom.chart.style.display = model ? "block" : "none";
  dom.legend.style.display = model ? "flex" : "none";
  if (model) drawChart(model);

  dom.panel.style.display = "block";
  positionPanel(win);
  flashPulse();
}

function queueRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(render);
}

// --- Wiring ---

async function init() {
  await allio.connect();

  allio.on("subtree:changed", ({ root_id }) => {
    if (root_id === watched?.tableId) queueRender();
  });
  allio.on("element:removed", ({ element_id }) => {
    if (element_id === watched?.tableId) stopWatching().then(hidePanel);
  });
  allio.on("window:changed", ({ window }) => {
    if (window.id === watched?.windowId) positionPanel(window);
  });
  allio.on("window:removed", ({ window_id }) => {
    misses.delete(window_id);
    if (window_id === watched?.windowId) stopWatching().then(hidePanel);
  });
  allio.on("focus:window", () => void onFocusChanged());

  // Apps like Numbers build their table tree lazily after allio announces itself;
  // re-scan the focused window until a table appears.
  setInterval(() => {
    if (!watched) void onFocusChanged();
  }, 2000);

  void onFocusChanged();
}

init();
