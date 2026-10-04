/**
 * A fixed magnifier that the pointer agrees with. Put it over a toolbar or some small text, move
 * the pointer through it, and click what you see: inside the lens the pointer really is over the
 * magnified thing under it, so it moves more slowly over the real screen and lands exactly.
 *
 * The shader and the pointer field use the same map; this page only places the lens and draws the
 * pointer while the system cursor is hidden.
 */
import manifest from "../shaders/lens.json";
import wgsl from "../shaders/lens.wgsl?raw";
import { connect, declared, drawPointer, panel, screen } from "./shader-demo";

const { allio, passthrough } = connect();

const lens = { x: innerWidth / 2, y: innerHeight / 2, r: 150, mag: 3 };

const fx = allio.shader({ region: screen(), wgsl, ...declared(manifest) });
const field = allio.pointer();
drawPointer(allio);

const showError = panel("Lens", "Drag the handle to put the lens over something small, then point and click through it.", [
  { range: "magnify", min: 1, max: 8, step: 0.1, value: lens.mag, onInput: (mag) => ((lens.mag = mag), update()) },
  { range: "radius", min: 40, max: 400, step: 1, value: lens.r, onInput: (r) => ((lens.r = r), update()) },
]);
fx.onerror = showError;

const handle = Object.assign(document.createElement("div"), { className: "chrome drag-handle" });
handle.setAttribute("ax-io", "opaque");
document.body.append(handle);

function update() {
  Object.assign(handle.style, { left: `${lens.x}px`, top: `${lens.y - lens.r - 22}px` });
  fx.set({ lens: [lens.x, lens.y, lens.r, lens.mag] });
  field.set({ lenses: lens.mag > 1 ? [{ ...lens }] : [] });
}

handle.addEventListener("pointerdown", (down) => {
  const start = { ...lens };
  handle.setPointerCapture(down.pointerId);
  passthrough.mode = "opaque";
  const move = (e: PointerEvent) => {
    lens.x = start.x + e.clientX - down.clientX;
    lens.y = start.y + e.clientY - down.clientY;
    update();
  };
  const up = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", up);
    passthrough.mode = "auto";
  };
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", up);
});

update();
