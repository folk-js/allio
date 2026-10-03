import {
  Allio,
  AX,
  accepts,
  findFirst,
  parseQuerySyntax,
  type TypedElement,
} from "allio";
import type { DocHandle } from "@automerge/automerge-repo";
import type { AllioLink, Todo, TodoDoc } from "./types";

/** A todo row extracted from the native accessibility tree. */
interface NativeItem {
  text: string;
  done: boolean;
  checkbox?: TypedElement;
  textEl?: TypedElement;
}

const BOOLEAN_ROLES = new Set(["checkbox", "switch", "radiobutton"]);
const TEXT_ROLES = new Set(["textfield", "textarea", "statictext", "searchfield"]);

function uuid(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : Math.random().toString(36).slice(2);
}

function textValue(el: TypedElement | undefined): string {
  if (!el) return "";
  return (typeof el.value === "string" ? el.value : null) ?? el.label ?? "";
}

/** The last role token of a selector, e.g. "tree > listitem" -> "listitem". */
function lastRole(selector: string): string {
  const tokens = selector.trim().split(/\s+/).filter((t) => t && t !== ">");
  return tokens[tokens.length - 1]?.toLowerCase() ?? "";
}

/**
 * A live, bidirectional bridge between a native app surface (via Allio) and a
 * Patchwork `todo` document (via Automerge).
 *
 * The mapping is fully query-driven (`link.query`), so this works for any app
 * whose a11y tree can be described as rows with a boolean + a text field.
 *
 * Reconciliation is positional (native row N <-> todo N). Echo loops are
 * prevented with the `applyingTo*` guards.
 */
export function createTodoBridge(
  allio: Allio,
  handle: DocHandle<TodoDoc>,
  link: AllioLink
) {
  const parsed = parseQuerySyntax(link.query);
  const containerSelector = parsed.find; // e.g. "tree" (undefined -> window root)
  const itemRole = lastRole(parsed.match || "listitem") || "listitem";
  // Map query fields onto the two todo fields. Prefer explicit field names
  // "done"/"text"; otherwise infer from the role's value kind.
  const extract = parsed.extract ?? {};
  const doneRole =
    extract.done ??
    Object.values(extract).find((r) => BOOLEAN_ROLES.has(r)) ??
    "checkbox";
  const textRole =
    extract.text ??
    Object.values(extract).find((r) => TEXT_ROLES.has(r)) ??
    "textfield";

  let rootId: AX.ElementId | null = null;
  let lastItems: NativeItem[] = [];
  let applyingToDoc = false;
  let applyingToNative = false;
  let stopped = false;

  /** BFS over the cached subtree (excludes the root id itself). */
  function descendants(id: AX.ElementId): TypedElement[] {
    const out: TypedElement[] = [];
    const queue: AX.ElementId[] = [id];
    const seen = new Set<AX.ElementId>();
    while (queue.length) {
      const cur = queue.shift()!;
      if (seen.has(cur)) continue;
      seen.add(cur);
      const el = allio.elements.get(cur);
      if (!el) continue;
      if (cur !== id) out.push(el);
      for (const child of el.children ?? []) queue.push(child);
    }
    return out;
  }

  function firstByRole(id: AX.ElementId, role: string): TypedElement | undefined {
    return descendants(id).find((d) => d.role === role);
  }

  // `rootId` is the *container* we observe (e.g. the `tree`), so items are just
  // its descendants matching `itemRole` — no further findFirst needed here.
  function extractItems(): NativeItem[] {
    if (rootId == null) return [];
    const items: NativeItem[] = [];
    for (const el of descendants(rootId)) {
      if (el.role !== itemRole) continue;
      const checkbox = firstByRole(el.id, doneRole);
      const textEl = firstByRole(el.id, textRole);
      const text = textValue(textEl).trim();
      if (!text && !checkbox) continue;
      items.push({ text, done: checkbox?.value === true, checkbox, textEl });
    }
    return items;
  }

  /** native -> doc */
  function syncToDoc() {
    if (rootId == null || applyingToNative) return;
    const items = extractItems();
    lastItems = items;

    const doc = handle.doc();
    const same =
      !!doc &&
      items.length === doc.todos.length &&
      items.every((n, i) => {
        const t = doc.todos[i];
        return t && t.description === n.text && t.done === n.done;
      });
    if (same) return;

    applyingToDoc = true;
    try {
      handle.change((d) => {
        const todos = d.todos;
        for (let i = 0; i < items.length; i++) {
          const n = items[i];
          if (i < todos.length) {
            if (todos[i].description !== n.text) todos[i].description = n.text;
            if (todos[i].done !== n.done) todos[i].done = n.done;
          } else {
            todos.push({ id: uuid(), description: n.text, done: n.done } as Todo);
          }
        }
        while (todos.length > items.length) todos.pop();
      });
    } finally {
      applyingToDoc = false;
    }
  }

  /** doc -> native */
  async function syncToNative() {
    if (rootId == null || applyingToDoc) return;
    const todos = handle.doc()?.todos ?? [];
    applyingToNative = true;
    try {
      for (let i = 0; i < todos.length && i < lastItems.length; i++) {
        const t = todos[i];
        const n = lastItems[i];
        if (n.checkbox && n.done !== t.done) {
          const cb = n.checkbox;
          // macOS checkboxes toggle via AXPress; setting AXValue directly is
          // usually a no-op, so prefer pressing when the action is available.
          if (cb.actions.includes("press")) await allio.perform(cb.id, "press");
          else if (accepts(cb, "boolean")) await allio.set(cb, t.done);
          n.done = t.done;
        }
        if (n.textEl && accepts(n.textEl, "string") && n.text !== t.description) {
          await allio.set(n.textEl, t.description);
          n.text = t.description;
        }
      }
    } finally {
      applyingToNative = false;
    }
  }

  async function bind() {
    const win = [...allio.windows.values()].find((w) => w.app_name === link.app);
    if (!win) return;
    const root = await allio.windowRoot(win.id);
    if (!root || stopped) return;

    // Descend to the container (e.g. `tree`) and observe *that*, matching the
    // proven overlay path. Observing the whole window root deeply both misses
    // items and thrashes the backend tree (reparenting errors).
    let observeId = root.id;
    if (containerSelector) {
      let found = findFirst(allio, root.id, containerSelector);
      if (!found) {
        await allio.children(root.id);
        found = findFirst(allio, root.id, containerSelector);
      }
      if (found) observeId = found.id;
    }
    if (stopped) return;
    rootId = observeId;
    await allio.observe(observeId, { depth: 12, wait_between_ms: 150 });
    syncToDoc();
  }

  const onSubtree = ({ root_id }: { root_id: AX.ElementId }) => {
    if (root_id === rootId) syncToDoc();
  };
  const onSyncInit = () => void bind();
  const onDocChange = () => {
    if (!applyingToDoc) void syncToNative();
  };

  return {
    async start() {
      stopped = false;
      allio.on("subtree:changed", onSubtree);
      allio.on("sync:init", onSyncInit);
      handle.on("change", onDocChange);
      await bind();
    },
    stop() {
      stopped = true;
      allio.off("subtree:changed", onSubtree);
      allio.off("sync:init", onSyncInit);
      handle.off("change", onDocChange);
      if (rootId != null) void allio.unobserve(rootId);
      rootId = null;
    },
    get bound() {
      return rootId != null;
    },
  };
}

export type TodoBridge = ReturnType<typeof createTodoBridge>;
