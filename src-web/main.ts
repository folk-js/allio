import { register } from "@tauri-apps/plugin-global-shortcut";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

const appWindow = getCurrentWebviewWindow();
let clickthrough = false;
let allWindows: WindowInfo[] = [];
let activeWindow: WindowInfo | null = null;

// Register keyboard shortcuts
await register("CommandOrControl+Shift+E", () => {
  console.log("Toggling clickthrough");
  clickthrough = !clickthrough;
  appWindow.setIgnoreCursorEvents(clickthrough);
});

await register("CommandOrControl+Shift+W", async () => {
  try {
    const windows = (await invoke("get_all_windows")) as WindowInfo[];
    console.log("All windows:", windows);

    const active = (await invoke(
      "get_active_window_info"
    )) as WindowInfo | null;
    console.log("Active window:", active);
  } catch (error) {
    console.error("Error getting window info:", error);
  }
});

// Function to create outline elements for all windows
function updateAllOutlines(windows: WindowInfo[]) {
  // Clear existing outlines
  const existingOutlines = document.querySelectorAll(".window-outline");
  existingOutlines.forEach((outline) => outline.remove());

  // Create outlines for each window
  windows.forEach((window, index) => {
    const outlineElement = document.createElement("div");
    outlineElement.className = "window-outline";
    outlineElement.style.position = "fixed";
    outlineElement.style.left = `${window.x}px`;
    outlineElement.style.top = `${window.y}px`;
    outlineElement.style.width = `${window.w}px`;
    outlineElement.style.height = `${window.h}px`;
    outlineElement.style.border = "3px solid #ff0000";
    outlineElement.style.background = "transparent";
    outlineElement.style.pointerEvents = "none";
    outlineElement.style.opacity = "0.6";
    outlineElement.style.boxSizing = "border-box";
    outlineElement.style.zIndex = "9999";

    // Make active window more prominent
    if (activeWindow && window.id === activeWindow.id) {
      outlineElement.style.border = "4px solid #00ff00";
      outlineElement.style.opacity = "0.9";
    }

    document.body.appendChild(outlineElement);
  });
}

// Function to update window info display
function updateWindowInfo(windows: WindowInfo[], active: WindowInfo | null) {
  const detailsElement = document.getElementById("window-details");
  if (!detailsElement) return;

  let html = `<div style="margin-top: 10px;">
    <strong>Total Windows: ${windows.length}</strong>`;

  if (active) {
    html += `
      <div style="margin-top: 5px;">
        <strong>Active:</strong> ${active.name}
        <div>Size: ${Math.round(active.w)}x${Math.round(active.h)}</div>
        <div>Position: (${Math.round(active.x)}, ${Math.round(active.y)})</div>
      </div>`;
  }

  html += "</div>";
  detailsElement.innerHTML = html;
}

// Function to fetch and update window information
async function fetchWindowInfo() {
  try {
    const [windows, active] = await Promise.all([
      invoke("get_all_windows") as Promise<WindowInfo[]>,
      invoke("get_active_window_info") as Promise<WindowInfo | null>,
    ]);

    allWindows = windows;
    activeWindow = active;

    updateAllOutlines(windows);
    updateWindowInfo(windows, active);
  } catch (error) {
    console.error("Error fetching window info:", error);
    // Clear outlines on error
    const existingOutlines = document.querySelectorAll(".window-outline");
    existingOutlines.forEach((outline) => outline.remove());
  }
}

// Initialize and set up high-frequency updates (every 10ms)
fetchWindowInfo();
setInterval(fetchWindowInfo, 10);

console.log("Overlay app initialized with x-win crate for all windows");
