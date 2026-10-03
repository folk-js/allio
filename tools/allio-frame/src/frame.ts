import type { DocHandle, Repo, AutomergeUrl } from "@automerge/automerge-repo";
import { Allio } from "allio";
import { createTodoBridge, type TodoBridge } from "./bridge";
import {
  DEFAULT_QUERY,
  TODO_TYPE,
  type AllioFrameDoc,
  type Embed,
  type TodoDoc,
} from "./types";
import "./styles.css";

type ToolElement = HTMLElement & { repo: Repo };

function uuid(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : Math.random().toString(36).slice(2);
}

export function renderAllioFrame(
  handle: DocHandle<AllioFrameDoc>,
  element: ToolElement
): () => void {
  const repo = element.repo;
  const allio = new Allio(undefined, 5000, { debug: false });

  const bridges = new Map<string, TodoBridge>();
  const cards = new Map<string, HTMLElement>();
  let disposed = false;

  /** Distinct native app names currently visible to Allio (minus Allio itself). */
  function appNames(): string[] {
    const names = new Set<string>();
    for (const w of allio.windows.values()) {
      if (w.app_name && w.app_name !== "allio") names.add(w.app_name);
    }
    return [...names].sort();
  }

  // --- Scaffold -------------------------------------------------------------
  element.innerHTML = "";
  const root = document.createElement("div");
  root.className = "allio-frame";

  const toolbar = document.createElement("div");
  toolbar.className = "allio-frame-toolbar";
  const addBtn = document.createElement("button");
  addBtn.textContent = "+ Todo card";
  const status = document.createElement("span");
  status.className = "allio-frame-status";
  status.textContent = "Allio: connecting…";
  toolbar.append(addBtn, status);

  const cardsLayer = document.createElement("div");
  cardsLayer.style.position = "absolute";
  cardsLayer.style.inset = "0";

  root.append(cardsLayer, toolbar);
  element.appendChild(root);

  // --- Connection -----------------------------------------------------------
  function setStatus(connected: boolean) {
    status.className = `allio-frame-status ${
      connected ? "connected" : "disconnected"
    }`;
    status.textContent = connected ? "Allio: connected" : "Allio: offline";
  }

  // The server pushes a full `sync:init` snapshot on every connection, so we
  // never call the (broken-for-unit-variants) snapshot RPC. The client
  // auto-reconnects on close, so a single connect() is enough — retrying it
  // here would leak a new socket per attempt.
  allio.on("sync:init", () => setStatus(true));
  const statusPoll = window.setInterval(() => setStatus(allio.connected), 2000);

  // --- Doc helpers ----------------------------------------------------------
  function embeds(): Embed[] {
    return handle.doc()?.embeds ?? [];
  }

  function updateEmbed(id: string, fn: (e: Embed) => void) {
    handle.change((doc) => {
      const e = doc.embeds.find((x) => x.id === id);
      if (e) fn(e);
    });
  }

  async function addTodoCard() {
    const todo = repo.create<TodoDoc>();
    todo.change((doc) => {
      doc["@patchwork"] = { type: TODO_TYPE };
      doc.title = "Todo";
      doc.todos = [];
    });
    handle.change((doc) => {
      if (!doc.embeds) doc.embeds = [];
      doc.embeds.push({
        id: uuid(),
        docUrl: todo.url,
        toolId: TODO_TYPE,
        x: 60 + doc.embeds.length * 28,
        y: 80 + doc.embeds.length * 28,
        width: 300,
        height: 340,
      });
    });
  }

  // --- Card rendering -------------------------------------------------------
  function makeCard(embed: Embed): HTMLElement {
    const card = document.createElement("div");
    card.className = "allio-card";
    card.dataset.embedId = embed.id;

    const header = document.createElement("div");
    header.className = "allio-card-header";
    const title = document.createElement("span");
    title.className = "allio-card-title";
    title.textContent = "Todo";
    const linkBtn = document.createElement("button");
    linkBtn.textContent = "Link app…";
    const closeBtn = document.createElement("button");
    closeBtn.textContent = "✕";
    header.append(title, linkBtn, closeBtn);

    const body = document.createElement("div");
    body.className = "allio-card-body";
    const view = document.createElement("patchwork-view");
    view.setAttribute("doc-url", embed.docUrl);
    if (embed.toolId) view.setAttribute("tool-id", embed.toolId);
    body.appendChild(view);

    const resize = document.createElement("div");
    resize.className = "allio-card-resize";
    resize.innerHTML =
      '<svg viewBox="0 0 14 14" width="14" height="14"><path d="M13 5 L5 13 M13 9 L9 13" stroke="currentColor" fill="none"/></svg>';

    card.append(header, body, resize);

    // Drag via header
    header.addEventListener("pointerdown", (e) => {
      if ((e.target as HTMLElement).tagName === "BUTTON") return;
      e.preventDefault();
      const start = { x: e.clientX, y: e.clientY };
      const origin = { x: embed.x, y: embed.y };
      const onMove = (ev: PointerEvent) => {
        const nx = origin.x + (ev.clientX - start.x);
        const ny = origin.y + (ev.clientY - start.y);
        card.style.left = `${nx}px`;
        card.style.top = `${ny}px`;
      };
      const onUp = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        updateEmbed(embed.id, (x) => {
          x.x = parseFloat(card.style.left);
          x.y = parseFloat(card.style.top);
        });
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    });

    // Resize via handle
    resize.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      const start = { x: e.clientX, y: e.clientY };
      const origin = { w: embed.width, h: embed.height };
      const onMove = (ev: PointerEvent) => {
        const w = Math.max(220, origin.w + (ev.clientX - start.x));
        const h = Math.max(160, origin.h + (ev.clientY - start.y));
        card.style.width = `${w}px`;
        card.style.height = `${h}px`;
      };
      const onUp = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        updateEmbed(embed.id, (x) => {
          x.width = parseFloat(card.style.width);
          x.height = parseFloat(card.style.height);
        });
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    });

    linkBtn.addEventListener("click", () => openLinkPanel(embed, card));

    closeBtn.addEventListener("click", () => {
      handle.change((doc) => {
        const i = doc.embeds.findIndex((x) => x.id === embed.id);
        if (i >= 0) doc.embeds.splice(i, 1);
      });
    });

    return card;
  }

  // --- Link panel (pick app + query) ---------------------------------------
  function openLinkPanel(embed: Embed, card: HTMLElement) {
    card.querySelector(".allio-link-panel")?.remove();

    const panel = document.createElement("div");
    panel.className = "allio-link-panel";
    panel.addEventListener("pointerdown", (e) => e.stopPropagation());

    const appRow = document.createElement("label");
    appRow.textContent = "App";
    const appSelect = document.createElement("select");
    const names = appNames();
    if (embed.allioLink && !names.includes(embed.allioLink.app)) {
      names.unshift(embed.allioLink.app);
    }
    if (names.length === 0) {
      const opt = document.createElement("option");
      opt.value = "";
      opt.textContent = allio.connected
        ? "No apps detected"
        : "Allio offline";
      appSelect.appendChild(opt);
    }
    for (const name of names) {
      const opt = document.createElement("option");
      opt.value = name;
      opt.textContent = name;
      if (embed.allioLink?.app === name) opt.selected = true;
      appSelect.appendChild(opt);
    }
    appRow.appendChild(appSelect);

    const queryRow = document.createElement("label");
    queryRow.textContent = "Query";
    const queryInput = document.createElement("input");
    queryInput.type = "text";
    queryInput.spellcheck = false;
    queryInput.value = embed.allioLink?.query ?? DEFAULT_QUERY;
    queryRow.appendChild(queryInput);

    const actions = document.createElement("div");
    actions.className = "allio-link-actions";
    const saveBtn = document.createElement("button");
    saveBtn.textContent = "Link";
    saveBtn.className = "primary";
    const cancelBtn = document.createElement("button");
    cancelBtn.textContent = "Cancel";
    const unlinkBtn = document.createElement("button");
    unlinkBtn.textContent = "Unlink";

    saveBtn.addEventListener("click", () => {
      const app = appSelect.value.trim();
      if (!app) return;
      const query = queryInput.value.trim() || DEFAULT_QUERY;
      updateEmbed(embed.id, (x) => {
        x.allioLink = { app, query };
      });
      panel.remove();
    });
    cancelBtn.addEventListener("click", () => panel.remove());
    unlinkBtn.addEventListener("click", () => {
      updateEmbed(embed.id, (x) => {
        x.allioLink = undefined;
      });
      panel.remove();
    });

    actions.append(saveBtn);
    if (embed.allioLink) actions.append(unlinkBtn);
    actions.append(cancelBtn);
    panel.append(appRow, queryRow, actions);
    card.appendChild(panel);
    appSelect.focus();
  }

  function syncCards() {
    const current = embeds();
    const seen = new Set<string>();

    for (const embed of current) {
      seen.add(embed.id);
      let card = cards.get(embed.id);
      if (!card) {
        card = makeCard(embed);
        cards.set(embed.id, card);
        cardsLayer.appendChild(card);
      }
      card.style.left = `${embed.x}px`;
      card.style.top = `${embed.y}px`;
      card.style.width = `${embed.width}px`;
      card.style.height = `${embed.height}px`;
      card.classList.toggle("linked", !!embed.allioLink);
      const linkBtn = card.querySelector<HTMLButtonElement>(
        ".allio-card-header button"
      );
      if (linkBtn) {
        linkBtn.textContent = embed.allioLink
          ? `Linked: ${embed.allioLink.app}`
          : "Link app…";
        linkBtn.classList.toggle("linked", !!embed.allioLink);
      }
    }

    for (const [id, card] of cards) {
      if (!seen.has(id)) {
        card.remove();
        cards.delete(id);
      }
    }

    void syncBridges(current);
  }

  // --- Bridge lifecycle -----------------------------------------------------
  const bridgeSigs = new Map<string, string>();

  async function syncBridges(current: Embed[]) {
    const linked = new Map(
      current.filter((e) => e.allioLink).map((e) => [e.id, e])
    );

    for (const [id, embed] of linked) {
      const sig = JSON.stringify(embed.allioLink);
      if (bridges.get(id) && bridgeSigs.get(id) === sig) continue;
      // New link, or the app/query changed: (re)start the bridge.
      bridges.get(id)?.stop();
      const todoHandle = await repo.find<TodoDoc>(embed.docUrl as AutomergeUrl);
      if (disposed) return;
      const bridge = createTodoBridge(allio, todoHandle, embed.allioLink!);
      bridges.set(id, bridge);
      bridgeSigs.set(id, sig);
      await bridge.start();
    }

    for (const [id, bridge] of bridges) {
      if (!linked.has(id)) {
        bridge.stop();
        bridges.delete(id);
        bridgeSigs.delete(id);
      }
    }
  }

  // --- Wire up --------------------------------------------------------------
  addBtn.addEventListener("click", () => void addTodoCard());
  handle.on("change", syncCards);

  allio.connect().catch(() => setStatus(false));
  if (handle.doc()) {
    syncCards();
  } else {
    handle.once("change", syncCards);
  }

  // --- Cleanup --------------------------------------------------------------
  return () => {
    disposed = true;
    window.clearInterval(statusPoll);
    handle.off("change", syncCards);
    for (const bridge of bridges.values()) bridge.stop();
    bridges.clear();
    allio.disconnect();
    element.innerHTML = "";
  };
}
