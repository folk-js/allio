import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Legacy type for debug overlay (uses old structure)
type WindowInfo = {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
  process_id: number;
};

interface UITreeNode {
  role: string;
  title?: string;
  value?: string;
  enabled: boolean;
  children: UITreeNode[];
  depth: number;
}

interface AppInfo {
  process_id: number;
  name: string;
  window_count: number;
  has_focused_window: boolean;
}

interface AccessibilityEvent {
  event_type: string;
  element_role: string;
  element_title?: string;
  element_value?: string;
  timestamp: number;
}

// Add event payload interface
interface WindowUpdatePayload {
  windows: WindowInfo[];
}

let currentWindows: WindowInfo[] = [];
let currentApps: AppInfo[] = [];
let selectedAppPid: number | null = null;
let uiTree: UITreeNode | null = null;
let isListeningForEvents: boolean = false;
let eventLog: AccessibilityEvent[] = [];

// Map to track outline elements by window ID
const outlineElements = new Map<string, HTMLDivElement>();

// Function to create a new outline element
function createOutlineElement(): HTMLDivElement {
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
    const isActive = window.focused;

    let element = outlineElements.get(window.id);
    if (!element) {
      // Create new outline element for new window
      element = createOutlineElement();
      outlineElements.set(window.id, element);
    }

    // Update element properties
    updateOutlineElement(element, window, !!isActive);
  });
}

// Function to group windows by app
function groupWindowsByApp(windows: WindowInfo[]): AppInfo[] {
  const appMap = new Map<number, AppInfo>();

  windows.forEach((window) => {
    const pid = window.process_id;

    if (!appMap.has(pid)) {
      appMap.set(pid, {
        process_id: pid,
        name: window.name.split(" - ")[0] || window.name, // Try to get app name from window title
        window_count: 0,
        has_focused_window: false,
      });
    }

    const app = appMap.get(pid)!;
    app.window_count++;
    if (window.focused) {
      app.has_focused_window = true;
    }
  });

  return Array.from(appMap.values()).sort((a, b) => {
    // Sort by focused first, then by name
    if (a.has_focused_window && !b.has_focused_window) return -1;
    if (!a.has_focused_window && b.has_focused_window) return 1;
    return a.name.localeCompare(b.name);
  });
}

// Function to update app buttons
function updateAppButtons(apps: AppInfo[]) {
  const appListElement = document.getElementById("app-list");
  if (!appListElement) return;

  appListElement.innerHTML = "";

  apps.forEach((app) => {
    const button = document.createElement("button");
    button.className = "app-button";
    button.style.cssText = `
      display: block;
      width: 100%;
      margin: 5px 0;
      padding: 10px;
      border: 2px solid ${app.has_focused_window ? "#4CAF50" : "#ccc"};
      background: ${selectedAppPid === app.process_id ? "#e3f2fd" : "white"};
      border-radius: 5px;
      cursor: pointer;
      text-align: left;
      font-size: 14px;
    `;

    const statusIcon = app.has_focused_window ? "🎯" : "📱";
    button.innerHTML = `
      ${statusIcon} <strong>${app.name}</strong><br>
      <small>PID: ${app.process_id} • ${app.window_count} window${
      app.window_count !== 1 ? "s" : ""
    }</small>
    `;

    button.addEventListener("click", () => {
      selectApp(app.process_id);
    });

    appListElement.appendChild(button);
  });

  // Add event listener controls
  const eventControls = document.createElement("div");
  eventControls.style.cssText = `
    margin-top: 20px;
    padding-top: 15px;
    border-top: 1px solid #333;
  `;

  const eventButton = document.createElement("button");
  eventButton.style.cssText = `
    width: 100%;
    padding: 10px;
    border: 2px solid ${isListeningForEvents ? "#f44336" : "#4CAF50"};
    background: ${isListeningForEvents ? "#ffebee" : "#e8f5e9"};
    color: ${isListeningForEvents ? "#f44336" : "#4CAF50"};
    border-radius: 5px;
    cursor: pointer;
    font-weight: bold;
  `;

  eventButton.textContent = isListeningForEvents
    ? "🛑 Stop Event Listening"
    : "🎧 Start Event Listening";

  eventButton.addEventListener("click", toggleEventListening);

  eventControls.appendChild(eventButton);

  if (eventLog.length > 0) {
    const eventCount = document.createElement("div");
    eventCount.style.cssText = `
      margin-top: 10px;
      font-size: 12px;
      color: #666;
      text-align: center;
    `;
    eventCount.textContent = `📝 ${eventLog.length} events logged`;
    eventControls.appendChild(eventCount);
  }

  appListElement.appendChild(eventControls);
}

// Function to select an app and fetch its accessibility tree
async function selectApp(pid: number) {
  selectedAppPid = pid;
  updateAppButtons(currentApps); // Refresh button styles

  try {
    console.log(`Fetching accessibility tree for PID: ${pid}`);
    const result = (await invoke("get_ui_tree_by_pid", { pid })) as UITreeNode;
    uiTree = result;
    console.log("UI Tree fetched successfully");
  } catch (error) {
    console.error("Failed to fetch UI tree:", error);
    uiTree = null;
  }

  updateUITreeDisplay();
  updateInfoPanel();
}

// Toggle event listening
async function toggleEventListening() {
  try {
    if (isListeningForEvents) {
      await invoke("stop_accessibility_events");
      isListeningForEvents = false;
      console.log("Stopped listening for accessibility events");
    } else {
      await invoke("start_accessibility_events");
      isListeningForEvents = true;
      console.log("Started listening for accessibility events");
    }
    updateAppButtons(currentApps); // Refresh button styles
  } catch (error) {
    console.error("Failed to toggle event listening:", error);
  }
}

// Replace fetchWindowInfo with event listener
async function setupWindowListener() {
  try {
    await listen<WindowUpdatePayload>("window-update", (event) => {
      const { windows } = event.payload;

      currentWindows = windows;
      currentApps = groupWindowsByApp(windows);

      updateAllOutlines(windows);
      updateAppButtons(currentApps);
      updateInfoPanel();
    });

    console.log("Window update listener established");

    // Setup accessibility event listener
    await listen<AccessibilityEvent>("accessibility-event", (event) => {
      const accessibilityEvent = event.payload;
      eventLog.push(accessibilityEvent);

      // Keep only last 50 events
      if (eventLog.length > 50) {
        eventLog.shift();
      }

      console.log("🔔 Accessibility Event:", accessibilityEvent);
      updateAppButtons(currentApps); // Refresh to show event count
    });
  } catch (error) {
    console.error("Failed to setup window listener:", error);
  }
}

// Separate function to update just the UI tree display
function updateUITreeDisplay() {
  const treeStatusElement = document.getElementById("tree-status");
  const treeContentElement = document.getElementById("tree-content");

  if (!treeStatusElement || !treeContentElement) return;

  if (uiTree && selectedAppPid) {
    const selectedApp = currentApps.find(
      (app) => app.process_id === selectedAppPid
    );
    const appName = selectedApp ? selectedApp.name : `PID ${selectedAppPid}`;
    treeStatusElement.innerHTML = `<div style="color: #4CAF50; margin-bottom: 10px;">✓ Accessibility Tree - ${appName}</div>`;
    treeContentElement.innerHTML = renderAccessibilityTree(uiTree);
  } else if (selectedAppPid) {
    treeStatusElement.innerHTML = `<div style="color: #ff9800;">⚠ Failed to load tree for PID ${selectedAppPid}</div>`;
    treeContentElement.innerHTML = "";
  } else {
    treeStatusElement.innerHTML = `<div style="color: #999;">Select an app to view its accessibility tree</div>`;
    treeContentElement.innerHTML = "";
  }
}

function updateInfoPanel() {
  let details = "";

  if (selectedAppPid) {
    const selectedApp = currentApps.find(
      (app) => app.process_id === selectedAppPid
    );
    if (selectedApp) {
      details += `<div><strong>Selected App:</strong></div>`;
      details += `<div>Name: ${selectedApp.name}</div>`;
      details += `<div>PID: ${selectedApp.process_id}</div>`;
      details += `<div>Windows: ${selectedApp.window_count}</div>`;
      details += `<div>Focused: ${
        selectedApp.has_focused_window ? "Yes" : "No"
      }</div>`;
    }
  }

  details += `<div style="margin-top: 10px;"><strong>Total Apps:</strong> ${currentApps.length}</div>`;
  details += `<div><strong>Total Windows:</strong> ${currentWindows.length}</div>`;

  const detailsElement = document.getElementById("window-details");
  if (detailsElement) {
    detailsElement.innerHTML = details;
  }
}

function renderAccessibilityTree(node: UITreeNode): string {
  const simpleNode = (n: UITreeNode): Record<string, any> => ({
    role: n.role,
    ...(n.title?.trim() && { title: n.title }),
    ...(n.value?.trim() && { value: n.value }),
    ...(n.children.length && { children: n.children.map(simpleNode) }),
  });

  return JSON.stringify(simpleNode(node), null, 2);
}

async function init() {
  await setupWindowListener();
}

init();
