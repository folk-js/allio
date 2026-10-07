/** Lights out: the desktop lit by global illumination, with a lamp on the pointer. */
import manifest from "../shaders/light.json";
import wgsl from "../shaders/light.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const values = { lamp: 40, glow: 4, density: 0.02, ambient: 0.15, exposure: 3 };
const { allio } = connect();
const fx = allio.shader({ region: screen(), wgsl, ...declared(manifest), values });

const range = (name: keyof typeof values, max: number, step: number) => ({
  range: name,
  min: 0,
  max,
  step,
  value: values[name],
  onInput: (v: number) => fx.set({ [name]: v }),
});
const showError = panel("Lights out", "The pointer carries a lamp; colourful things in windows glow.", [
  range("lamp", 150, 1),
  range("glow", 20, 0.1),
  range("density", 0.2, 0.001),
  range("ambient", 1, 0.01),
  range("exposure", 10, 0.1),
]);
fx.onerror = showError;
