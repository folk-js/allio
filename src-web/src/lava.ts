/** Lava that eats windows. The simulation runs natively; this page only turns the tap on and off. */
import manifest from "../shaders/lava.json";
import wgsl from "../shaders/lava.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const { allio, passthrough } = connect();
const fx = allio.shader({
  region: screen(),
  wgsl,
  ...declared(manifest),
  values: { pour: 0, radius: 40, rate: 0.012, reset: 0 },
});

let wipes = 0;
const showError = panel("Lava", "Click and hold in empty space to pour. Paper and plants burn, metal heats up, blue turns lava to stone.", [
  { range: "radius", min: 10, max: 240, step: 1, value: 40, onInput: (radius) => fx.set({ radius }) },
  { range: "burn rate", min: 0.002, max: 0.05, step: 0.001, value: 0.012, onInput: (rate) => fx.set({ rate }) },
  { button: "Wipe", onClick: () => fx.set({ reset: ++wipes }) },
]);
fx.onerror = showError;

// Clicks in empty space pour lava; clicks on a window go to the window, so it can still be focused
// and moved. A burnt-out hole counts as empty space: it shows the desktop, so clicking there
// pours too. The panel keeps its own clicks. While the button is down the overlay holds on to the
// pointer, so dragging over a window doesn't drop the pour.
let pouring = false;
let inHole = false;
let probing = false;
let mode = "";
const setMode = (next: "opaque" | "outside") => {
  if (next === mode) return; // changing the mode makes the passthrough re-evaluate, so only do it when needed
  mode = next;
  passthrough.mode = next;
};
const overPanel = (x: number, y: number) =>
  document.elementsFromPoint(x, y).some((el) => el.closest(".demo-panel"));
const overWindow = (x: number, y: number) => allio.windowAt(x, y) !== null;

setMode("outside");
allio.on("mouse:position", ({ x, y }) => {
  if (pouring) return;
  const settle = () => setMode(overPanel(x, y) || inHole ? "opaque" : "outside");
  if (!overWindow(x, y)) inHole = false;
  else if (!probing) {
    // One question to the simulation at a time: is the cell under the cursor burnt through?
    probing = true;
    fx.probe(x, y)
      .then(([, burn]) => (inHole = burn >= 1))
      .catch(() => (inHole = false))
      .finally(() => ((probing = false), settle()));
  }
  settle();
});

const pour = (on: boolean) => {
  pouring = on;
  fx.set({ pour: on ? 1 : 0 });
  if (on) setMode("opaque");
};
addEventListener("pointerdown", (e) => {
  if (!(e.target as Element).closest(".demo-panel")) pour(true);
});
for (const end of ["pointerup", "pointercancel", "blur"]) {
  addEventListener(end, () => pouring && pour(false));
}
