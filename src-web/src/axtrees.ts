import { InterlayClient } from "./interlay-client.ts";

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
  process_id: number;
}

interface ServerMessage {
  windows?: WindowInfo[];
  type?: string;
  pid?: number;
  success?: boolean;
  tree?: UITreeNode;
  error?: string;
  overlay_pid?: number;
  message?: string;
}

interface UITreeNode {
  role: string;
  title?: string;
  value?: string;
  enabled: boolean;
  children: UITreeNode[];
  depth: number;
  // Additional attributes for richer information
  description?: string;
  help?: string;
  placeholder?: string;
  role_description?: string;
  subrole?: string;
  focused?: boolean;
  selected?: boolean;
  selected_text?: string;
  character_count?: number;
  element_id?: string;
}

class AXTreeOverlay {
  private windowContainer: HTMLElement;
  private wsClient: InterlayClient;
  private currentWindows: WindowInfo[] = [];
  private focusedWindow: WindowInfo | null = null;
  private treeContainer: HTMLElement | null = null;
  private overlayProcessId: number | null = null;
  private lastNonOverlayWindow: WindowInfo | null = null;
  private expandedNodes: Set<string> = new Set(); // Track which nodes are collapsed (prefixed with 'collapsed:')
  private refreshTimer: number | null = null;
  private readonly REFRESH_INTERVAL = 2000; // 2 seconds

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.wsClient = new InterlayClient();
    this.setupWebSocketListener();

    // Clean up timer when page unloads
    window.addEventListener("beforeunload", () => {
      this.stopRefreshTimer();
    });
  }

  private async setupWebSocketListener() {
    try {
      // Set up message handler for window updates and accessibility responses
      this.wsClient.onMessage = (data) => {
        if (data.windows) {
          this.updateWindows(data.windows);
        } else if (data.type === "accessibility_tree_response") {
          this.handleAccessibilityTreeResponse(data);
        } else if (data.type === "accessibility_write_response") {
          this.handleAccessibilityWriteResponse(data);
        } else if (data.type === "overlay_info" && data.overlay_pid) {
          this.overlayProcessId = data.overlay_pid;
          console.log(
            `🎯 Received overlay PID from server: ${this.overlayProcessId}`
          );
        } else {
          // Log any unexpected messages
          console.log(`📨 Other message:`, data);
        }
      };

      // Connect to websocket
      await this.wsClient.connect();

      console.log("📡 WebSocket accessibility tree listener established");
    } catch (error) {
      console.error("❌ Failed to setup websocket listener:", error);
    }
  }

  private updateWindows(windows: WindowInfo[]) {
    this.currentWindows = windows;
    const newFocusedWindow = windows.find((w) => w.focused);

    // Overlay is filtered out of windows list by backend, so this is expected
    // (Removed spam logging since we know overlay won't be in the list)

    // Check if the newly focused window is the overlay itself
    // Since the overlay is filtered out of the windows list, we detect overlay focus
    // when no window is marked as focused (overlay focus = no focused window in list)
    let isOverlayFocused = false;

    if (!newFocusedWindow && this.overlayProcessId) {
      // No focused window found - this likely means overlay is focused
      isOverlayFocused = true;
      console.log(`🖱️ No focused window found - assuming overlay is focused`);
    } else if (newFocusedWindow && this.overlayProcessId) {
      isOverlayFocused = newFocusedWindow.process_id === this.overlayProcessId;

      // Only log on actual focus changes
      if (
        !this.focusedWindow ||
        this.focusedWindow.id !== newFocusedWindow.id
      ) {
        console.log(
          `🔍 Focus change: "${newFocusedWindow.name || "(empty)"}" (PID: ${
            newFocusedWindow.process_id
          }) - Is overlay: ${isOverlayFocused} (overlay PID: ${
            this.overlayProcessId
          })`
        );
      }
    } else if (newFocusedWindow && !this.overlayProcessId) {
      console.log(
        `🔍 Focus change but no overlay PID yet: "${
          newFocusedWindow.name || "(empty)"
        }" (PID: ${newFocusedWindow.process_id})`
      );
    }

    if (isOverlayFocused) {
      // Overlay is focused - keep the existing tree visible for interaction
      console.log("🖱️ Overlay focused - preserving existing tree");

      // Stop refresh timer while overlay is focused (to avoid interfering with interaction)
      this.stopRefreshTimer();

      if (this.treeContainer && this.lastNonOverlayWindow) {
        this.updateTreePosition();
        console.log("✅ Tree ready for interaction");
      } else if (!this.treeContainer) {
        console.log("ℹ️ No tree to interact with");
      }
      return; // Early return to prevent tree changes
    } else {
      // Resume refresh timer when overlay is not focused
      if (this.focusedWindow) {
        this.startRefreshTimer();
      }
    }

    // Store non-overlay focused windows for later reference
    if (newFocusedWindow && !isOverlayFocused) {
      // Only update if it's actually a different window
      if (
        !this.lastNonOverlayWindow ||
        this.lastNonOverlayWindow.id !== newFocusedWindow.id
      ) {
        this.lastNonOverlayWindow = newFocusedWindow;
        console.log(
          `📌 Stored non-overlay window: "${
            newFocusedWindow.name || "(empty)"
          }"`
        );
      } else {
        this.lastNonOverlayWindow = newFocusedWindow; // Update position but don't log
      }
    }

    // If focused window changed, request new accessibility tree
    if (
      newFocusedWindow &&
      (!this.focusedWindow || newFocusedWindow.id !== this.focusedWindow.id)
    ) {
      this.focusedWindow = newFocusedWindow;
      this.requestAccessibilityTree(newFocusedWindow.process_id);
      // Timer will be started when tree is displayed
    } else if (!newFocusedWindow) {
      // No focused window, clear the tree
      this.focusedWindow = null;
      this.clearAccessibilityTree(); // This will stop the timer
    } else if (
      newFocusedWindow &&
      this.focusedWindow &&
      newFocusedWindow.id === this.focusedWindow.id
    ) {
      // Same focused window but potentially moved - update position
      this.focusedWindow = newFocusedWindow;
      // Update lastNonOverlayWindow if this is not an overlay
      if (!isOverlayFocused) {
        this.lastNonOverlayWindow = newFocusedWindow;
      }
      this.updateTreePosition();
      // Keep timer running for same window
    }
  }

  private requestAccessibilityTree(pid: number) {
    if (
      this.wsClient.send({
        type: "get_accessibility_tree",
        pid: pid,
      })
    ) {
      console.log(`🌳 Requesting accessibility tree for PID: ${pid}`);
    } else {
      console.warn("❌ Failed to send accessibility tree request");
    }
  }

  private handleAccessibilityTreeResponse(data: ServerMessage) {
    if (data.success && data.tree && this.focusedWindow) {
      // Check if this is a refresh (tree already exists) or a new tree
      const isRefresh = this.treeContainer !== null;

      if (isRefresh) {
        // For refresh, preserve expanded state and update content
        this.refreshTreeContent(data.tree, this.focusedWindow);
      } else {
        // For new tree, display normally
        this.displayAccessibilityTree(data.tree, this.focusedWindow);
      }
    } else if (data.error) {
      console.warn(
        `❌ Accessibility tree error for PID ${data.pid}: ${data.error}`
      );
      this.showError(data.error);
    }
  }

  private handleAccessibilityWriteResponse(data: ServerMessage) {
    if (data.success) {
      console.log(`✅ Write successful: ${data.message}`);
      // Don't auto-refresh tree to avoid losing focus on live text inputs
      // The user can manually refresh if needed
    } else {
      console.error(`❌ Write failed: ${data.error}`);
    }
  }

  private showWriteFeedback(success: boolean, message: string) {
    // Create or update feedback element
    let feedbackEl = document.getElementById("write-feedback");
    if (!feedbackEl) {
      feedbackEl = document.createElement("div");
      feedbackEl.id = "write-feedback";
      feedbackEl.style.cssText = `
        position: fixed;
        top: 20px;
        right: 20px;
        padding: 10px 15px;
        border-radius: 5px;
        color: white;
        font-size: 12px;
        z-index: 10000;
        max-width: 300px;
        word-wrap: break-word;
      `;
      document.body.appendChild(feedbackEl);
    }

    // Set style and content based on success
    feedbackEl.style.backgroundColor = success ? "#4caf50" : "#f44336";
    feedbackEl.textContent = message;
    feedbackEl.style.display = "block";

    // Hide after 3 seconds
    setTimeout(() => {
      if (feedbackEl) {
        feedbackEl.style.display = "none";
      }
    }, 3000);
  }

  private writeToElement(elementPath: string, text: string) {
    // Use the last non-overlay window since overlay steals focus when clicked
    const targetWindow = this.lastNonOverlayWindow || this.focusedWindow;

    if (!targetWindow) {
      console.error("No target window to write to");
      return;
    }

    // Parse element path
    const pathArray = elementPath
      .split("-")
      .map((s) => parseInt(s, 10))
      .filter((n) => !isNaN(n));

    // Send write request using target window PID
    const writeRequest = {
      type: "write_to_element",
      pid: targetWindow.process_id,
      element_path: pathArray,
      text: text,
    };

    this.wsClient.send(writeRequest);
  }

  private refreshTreeContent(tree: UITreeNode, window: WindowInfo) {
    if (!this.treeContainer) return;

    // Find the content wrapper and update it
    const contentWrapper = this.treeContainer.querySelector(".tree-content");
    if (contentWrapper) {
      // Clear existing content but preserve container
      contentWrapper.innerHTML = "";

      // Create new tree content with preserved expanded state
      const treeContent = this.createTreeElement(tree);
      contentWrapper.appendChild(treeContent);

      // Update position in case window moved
      this.updateTreePosition();

      console.log(`🔄 Refreshed accessibility tree content for ${window.name}`);
    }
  }

  private displayAccessibilityTree(tree: UITreeNode, window: WindowInfo) {
    // Clear existing tree and reset state
    this.clearAccessibilityTree();
    this.expandedNodes.clear(); // Reset expansion state for new tree

    // Create tree container positioned to the right of the focused window
    this.treeContainer = document.createElement("div");
    this.treeContainer.className = "accessibility-tree";

    // Position to the right of the window
    const rightX = window.x + window.w + 10; // 10px margin
    this.treeContainer.style.left = `${rightX}px`;
    this.treeContainer.style.top = `${window.y}px`;
    // Set height to create a fixed-size container
    this.treeContainer.style.height = `${Math.min(window.h - 20, 800)}px`;

    // Create scrollable content wrapper
    const contentWrapper = document.createElement("div");
    contentWrapper.className = "tree-content";

    // Create tree content and add it to the wrapper
    const treeContent = this.createTreeElement(tree);
    contentWrapper.appendChild(treeContent);

    // Add wrapper to container
    this.treeContainer.appendChild(contentWrapper);

    this.windowContainer.appendChild(this.treeContainer);
    console.log(`✅ Displayed accessibility tree for ${window.name}`);

    // Start auto-refresh timer
    this.startRefreshTimer();
  }

  private createTreeElement(node: UITreeNode, nodeId?: string): HTMLElement {
    // Generate unique ID for this node
    if (!nodeId) {
      nodeId = `${node.role}-${node.depth}-${Math.random()
        .toString(36)
        .substr(2, 9)}`;
    }

    const nodeElement = document.createElement("div");
    nodeElement.className = "tree-node";
    // Reduced indentation - smaller increments, lower max
    const indent = Math.min(node.depth * 4, 16); // Max 16px indent (was 24px)
    nodeElement.style.marginLeft = `${indent}px`;

    // Create node content
    const nodeContent = document.createElement("div");
    nodeContent.className = "tree-node-content";

    // Expand/collapse indicator
    const expander = document.createElement("span");
    expander.className = "tree-expander";
    const hasChildren = node.children && node.children.length > 0;

    if (hasChildren) {
      expander.classList.add("expandable");
      // Expand by default, only collapse if manually set
      const isExpanded = !this.expandedNodes.has(`collapsed:${nodeId}`);
      expander.textContent = isExpanded ? "−" : "+";

      // Click handler for expand/collapse
      expander.addEventListener("click", (e) => {
        e.stopPropagation();
        this.toggleNodeExpansion(nodeId!, nodeElement, expander);
      });
    } else {
      expander.textContent = "•"; // Bullet for leaf nodes
    }

    nodeContent.appendChild(expander);

    // Node info container
    const nodeInfo = document.createElement("span");
    nodeInfo.style.flex = "1";

    // Role (always present)
    const roleSpan = document.createElement("span");
    roleSpan.className = "tree-role";
    roleSpan.textContent = node.role;
    nodeInfo.appendChild(roleSpan);

    // Subrole (if present and different from role)
    if (node.subrole && node.subrole !== node.role) {
      const subroleSpan = document.createElement("span");
      subroleSpan.className = "tree-subrole";
      subroleSpan.textContent = `:${node.subrole}`;
      nodeInfo.appendChild(subroleSpan);
    }

    // Title (if present)
    if (node.title) {
      const titleSpan = document.createElement("span");
      titleSpan.className = "tree-title";
      titleSpan.textContent = ` "${node.title}"`;
      nodeInfo.appendChild(titleSpan);
    }

    // Value (if present) - enhanced for text fields
    if (node.value) {
      const valueSpan = document.createElement("span");
      valueSpan.className = "tree-value";
      // For text fields, show more detailed value information
      if (node.role === "AXTextField" || node.role === "AXTextArea") {
        let valueText = ` = "${node.value}"`;
        if (node.character_count !== undefined) {
          valueText += ` (${node.character_count} chars)`;
        }
        valueSpan.textContent = valueText;
      } else {
        valueSpan.textContent = ` = ${node.value}`;
      }
      nodeInfo.appendChild(valueSpan);
    }

    // Placeholder (if present)
    if (node.placeholder) {
      const placeholderSpan = document.createElement("span");
      placeholderSpan.className = "tree-placeholder";
      placeholderSpan.textContent = ` placeholder:"${node.placeholder}"`;
      nodeInfo.appendChild(placeholderSpan);
    }

    // Selected text (if present)
    if (node.selected_text) {
      const selectedSpan = document.createElement("span");
      selectedSpan.className = "tree-selected";
      selectedSpan.textContent = ` selected:"${node.selected_text}"`;
      nodeInfo.appendChild(selectedSpan);
    }

    // Description (if present)
    if (node.description) {
      const descSpan = document.createElement("span");
      descSpan.className = "tree-description";
      descSpan.textContent = ` desc:"${node.description}"`;
      nodeInfo.appendChild(descSpan);
    }

    // State indicators
    const stateIndicators = [];
    if (node.focused) stateIndicators.push("focused");
    if (node.selected) stateIndicators.push("selected");
    if (!node.enabled) stateIndicators.push("disabled");

    if (stateIndicators.length > 0) {
      const stateSpan = document.createElement("span");
      stateSpan.className = "tree-state";
      stateSpan.textContent = ` [${stateIndicators.join(", ")}]`;
      nodeInfo.appendChild(stateSpan);
    }

    // Children count for nodes with children
    if (hasChildren) {
      const childCountSpan = document.createElement("span");
      childCountSpan.className = "tree-count";
      childCountSpan.textContent = ` (${node.children.length})`;
      nodeInfo.appendChild(childCountSpan);
    }

    nodeContent.appendChild(nodeInfo);

    // Add live text input for writable elements
    if (
      node.element_id &&
      (node.role === "AXTextField" ||
        node.role === "AXTextArea" ||
        node.role === "AXComboBox" ||
        node.role === "AXSecureTextField")
    ) {
      const textInput = document.createElement("input");
      textInput.type = "text";
      textInput.className = "tree-text-input";
      textInput.value = node.value || "";
      textInput.placeholder = "Type to update...";
      textInput.title = `Live edit ${node.role}`;
      textInput.style.cssText = `
        margin-left: 8px;
        padding: 2px 6px;
        font-size: 10px;
        background: #2a2a2a;
        color: white;
        border: 1px solid #4fc3f7;
        border-radius: 3px;
        width: 120px;
        outline: none;
      `;

      // Update on every keystroke
      textInput.addEventListener("input", (e) => {
        e.stopPropagation();
        const inputValue = (e.target as HTMLInputElement).value;
        this.writeToElement(node.element_id!, inputValue);
      });

      // Prevent clicks from affecting tree expansion
      textInput.addEventListener("click", (e) => {
        e.stopPropagation();
      });

      // Styling on focus
      textInput.addEventListener("focus", () => {
        textInput.style.borderColor = "#81d4fa";
        textInput.style.background = "#1a1a1a";
      });

      textInput.addEventListener("blur", () => {
        textInput.style.borderColor = "#4fc3f7";
        textInput.style.background = "#2a2a2a";
      });

      nodeContent.appendChild(textInput);
    }

    nodeElement.appendChild(nodeContent);

    // Add children container
    if (hasChildren) {
      const childrenContainer = document.createElement("div");
      childrenContainer.className = "tree-children";

      // Expand by default, only collapse if manually set
      const isExpanded = !this.expandedNodes.has(`collapsed:${nodeId}`);
      if (!isExpanded) {
        childrenContainer.classList.add("collapsed");
      }

      for (const child of node.children) {
        childrenContainer.appendChild(this.createTreeElement(child));
      }

      nodeElement.appendChild(childrenContainer);
    }

    return nodeElement;
  }

  private toggleNodeExpansion(
    nodeId: string,
    nodeElement: HTMLElement,
    expander: HTMLElement
  ) {
    const childrenContainer = nodeElement.querySelector(
      ".tree-children"
    ) as HTMLElement;
    if (!childrenContainer) return;

    const isCurrentlyExpanded =
      !childrenContainer.classList.contains("collapsed");

    if (isCurrentlyExpanded) {
      // Collapse
      childrenContainer.classList.add("collapsed");
      expander.textContent = "+";
      this.expandedNodes.add(`collapsed:${nodeId}`);
    } else {
      // Expand
      childrenContainer.classList.remove("collapsed");
      expander.textContent = "−";
      this.expandedNodes.delete(`collapsed:${nodeId}`);
    }
  }

  private updateTreePosition() {
    if (this.treeContainer) {
      // Use the last non-overlay window if available, otherwise fall back to focused window
      const referenceWindow = this.lastNonOverlayWindow || this.focusedWindow;
      if (referenceWindow) {
        const rightX = referenceWindow.x + referenceWindow.w + 10;
        this.treeContainer.style.left = `${rightX}px`;
        this.treeContainer.style.top = `${referenceWindow.y}px`;
        // Set height to maintain fixed-size container
        this.treeContainer.style.height = `${Math.min(
          referenceWindow.h - 20,
          800
        )}px`;
      } else {
        console.warn("⚠️ No reference window available for positioning tree");
      }
    }
  }

  private clearAccessibilityTree() {
    if (this.treeContainer) {
      this.treeContainer.remove();
      this.treeContainer = null;
    }
    // Stop refresh timer when clearing tree
    this.stopRefreshTimer();
  }

  private startRefreshTimer() {
    // Clear any existing timer first
    this.stopRefreshTimer();

    this.refreshTimer = window.setInterval(() => {
      // Only refresh if we have a focused window and tree is visible
      if (this.focusedWindow && this.treeContainer) {
        console.log(
          `🔄 Auto-refreshing accessibility tree for ${this.focusedWindow.name}`
        );
        this.requestAccessibilityTree(this.focusedWindow.process_id);
      }
    }, this.REFRESH_INTERVAL);

    console.log(`⏰ Started auto-refresh timer (${this.REFRESH_INTERVAL}ms)`);
  }

  private stopRefreshTimer() {
    if (this.refreshTimer !== null) {
      window.clearInterval(this.refreshTimer);
      this.refreshTimer = null;
      console.log("⏰ Stopped auto-refresh timer");
    }
  }

  private showError(error: string) {
    this.clearAccessibilityTree();

    if (this.focusedWindow) {
      const errorContainer = document.createElement("div");
      errorContainer.className = "accessibility-error";

      const rightX = this.focusedWindow.x + this.focusedWindow.w + 10;
      errorContainer.style.left = `${rightX}px`;
      errorContainer.style.top = `${this.focusedWindow.y}px`;

      errorContainer.textContent = `Error: ${error}`;
      this.windowContainer.appendChild(errorContainer);
      this.treeContainer = errorContainer;
    }
  }
}

// Initialize the overlay when DOM is loaded
document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
