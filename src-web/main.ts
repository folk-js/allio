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
let activeWindow: WindowInfo | null = null;

// Map to track outline elements by window ID
const outlineElements = new Map<string, HTMLDivElement>();

// Function to create a new outline element
function createOutlineElement(window: WindowInfo): HTMLDivElement {
  const outlineElement = document.createElement("div");
  outlineElement.className = "window-outline";
  outlineElement.style.position = "fixed";
  outlineElement.style.border = "3px solid #ff0000";
  outlineElement.style.background = "transparent";
  outlineElement.style.pointerEvents = "none";
  outlineElement.style.opacity = "0.6";
  outlineElement.style.boxSizing = "border-box";
  outlineElement.style.zIndex = "9999";

  document.body.appendChild(outlineElement);
  return outlineElement;
}

// Function to update outline element properties
function updateOutlineElement(
  element: HTMLDivElement,
  window: WindowInfo,
  isActive: boolean
) {
  element.style.left = `${window.x}px`;
  element.style.top = `${window.y}px`;
  element.style.width = `${window.w}px`;
  element.style.height = `${window.h}px`;
  element.style.borderRadius = "10px";

  if (isActive) {
    element.style.border = "2px solid #00ff00";
    element.style.opacity = "0.9";
  } else {
    element.style.border = "2px solid #ff0000";
    element.style.opacity = "0.6";
  }
}

// Function to efficiently update outlines
function updateAllOutlines(windows: WindowInfo[]) {
  const currentWindowIds = new Set(windows.map((w) => w.id));

  // Remove outline elements for closed windows
  for (const [windowId, element] of outlineElements) {
    if (!currentWindowIds.has(windowId)) {
      element.remove();
      outlineElements.delete(windowId);
    }
  }

  // Update existing and create new outline elements
  windows.forEach((window) => {
    const isActive = activeWindow && window.id === activeWindow.id;

    let element = outlineElements.get(window.id);
    if (!element) {
      // Create new outline element for new window
      element = createOutlineElement(window);
      outlineElements.set(window.id, element);
    }

    // Update element properties
    updateOutlineElement(element, window, !!isActive);
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

    activeWindow = active;

    updateAllOutlines(windows);
    updateWindowInfo(windows, active);
  } catch (error) {
    console.error("Error fetching window info:", error);
    // Clear all outlines on error
    outlineElements.forEach((element) => element.remove());
    outlineElements.clear();
  }
}

// Initialize and set up high-frequency updates (every 10ms)
fetchWindowInfo();
setInterval(fetchWindowInfo, 5);
