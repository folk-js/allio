/**
 * Frame (foundational) — the allio half of the allio <-> patchwork bridge.
 *
 * This overlay proves the read/observe/write path that the Patchwork frame
 * tool relies on. It is app-agnostic: pick any window and describe its list
 * with Allio's query syntax (the same one the `query` demo uses), e.g.
 *
 *   (tree) listitem { checkbox:done textfield:text }
 *
 * The overlay then:
 *   1. Binds to the chosen app's window and observes its subtree.
 *   2. Extracts `{ done, text }` rows per the query, keeping element refs.
 *   3. Renders a live desktop "card" and mirrors edits back via set()/perform().
 */

import {
  Allio,
  AX,
  AllioPassthrough,
  findFirst,
  parseQuerySyntax,
  accepts,
  type TypedElement,
} from "allio";

const DEFAULT_QUERY = "(tree) listitem { checkbox:done textfield:text }";
const BOOLEAN_ROLES = new Set(["checkbox", "switch", "radiobutton"]);
const TEXT_ROLES = new Set([
  "textfield",
  "textarea",
  "statictext",
  "searchfield",
]);

interface Row {
  checkbox: TypedElement | null;
  textEl: TypedElement | null;
  done: boolean;
  text: string;
}

interface Link {
  app: string;
  query: string;
}

interface Binding extends Link {
  windowId: AX.WindowId;
  rootId: AX.ElementId;
}

// --- State ---

let allio: Allio;
let link: Link | null = null;
let binding: Binding | null = null;
let pickerOpen = true;
/** Suppress re-render while the user is typing in a text field. */
let editingText = false;

const dom = {
  cards: document.getElementById("cards")!,
  outline: document.getElementById("windowOutline")!,
  wires: document.getElementById("wires") as unknown as SVGSVGElement,
};

// --- Init ---

async function init() {
  allio = new Allio();
  // Auto passthrough: clicks outside our opaque cards fall through to the apps
  // beneath. The instance stays alive via the mouse listener it registers.
  new AllioPassthrough(allio, { mode: "auto" });

  allio.on("sync:init", () => {
    tryBind();
    render();
  });
  allio.on("window:added", () => {
    tryBind();
    render();
  });
  allio.on("window:changed", render);
  allio.on("window:removed", ({ window_id }) => {
    if (binding?.windowId === window_id) unbind();
    render();
  });
  allio.on("subtree:changed", ({ root_id }) => {
    if (binding && root_id === binding.rootId) render();
  });

  // The server pushes a full `sync:init` on connect (see the sync:init handler
  // above), so we don't call the snapshot RPC. tryBind/render here are a
  // best-effort first paint; sync:init will re-run them once state arrives.
  await allio.connect();
  tryBind();
  render();
}

// --- Query mapping ---

function parseLink(query: string) {
  const parsed = parseQuerySyntax(query);
  const extract = parsed.extract ?? {};
  const doneRole =
    extract.done ??
    Object.values(extract).find((r) => BOOLEAN_ROLES.has(r)) ??
    "checkbox";
  const textRole =
    extract.text ??
    Object.values(extract).find((r) => TEXT_ROLES.has(r)) ??
    "textfield";
  const tokens = (parsed.match || "listitem")
    .trim()
    .split(/\s+/)
    .filter((t) => t && t !== ">");
  const itemRole = (tokens[tokens.length - 1] ?? "listitem").toLowerCase();
  return { container: parsed.find, itemRole, doneRole, textRole };
}

// --- Binding to the native window ---

function appNames(): string[] {
  const names = new Set<string>();
  for (const w of allio.windows.values()) {
    if (w.app_name && w.app_name !== "allio") names.add(w.app_name);
  }
  return [...names].sort();
}

function findWindow(app: string): AX.Window | null {
  for (const w of allio.windows.values()) {
    if (w.app_name === app) return w;
  }
  return null;
}

async function tryBind() {
  if (!link || binding) return;
  const win = findWindow(link.app);
  if (!win) return;

  try {
    const root = await allio.windowRoot(win.id);
    if (!root) return;

    const { container } = parseLink(link.query);
    let observeId = root.id;
    if (container) {
      let found = findFirst(allio, root.id, container);
      if (!found) {
        await allio.children(root.id);
        found = findFirst(allio, root.id, container);
      }
      if (found) observeId = found.id;
    }

    binding = {
      ...link,
      windowId: win.id,
      rootId: observeId,
    };
    await allio.observe(observeId, { depth: 12, wait_between_ms: 150 });
    console.log("[frame] bound to", link.app, "root", observeId);
  } catch (err) {
    console.error("[frame] bind failed", err);
  }
}

function unbind() {
  if (binding) {
    allio.unobserve(binding.rootId).catch(() => {});
    binding = null;
  }
}

function setLink(next: Link | null) {
  unbind();
  link = next;
  pickerOpen = !next;
  if (next) tryBind();
  render();
}

// --- Extraction (keeps element refs for write-back) ---

function descendants(rootId: AX.ElementId): TypedElement[] {
  const out: TypedElement[] = [];
  const queue = [rootId];
  const seen = new Set<AX.ElementId>();
  while (queue.length) {
    const id = queue.shift()!;
    if (seen.has(id)) continue;
    seen.add(id);
    const el = allio.elements.get(id);
    if (!el) continue;
    if (id !== rootId) out.push(el);
    for (const c of el.children ?? []) queue.push(c);
  }
  return out;
}

function firstByRole(rootId: AX.ElementId, role: string): TypedElement | null {
  return descendants(rootId).find((d) => d.role === role) ?? null;
}

function extractRows(): Row[] {
  if (!binding) return [];
  const { itemRole, doneRole, textRole } = parseLink(binding.query);
  const rows: Row[] = [];
  for (const d of descendants(binding.rootId)) {
    if (d.role !== itemRole) continue;
    const checkbox = firstByRole(d.id, doneRole);
    const textEl = firstByRole(d.id, textRole);
    const text =
      (typeof textEl?.value === "string" ? textEl.value : null) ??
      textEl?.label ??
      "";
    if (!text && !checkbox) continue;
    rows.push({ checkbox, textEl, done: checkbox?.value === true, text });
  }
  return rows;
}

// --- Write-back ---

async function toggleItem(row: Row, desired: boolean) {
  const cb = row.checkbox;
  if (!cb) return;
  try {
    if (cb.value === desired) return;
    // macOS checkboxes toggle via AXPress; a direct value set is usually a
    // no-op, so prefer pressing when the action is available.
    if (cb.actions.includes("press")) await allio.perform(cb.id, "press");
    else if (accepts(cb, "boolean")) await allio.set(cb, desired);
  } catch (err) {
    console.error("[frame] toggle failed", err);
  }
}

async function editItem(row: Row, value: string) {
  const el = row.textEl;
  if (!el) return;
  try {
    if (accepts(el, "string")) await allio.set(el, value);
  } catch (err) {
    console.error("[frame] edit failed", err);
  }
}

// --- Render ---

function render() {
  if (editingText) return;

  dom.cards.innerHTML = "";
  hideOutline();
  clearWires();

  if (!link || pickerOpen) {
    renderPickerCard();
    return;
  }

  if (!binding) {
    renderCard({
      title: `Waiting for ${link.app}…`,
      linked: false,
      body: emptyBody(`Open ${link.app}, or pick another app.`),
      onChange: () => {
        pickerOpen = true;
        render();
      },
    });
    return;
  }

  const win = allio.windows.get(binding.windowId);
  const rows = extractRows();

  const list = document.createElement("ul");
  list.className = "todo-list";
  if (rows.length === 0) {
    list.appendChild(emptyBody("No matching rows. Try editing the query."));
  } else {
    for (const row of rows) list.appendChild(renderRow(row));
  }

  const card = renderCard({
    title: win?.title || `${link.app}`,
    subtitle: `${rows.length} item${rows.length === 1 ? "" : "s"} · live`,
    linked: true,
    body: list,
    onChange: () => {
      pickerOpen = true;
      render();
    },
  });

  if (win) drawBinding(card, win);
}

function renderPickerCard() {
  const body = document.createElement("div");
  body.className = "picker";

  const appLabel = document.createElement("label");
  appLabel.textContent = "App";
  const appSelect = document.createElement("select");
  const names = appNames();
  if (link && !names.includes(link.app)) names.unshift(link.app);
  if (names.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = allio.connected ? "No apps detected" : "Connecting…";
    appSelect.appendChild(opt);
  }
  for (const name of names) {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    if (link?.app === name) opt.selected = true;
    appSelect.appendChild(opt);
  }
  appLabel.appendChild(appSelect);

  const queryLabel = document.createElement("label");
  queryLabel.textContent = "Query";
  const queryInput = document.createElement("input");
  queryInput.type = "text";
  queryInput.spellcheck = false;
  queryInput.value = link?.query ?? DEFAULT_QUERY;
  queryInput.addEventListener("keydown", (e) => e.stopPropagation());
  queryLabel.appendChild(queryInput);

  const actions = document.createElement("div");
  actions.className = "picker-actions";
  if (link) {
    const cancel = document.createElement("button");
    cancel.className = "card-btn";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", () => {
      pickerOpen = false;
      render();
    });
    actions.appendChild(cancel);
  }
  const linkBtn = document.createElement("button");
  linkBtn.className = "card-btn primary";
  linkBtn.textContent = "Link";
  linkBtn.addEventListener("click", () => {
    const app = appSelect.value.trim();
    if (!app) return;
    setLink({ app, query: queryInput.value.trim() || DEFAULT_QUERY });
  });
  actions.appendChild(linkBtn);

  body.append(appLabel, queryLabel, actions);

  renderCard({
    title: "Link an app",
    linked: false,
    body,
  });
}

function renderCard(opts: {
  title: string;
  subtitle?: string;
  linked: boolean;
  body: HTMLElement;
  onChange?: () => void;
}): HTMLElement {
  const card = document.createElement("div");
  card.className = "card";
  card.setAttribute("ax-io", "opaque");
  card.style.left = `${window.innerWidth - 380}px`;
  card.style.top = `80px`;

  const header = document.createElement("div");
  header.className = "card-header";

  const dot = document.createElement("span");
  dot.className = `status-dot${opts.linked ? " linked" : ""}`;
  header.appendChild(dot);

  const title = document.createElement("span");
  title.className = "card-title";
  title.textContent = opts.title;
  header.appendChild(title);

  if (opts.onChange) {
    const changeBtn = document.createElement("button");
    changeBtn.className = "card-btn";
    changeBtn.textContent = "⚙";
    changeBtn.title = "Change app / query";
    changeBtn.addEventListener("click", opts.onChange);
    header.appendChild(changeBtn);
  }

  card.appendChild(header);

  if (opts.subtitle) {
    const sub = document.createElement("div");
    sub.className = "card-sub";
    sub.textContent = opts.subtitle;
    card.appendChild(sub);
  }

  card.appendChild(opts.body);

  makeDraggable(card, header);
  dom.cards.appendChild(card);
  return card;
}

function renderRow(row: Row): HTMLElement {
  const li = document.createElement("li");
  li.className = `todo-row${row.done ? " done" : ""}`;

  const cb = document.createElement("input");
  cb.type = "checkbox";
  cb.checked = row.done;
  cb.disabled = !row.checkbox;
  cb.addEventListener("change", () => toggleItem(row, cb.checked));
  li.appendChild(cb);

  const text = document.createElement("input");
  text.type = "text";
  text.value = row.text;
  text.readOnly = !row.textEl || !accepts(row.textEl, "string");
  text.addEventListener("focus", () => (editingText = true));
  text.addEventListener("blur", () => {
    editingText = false;
    if (text.value !== row.text) editItem(row, text.value);
    render();
  });
  text.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") text.blur();
    if (e.key === "Escape") {
      text.value = row.text;
      text.blur();
    }
  });
  li.appendChild(text);

  return li;
}

function emptyBody(msg: string): HTMLElement {
  const el = document.createElement("div");
  el.className = "empty";
  el.textContent = msg;
  return el;
}

// --- Visual binding to the source window ---

function drawBinding(card: HTMLElement, win: AX.Window) {
  const { x, y, w, h } = win.bounds;
  Object.assign(dom.outline.style, {
    left: `${x}px`,
    top: `${y}px`,
    width: `${w}px`,
    height: `${h}px`,
    display: "block",
  });

  const cardRect = card.getBoundingClientRect();
  const x1 = x + w;
  const y1 = y + Math.min(h / 2, 120);
  const x2 = cardRect.left;
  const y2 = cardRect.top + 20;
  const curve = Math.min(Math.abs(x2 - x1) / 2, 80);

  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("class", "wire");
  path.setAttribute(
    "d",
    `M ${x1},${y1} C ${x1 + curve},${y1} ${x2 - curve},${y2} ${x2},${y2}`
  );
  dom.wires.appendChild(path);
}

function hideOutline() {
  dom.outline.style.display = "none";
}

function clearWires() {
  dom.wires.innerHTML = "";
}

// --- Card dragging ---

function makeDraggable(card: HTMLElement, handle: HTMLElement) {
  handle.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    if ((e.target as HTMLElement).tagName === "BUTTON") return;
    e.preventDefault();
    const startX = e.clientX;
    const startY = e.clientY;
    const rect = card.getBoundingClientRect();

    const onMove = (ev: MouseEvent) => {
      card.style.left = `${rect.left + (ev.clientX - startX)}px`;
      card.style.top = `${rect.top + (ev.clientY - startY)}px`;
      clearWires();
      const win = binding && allio.windows.get(binding.windowId);
      if (win) drawBinding(card, win);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });
}

document.addEventListener("DOMContentLoaded", init);
