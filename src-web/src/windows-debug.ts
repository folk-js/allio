import { AXIO, AXWindow } from "@axio/client";

const axio = new AXIO();
const output = document.getElementById("output")!;

function property(key: string, value: any): string {
  if (value === undefined || value === null) return "";
  return `<div class="property"><span class="property-key">${key}</span><span class="property-value">${value}</span></div>`;
}

function renderWindows(windows: ReadonlyArray<AXWindow>) {
  if (windows.length === 0) {
    output.innerHTML = '<div class="connecting">No windows detected</div>';
    return;
  }

  let html = "";

  windows.forEach((window) => {
    const focusedClass = window.focused ? "focused" : "";
    html += `<div class="window-item ${focusedClass}">`;
    html += `<div class="window-title">${
      window.title || window.app_name || "Untitled Window"
    }</div>`;

    html += property("id", window.id);
    html += property("app_name", window.app_name);
    html += property("focused", window.focused);

    const { x, y, w, h } = window.bounds;
    html += property("position", `(${x}, ${y})`);
    html += property("size", `${w} × ${h}`);

    html += `</div>`;
  });

  output.innerHTML = html;
}

async function init() {
  try {
    await axio.connect();
    output.innerHTML =
      '<div class="connecting">Connected. Waiting for windows...</div>';

    const updateWindows = () => renderWindows([...axio.windows.values()]);

    // Use new event names
    axio.on("window:added", updateWindows);
    axio.on("window:changed", updateWindows);
    axio.on("window:removed", updateWindows);
    axio.on("focus:changed", updateWindows);
    axio.on("active:changed", updateWindows);

    if (axio.windows.size > 0) {
      renderWindows([...axio.windows.values()]);
    }
  } catch (error) {
    output.innerHTML = `<div class="connecting">Error: ${error}</div>`;
    console.error(error);
  }
}

init();
