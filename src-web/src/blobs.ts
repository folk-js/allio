/** Window blobs: monochrome metaballs that wander the screen and fuse with windows into goo. */
import manifest from "../shaders/blobs.json";
import wgsl from "../shaders/blobs.wgsl?raw";
import { connect, declared, panel, screen } from "./shader-demo";

const defaults = { goo: 60, size: 14, count: 6, speed: 1, cling: 0.4, tone: 0.1, ball: 28, wobble: 8 };

const { allio } = connect();
const fx = allio.shader({
  region: screen(),
  wgsl,
  ...declared(manifest),
  values: defaults,
});

const showError = panel("Window blobs", null, [
  { range: "goo", min: 10, max: 150, step: 1, value: defaults.goo, onInput: (goo) => fx.set({ goo }) },
  { range: "blobs", min: 1, max: 12, step: 1, value: defaults.count, onInput: (count) => fx.set({ count }) },
  { range: "size", min: 6, max: 40, step: 1, value: defaults.size, onInput: (size) => fx.set({ size }) },
  { range: "speed", min: 0.2, max: 3, step: 0.1, value: defaults.speed, onInput: (speed) => fx.set({ speed }) },
  { range: "cling", min: 0, max: 2, step: 0.1, value: defaults.cling, onInput: (cling) => fx.set({ cling }) },
  { range: "wobble", min: 0, max: 20, step: 1, value: defaults.wobble, onInput: (wobble) => fx.set({ wobble }) },
  { range: "cursor", min: 0, max: 60, step: 1, value: defaults.ball, onInput: (ball) => fx.set({ ball }) },
]);
fx.onerror = showError;
