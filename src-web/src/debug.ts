/**
 * AXIO Window Viewer
 *
 * Displays all open windows using AXIO (Accessibility I/O) data.
 * Shows window details including fullscreen status.
 */

import { AXIO, AXNode } from "./axio";

// AXIO instance
const axio = new AXIO("ws://127.0.0.1:3030/ws");

// Parse app name from description field
function getAppName(window: AXNode): string | null {
  if (!window.description) return null;
  const match = window.description.match(/app=([^;]+)/);
  return match ? match[1] : null;
}

// Update the window display
function updateWindowDisplay() {
  const windowsContainer = document.getElementById("windows");
  if (!windowsContainer) return;

  const windows = axio.windows;

  // Build output
  const lines: string[] = [];

  windows.forEach((w) => {
    const app = getAppName(w) || "(unknown app)";
    const title = w.title || "(unknown title)";
    const b = w.bounds;

    lines.push('<div class="window-item">');

    lines.push(`app: ${app}`);
    lines.push(`title: ${title}`);
    lines.push(`id: ${w.id}`);
    lines.push(`pid: ${w.pid}`);
    lines.push(`role: ${w.role}`);
    if (w.focused) {
      lines.push(`focused: true`);
    }
    lines.push(
      `x: ${Math.round(b.position.x)}, y: ${Math.round(b.position.y)}`
    );
    lines.push(
      `w: ${Math.round(b.size.width)}, h: ${Math.round(b.size.height)}`
    );

    lines.push("</div>");
  });

  windowsContainer.innerHTML = lines.join("<br>");
}

// Initialize
async function init() {
  console.log("AXIO Window Viewer initializing...");

  // Connect to AXIO backend
  await axio.connect();

  // Listen for window updates
  axio.onWindowUpdate(() => {
    updateWindowDisplay();
  });
}

init();
