/** X-ray: a hole punched through the window under the cursor. */
import manifest from "../shaders/xray.json";
import wgsl from "../shaders/xray.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const { allio } = connect();
const fx = allio.shader({
  region: screen(),
  wgsl,
  ...declared(manifest),
  values: { radius: 70, thickness: 6, warp: 1 },
});

const showError = panel("X-ray", null, [
  { range: "radius", min: 10, max: 240, step: 1, value: 70, onInput: (radius) => fx.set({ radius }) },
  { range: "thickness", min: 2, max: 24, step: 1, value: 6, onInput: (thickness) => fx.set({ thickness }) },
  { range: "warp", min: -1, max: 1, step: 0.1, value: 1, onInput: (warp) => fx.set({ warp }) },
]);
fx.onerror = showError;

// Ask the host to leave the window under the cursor out of `behind`, so the shader can show what
// is behind it. The shader finds the same window itself, from the host-bound `windows`.
let hidden: number | null = null;
allio.on("mouse:position", ({ x, y }) => {
  const under = allio.windowAt(x, y);
  const id = under?.id ?? null;
  if (id === hidden) return;
  hidden = id;
  fx.behind = id === null ? "none" : { windows: [id] };
});
