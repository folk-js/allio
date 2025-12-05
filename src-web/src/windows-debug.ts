import { AXIO } from "@axio/client";

const axio = new AXIO();
const output = document.getElementById("output")!;

function render() {
  const windows = [...axio.windows.values()];

  if (windows.length === 0) {
    output.innerHTML = '<div class="connecting">No windows detected</div>';
    return;
  }

  output.innerHTML = windows
    .map((w) => {
      const { x, y, w: width, h: height } = w.bounds;
      return `
        <div class="window-item ${w.focused ? "focused" : ""}">
          <div class="window-title">${w.title || w.app_name || "Untitled"}</div>
          <div class="property"><span class="property-key">id</span><span class="property-value">${
            w.id
          }</span></div>
          <div class="property"><span class="property-key">app</span><span class="property-value">${
            w.app_name
          }</span></div>
          <div class="property"><span class="property-key">position</span><span class="property-value">(${x}, ${y})</span></div>
          <div class="property"><span class="property-key">size</span><span class="property-value">${width} × ${height}</span></div>
        </div>
      `;
    })
    .join("");
}

// Single pattern: connect, then render on any window/focus change
axio.connect().then(() => {
  // sync:init already populated axio.windows, just render
  render();

  // Re-render on any change
  const events = [
    "sync:init",
    "window:added",
    "window:changed",
    "window:removed",
    "focus:changed",
    "active:changed",
  ] as const;
  events.forEach((e) => axio.on(e, render));
});
