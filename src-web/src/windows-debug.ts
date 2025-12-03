import { AXIO } from "./axio";
import { Window } from "./axio";

const axio = new AXIO();
const output = document.getElementById("output")!;

function property(key: string, value: any): string {
  if (value === undefined || value === null) return "";
  return `<div class="property"><span class="property-key">${key}</span><span class="property-value">${value}</span></div>`;
}

function renderWindows(windows: ReadonlyArray<Window>) {
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

    html += property("position", `(${window.x}, ${window.y})`);
    html += property("size", `${window.w} × ${window.h}`);

    html += `</div>`;
  });

  output.innerHTML = html;
}

async function init() {
  try {
    await axio.connect();
    output.innerHTML =
      '<div class="connecting">Connected. Waiting for windows...</div>';

    axio.onWindowUpdate((windows) => {
      renderWindows(windows);
    });

    if (axio.windows.length > 0) {
      renderWindows(axio.windows);
    }
  } catch (error) {
    output.innerHTML = `<div class="connecting">Error: ${error}</div>`;
    console.error(error);
  }
}

init();
