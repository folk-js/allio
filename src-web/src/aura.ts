/** Window auras: a glowing halo around every window, brighter near the cursor. */
import manifest from "../shaders/aura.json";
import wgsl from "../shaders/aura.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const { allio } = connect();
const fx = allio.shader({
  region: screen(),
  wgsl,
  ...declared(manifest),
  values: { width: 36, intensity: 1 },
});

const showError = panel("Window auras", "Move windows and the cursor around.", [
  { range: "width", min: 4, max: 120, step: 1, value: 36, onInput: (width) => fx.set({ width }) },
  { range: "intensity", min: 0, max: 2, step: 0.01, value: 1, onInput: (intensity) => fx.set({ intensity }) },
]);
fx.onerror = showError;
