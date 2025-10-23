import { AXIO, AXNode } from "./axio";

const axio = new AXIO();
const output = document.getElementById("output")!;

function formatValue(value: any): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "object") {
    if (value.type && value.value !== undefined) {
      return `${value.type}(${value.value})`;
    }
    return JSON.stringify(value);
  }
  return String(value);
}

function property(key: string, value: any): string {
  if (value === undefined || value === null) return "";
  return `<div class="property"><span class="property-key">${key}</span><span class="property-value">${value}</span></div>`;
}

function renderWindows(windows: readonly AXNode[]) {
  if (windows.length === 0) {
    output.innerHTML = '<div class="connecting">No windows detected</div>';
    return;
  }

  let html = "";

  windows.forEach((window) => {
    const focusedClass = window.focused ? "focused" : "";
    html += `<div class="window-item ${focusedClass}">`;
    html += `<div class="window-title">${
      window.title || "Untitled Window"
    }</div>`;

    html += property("id", window.id);
    html += property("role", window.role);
    html += property("pid", window.pid);
    html += property("path", `[${window.path.join(", ")}]`);

    if (window.value) {
      html += property("value", formatValue(window.value));
    }
    if (window.description) {
      html += property("description", window.description);
    }
    if (window.subrole) {
      html += property("subrole", window.subrole);
    }

    html += property("focused", window.focused);
    html += property("enabled", window.enabled);

    if (window.selected !== undefined) {
      html += property("selected", window.selected);
    }

    if (window.bounds) {
      html += property(
        "position",
        `(${window.bounds.position.x}, ${window.bounds.position.y})`
      );
      html += property(
        "size",
        `${window.bounds.size.width} × ${window.bounds.size.height}`
      );
    }

    html += property(
      "children",
      `${window.children_count} (${window.children.length} loaded)`
    );

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
