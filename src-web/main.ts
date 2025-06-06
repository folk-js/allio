import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
}

interface UITreeNode {
  role: string;
  title?: string;
  value?: string;
  enabled: boolean;
  children: UITreeNode[];
  depth: number;
}

interface TextElement {
  id: string;
  role: string;
  title: string;
  current_value: string;
  is_editable: boolean;
  app_name: string;
}

// Add event payload interface
interface WindowUpdatePayload {
  windows: WindowInfo[];
}

let activeWindow: WindowInfo | null = null;
let currentWindows: WindowInfo[] = [];
let textElements: TextElement[] = [];
let uiTree: UITreeNode | null = null;

// Map to track outline elements by window ID
const outlineElements = new Map<string, HTMLDivElement>();

// Track when we last got a fresh tree
let lastTreeUpdate = 0;

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

// Function to update window info display

// Replace fetchWindowInfo with event listener
async function setupWindowListener() {
  try {
    await listen<WindowUpdatePayload>("window-update", (event) => {
      const { windows } = event.payload;

      currentWindows = windows;

      updateAllOutlines(windows);
    });

    console.log("Window update listener established");
  } catch (error) {
    console.error("Failed to setup window listener:", error);
  }
}

// Update accessibility UI tree - keep last good tree
async function updateUITree() {
  try {
    const result = (await invoke(
      "get_ui_tree_for_active_window"
    )) as UITreeNode | null;

    if (result) {
      uiTree = result;
      lastTreeUpdate = Date.now();
      console.log("UI Tree updated successfully");
    }

    updateUITreeDisplay();
    updateInfoPanel();
  } catch (error) {
    // Keep the last good tree
    updateUITreeDisplay();
    updateInfoPanel();
  }
}

// Separate function to update just the UI tree display
function updateUITreeDisplay() {
  const treeStatusElement = document.getElementById("tree-status");
  const treeContentElement = document.getElementById("tree-content");

  if (!treeStatusElement || !treeContentElement) return;

  if (uiTree) {
    const secondsAgo = Math.floor((Date.now() - lastTreeUpdate) / 1000);
    const staleText = secondsAgo > 10 ? ` (${secondsAgo}s old)` : "";
    treeStatusElement.innerHTML = `<div style="color: #4CAF50; margin-bottom: 10px;">✓ Accessibility Tree${staleText}</div>`;
    treeContentElement.innerHTML = renderAccessibilityTree(uiTree);
  } else {
    treeStatusElement.innerHTML = `<div style="color: #ff9800;">○ No tree available</div>`;
    treeContentElement.innerHTML = "";
  }
}

function updateInfoPanel() {
  let details = "";

  if (activeWindow) {
    details += `<div><strong>Active Window:</strong></div>`;
    details += `<div>Name: ${activeWindow.name}</div>`;
    details += `<div>Position: ${activeWindow.x}, ${activeWindow.y}</div>`;
    details += `<div>Size: ${activeWindow.w} × ${activeWindow.h}</div>`;
    details += `<div>ID: ${activeWindow.id}</div>`;
  }

  details += `<div style="margin-top: 10px;"><strong>Total Windows:</strong> ${currentWindows.length}</div>`;

  if (textElements.length > 0) {
    details += `<div style="margin-top: 10px;"><strong>Text Elements Found:</strong> ${textElements.length}</div>`;
    textElements.slice(0, 5).forEach((el) => {
      const statusIcon = el.is_editable ? "✎" : "👁";
      details += `<div style="font-size: 10px; margin-left: 10px;">${statusIcon} ${
        el.role
      }: ${el.title || "Untitled"}</div>`;
    });
    if (textElements.length > 5) {
      details += `<div style="font-size: 10px; margin-left: 10px; color: #666;">... ${
        textElements.length - 5
      } more</div>`;
    }
  }

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
  await updateUITree();

  // setInterval(updateUITree, 2000);
}

init();
