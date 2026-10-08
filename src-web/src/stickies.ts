/**
 * Stickies: notes stuck on windows, or left on a Space. A note on a window lives on that
 * window's Space, full-screen ones included, all the time: switch Spaces and it slides in and out
 * with its window. A note left on a Space stays on that Space. This page says only what each note
 * belongs to (`ax-on`); where that is, and showing it there, is allio's job.
 */
import { AllioBelonging, type AX } from "allio";
import { connect, panel } from "./shader-demo";

const { allio, passthrough } = connect();
new AllioBelonging(allio);

/** A note: what it belongs to, and where it is (relative to its window, or on its Space). */
interface Sticky {
  el: HTMLElement;
  owner: { window: AX.WindowId } | { space: AX.SpaceId };
  x: number;
  y: number;
}
const stickies = new Set<Sticky>();

panel("Stickies", "Notes stay with their window on its Space (full screen too), and slide with it.", [
  { button: "Stick a note on a window", onClick: () => pick() },
  { button: "Leave a note on this Space", onClick: () => noteOnThisSpace() },
]);

/** Where a note goes on screen. */
function place(s: Sticky) {
  let x = s.x;
  let y = s.y;
  if ("window" in s.owner) {
    const w = allio.windows.get(s.owner.window);
    if (!w) return;
    x += w.bounds.x;
    y += w.bounds.y;
  }
  Object.assign(s.el.style, { left: `${x}px`, top: `${y}px` });
}

function create(owner: Sticky["owner"], x: number, y: number) {
  const el = document.createElement("div");
  el.className = "window" in owner ? "sticky" : "sticky on-space";
  el.setAttribute("ax-io", "opaque");
  el.setAttribute("ax-on", "window" in owner ? `window:${owner.window}` : `space:${owner.space}`);
  const header = document.createElement("header");
  const close = Object.assign(document.createElement("div"), { className: "close" });
  header.append(close);
  const text = Object.assign(document.createElement("textarea"), { placeholder: "Note…" });
  el.append(header, text);
  document.body.append(el);

  const sticky: Sticky = { el, owner, x, y };
  stickies.add(sticky);
  place(sticky);
  close.onpointerdown = (e) => e.stopPropagation();
  close.onclick = () => {
    stickies.delete(sticky);
    el.remove();
  };
  header.onpointerdown = (down) => {
    down.preventDefault();
    header.setPointerCapture(down.pointerId);
    passthrough.mode = "opaque";
    const start = { x: sticky.x, y: sticky.y };
    header.onpointermove = (e) => {
      sticky.x = start.x + e.clientX - down.clientX;
      sticky.y = start.y + e.clientY - down.clientY;
      place(sticky);
    };
    header.onpointerup = () => {
      header.onpointermove = header.onpointerup = null;
      passthrough.mode = "auto";
    };
  };
  text.focus();
}

/** The next click chooses a window, and the note goes where you clicked on it. */
function pick() {
  const layer = Object.assign(document.createElement("div"), { className: "pick-layer" });
  layer.setAttribute("ax-io", "opaque");
  document.body.append(layer);
  passthrough.mode = "opaque";
  layer.onclick = (e) => {
    layer.remove();
    passthrough.mode = "auto";
    const w = allio.windowAt(e.clientX, e.clientY);
    if (w) create({ window: w.id }, e.clientX - w.bounds.x, e.clientY - w.bounds.y);
  };
}

function noteOnThisSpace() {
  const space = allio.spaces.find((s) => s.current);
  if (space) create({ space: space.id }, innerWidth / 2 - 95, innerHeight / 3);
}

allio.on("window:changed", ({ window }) => {
  for (const s of stickies) if ("window" in s.owner && s.owner.window === window.id) place(s);
});
allio.on("window:removed", ({ window_id }) => {
  for (const s of stickies) {
    if ("window" in s.owner && s.owner.window === window_id) {
      stickies.delete(s);
      s.el.remove();
    }
  }
});
