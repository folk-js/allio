/** Shared setup for the screen shader demos. */
import { Allio, AllioPassthrough, type Hide, type Region, type Uniforms } from "allio";

/** The whole screen, in points: the overlay window is exactly that big. */
export const screen = (): Region => ({ x: 0, y: 0, w: innerWidth, h: innerHeight });

/** A demo's `.json` file: its uniforms and options, shared with the Rust tests. */
interface Manifest {
  uniforms: Record<string, string>;
  hide?: unknown;
  cell?: number;
  steps?: number;
  behind?: unknown;
}

/** The shader options a manifest declares. */
export const declared = (m: Manifest) => ({
  uniforms: m.uniforms as Uniforms,
  hide: m.hide as Hide | undefined,
  cell: m.cell,
  steps: m.steps,
  behind: m.behind as Hide | undefined,
});

/** Connects to allio, with the control panel clickable and everything else passing clicks through. */
export function connect(): { allio: Allio; passthrough: AllioPassthrough } {
  const allio = new Allio();
  const passthrough = new AllioPassthrough(allio);
  allio.connect();
  return { allio, passthrough };
}

type Control =
  | { range: string; min: number; max: number; step: number; value: number; onInput(value: number): void }
  | { toggle: string; value: boolean; onChange(value: boolean): void }
  | { button: string; onClick(): void };

/** A small control panel. Returns a function that shows an error (or clears it). */
export function panel(title: string, hint: string | null, controls: Control[]): (error: string | null) => void {
  const root = document.createElement("div");
  root.className = "demo-panel";
  root.setAttribute("ax-io", "opaque");
  root.append(Object.assign(document.createElement("h1"), { textContent: title }));

  for (const control of controls) {
    if ("button" in control) {
      const button = Object.assign(document.createElement("button"), { textContent: control.button });
      button.onclick = control.onClick;
      root.append(button);
      continue;
    }
    const label = document.createElement("label");
    const name = "range" in control ? control.range : control.toggle;
    const input = document.createElement("input");
    const out = document.createElement("output");
    if ("range" in control) {
      Object.assign(input, { type: "range", min: control.min, max: control.max, step: control.step, value: control.value });
      const show = () => (out.textContent = String(Math.round(Number(input.value) * 1000) / 1000));
      input.oninput = () => (show(), control.onInput(Number(input.value)));
      show();
    } else {
      Object.assign(input, { type: "checkbox", checked: control.value });
      input.onchange = () => control.onChange(input.checked);
    }
    label.append(Object.assign(document.createElement("span"), { textContent: name }), input, out);
    root.append(label);
  }

  if (hint) root.append(Object.assign(document.createElement("p"), { className: "hint", textContent: hint }));
  const error = Object.assign(document.createElement("div"), { className: "error" });
  root.append(error);
  document.body.append(root);
  return (message) => (error.textContent = message ?? "");
}
