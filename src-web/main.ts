import { invoke } from "@tauri-apps/api/core";

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
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
    const isActive = activeWindow && window.id === activeWindow.id;

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
    treeContentElement.innerHTML = renderUITree(uiTree);
  } else {
    treeStatusElement.innerHTML = `<div style="color: #ff9800;">○ No tree available</div>`;
    treeContentElement.innerHTML = "";
  }
}

// Update accessibility text elements
async function updateTextElements() {
  try {
    // First try to get text elements from the active window's UI tree
    if (uiTree) {
      // Extract text elements from the current UI tree
      const extractedElements: TextElement[] = [];
      function extractTextFromTree(
        node: UITreeNode,
        appName: string = "Unknown"
      ) {
        const textRoles = [
          "AXTextField",
          "AXTextArea",
          "AXComboBox",
          "AXSearchField",
          "AXSecureTextField",
        ];
        if (textRoles.includes(node.role)) {
          extractedElements.push({
            id: `${node.role}_${node.depth}`,
            role: node.role,
            title: node.title || "Untitled",
            current_value: node.value || "",
            is_editable: node.enabled,
            app_name: appName,
          });
        }
        node.children.forEach((child) => extractTextFromTree(child, appName));
      }

      extractTextFromTree(uiTree);
      textElements = extractedElements;
    } else {
      // Fallback to the original method
      textElements = (await invoke("get_text_elements")) as TextElement[];
    }

    updateInfoPanel();
  } catch (error) {
    // Silently handle errors, just clear text elements
    textElements = [];
  }
}

// Insert text into active field
async function insertTextIntoActiveField(text: string) {
  try {
    await invoke("insert_text_active", { text });
    console.log(`Successfully inserted text: ${text}`);
  } catch (error) {
    console.error("Failed to insert text:", error);
    alert(`Failed to insert text: ${error}`);
  }
}

// Update info panel (remove UI tree part)
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

  details += `<div style="margin-top: 10px;"><strong>Accessibility Commands:</strong></div>`;
  details += `<div style="font-size: 10px;">Cmd+Shift+T: Insert "Hello World"</div>`;
  details += `<div style="font-size: 10px;">Cmd+Shift+U: Manual refresh</div>`;

  const detailsElement = document.getElementById("window-details");
  if (detailsElement) {
    detailsElement.innerHTML = details;
  }
}

// Register global shortcut for toggling clickthrough

// Initialize the application
async function init() {
  console.log("Initializing overlay application...");

  // Initial updates
  await fetchWindowInfo();
  await updateUITree();
  await updateTextElements();

  // Set up periodic updates
  setInterval(fetchWindowInfo, 100); // Window tracking every 100ms for smooth outline updates
  setInterval(updateUITree, 1000); // UI tree refresh every 1 second
  setInterval(updateTextElements, 5000); // Text elements refresh every 5 seconds

  console.log("Overlay application initialized");
}

// Start the application
init();

// Clean and terse JSON rendering for debugging
function renderUITree(node: UITreeNode): string {
  console.log("Full UI tree:", node);

  function cleanNode(n: UITreeNode): any {
    const cleaned: any = {
      role: n.role,
      depth: n.depth,
    };

    // Only include title if it exists and isn't empty
    if (n.title && n.title.trim() !== "") {
      cleaned.title = n.title;
    }

    // Only include value if it exists and isn't empty
    if (n.value && n.value.trim() !== "") {
      cleaned.value = n.value;
    }

    // Skip 'enabled' property entirely

    // Recursively clean children
    if (n.children && n.children.length > 0) {
      cleaned.children = n.children.map((child) => cleanNode(child));
    }

    return cleaned;
  }

  return JSON.stringify(cleanNode(node), null, 2);
}
