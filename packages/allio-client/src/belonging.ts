/**
 * AllioBelonging - UI that belongs to a window, an element or a Space, and lives there.
 *
 * The page is one document shown on every Space. An element that says what it belongs to is
 * shown where that thing is, all the time: on its window's Space (full-screen ones included),
 * sliding in and out with it when you swipe between Spaces.
 *
 *   const belonging = new AllioBelonging(allio);
 *
 *   <div ax-on="window:4521">…</div>  <!-- on that window's Space -->
 *   <div ax-on="element:88">…</div>   <!-- on that element's window's Space -->
 *   <div ax-on="space:1295">…</div>   <!-- on that Space -->
 *   <div>…</div>                      <!-- with you: on every Space, as before -->
 *
 * Positions are screen points, as everywhere else: an element on Space 2 is placed where it
 * should appear there. Owned elements are moved into their Space's container (a full-screen,
 * fixed layer), so position them with `position: absolute` or `fixed` and don't rely on where they
 * were in the DOM. Only the current Space's container takes the pointer and keyboard.
 *
 * When the owner is on no Space (minimised, hidden, closed or not known yet), the element is
 * marked `ax-away`, which a default rule hides. Style `[ax-away]` to show it some other way.
 *
 * How it is shown: each Space's container is hidden in the page itself, and the host shows it on
 * its Space through a mirror window (see `docs/SPACES.md`). Containers are created once and never
 * moved or hidden, so the host can always find their layers.
 */

import type { Allio, AX } from "./index";

const STYLE_ID = "allio-belonging-style";
const STYLE = `
  /* Every Space's container, hidden here: each is shown on its own Space by the host. */
  #allio-spaces { position: fixed; inset: 0; opacity: 0.0001; will-change: opacity;
    pointer-events: none; z-index: 2147483647 }
  .allio-space { position: fixed; inset: 0; will-change: transform; pointer-events: none }
  :where(.allio-space) > * { pointer-events: auto }
  :where([ax-away]) { display: none }
`;

type Container = { el: HTMLElement; marker: number };

export class AllioBelonging {
  private readonly wrapper: HTMLElement;
  private readonly containers = new Map<AX.SpaceId, Container>();
  private nextMarker = 1;
  private scheduled = false;
  private sent = "";
  private readonly observer: MutationObserver;

  constructor(private readonly allio: Allio) {
    if (!document.getElementById(STYLE_ID)) {
      const style = Object.assign(document.createElement("style"), { id: STYLE_ID, textContent: STYLE });
      document.head.append(style);
    }
    this.wrapper = Object.assign(document.createElement("div"), { id: "allio-spaces" });
    document.body.append(this.wrapper);

    const schedule = () => this.schedule();
    allio.on("sync:init", () => {
      this.sent = ""; // a new connection: declare the layers again
      schedule();
    });
    for (const event of [
      "spaces:changed",
      "window:added",
      "window:changed",
      "window:removed",
      "element:added",
      "element:removed",
    ] as const) {
      allio.on(event, schedule);
    }
    this.observer = new MutationObserver(schedule);
    this.observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["ax-on"],
    });
    schedule();
  }

  /** The Space an owner (`window:ID`, `element:ID` or `space:ID`) is on, or null if none. */
  spaceOf(owner: string): AX.SpaceId | null {
    const [kind, raw] = owner.split(":");
    const id = Number(raw);
    if (kind === "space") return this.allio.spaces.some((s) => s.id === id) ? id : null;
    const windowId =
      kind === "window" ? id : kind === "element" ? this.allio.elements.get(id)?.window_id : undefined;
    const window = windowId === undefined ? undefined : this.allio.windows.get(windowId);
    if (!window?.spaces.length) return null;
    // A window on several Spaces (on all desktops) is shown on the current one.
    const current = window.spaces.find((s) => this.allio.spaces.find((x) => x.id === s)?.current);
    return current ?? window.spaces[0];
  }

  /** Stops managing the page (owned elements stay where they are). */
  dispose(): void {
    this.observer.disconnect();
    this.allio.spaceLayers([]).catch(() => {});
  }

  private schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.reconcile();
    });
  }

  private reconcile(): void {
    const spaces = this.allio.spaces;
    for (const space of spaces) {
      if (this.containers.has(space.id)) continue;
      const marker = this.nextMarker++;
      const el = Object.assign(document.createElement("div"), {
        id: `allio-space-${space.id}`,
        className: "allio-space",
      });
      // Renders exactly like 1; lets the host tell this container's layer from any other.
      el.style.opacity = String(1 - marker / 100000);
      this.wrapper.append(el);
      this.containers.set(space.id, { el, marker });
    }

    for (const el of document.querySelectorAll<HTMLElement>("[ax-on]")) {
      const space = this.spaceOf(el.getAttribute("ax-on")!);
      const container = space === null ? undefined : this.containers.get(space);
      if (!container) {
        el.setAttribute("ax-away", "");
        continue;
      }
      el.removeAttribute("ax-away");
      if (el.parentElement !== container.el) container.el.append(el);
    }

    // Spaces that are gone (a full-screen window left full screen): their elements are away.
    const live = new Set(spaces.map((s) => s.id));
    for (const [id, container] of this.containers) {
      if (live.has(id)) continue;
      for (const child of [...container.el.children]) {
        child.setAttribute("ax-away", "");
        document.body.append(child);
      }
      container.el.remove();
      this.containers.delete(id);
    }

    // Only the current Space's content takes the pointer and keyboard.
    for (const space of spaces) {
      const container = this.containers.get(space.id);
      if (container) container.el.inert = !space.current;
    }

    const layers = [...this.containers].map(([space, c]) => ({ space, marker: c.marker }));
    const key = JSON.stringify(layers);
    if (key !== this.sent && this.allio.connected) {
      this.sent = key;
      this.allio.spaceLayers(layers).catch(() => (this.sent = ""));
    }
  }
}
