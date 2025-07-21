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
}

interface UITreeNode {
  role: string;
  title?: string;
  value?: string;
  enabled: boolean;
  children: UITreeNode[];
  depth: number;
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

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.wsClient = new InterlayClient();
    this.setupWebSocketListener();
  }

  private async setupWebSocketListener() {
    try {
      // Set up message handler for window updates and accessibility responses
      this.wsClient.onMessage = (data) => {
        if (data.windows) {
          this.updateWindows(data.windows);
        } else if (data.type === "accessibility_tree_response") {
          this.handleAccessibilityTreeResponse(data);
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

      if (this.treeContainer && this.lastNonOverlayWindow) {
        this.updateTreePosition();
        console.log("✅ Tree ready for interaction");
      } else if (!this.treeContainer) {
        console.log("ℹ️ No tree to interact with");
      }
      return; // Early return to prevent tree changes
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
    } else if (!newFocusedWindow) {
      // No focused window, clear the tree
      this.focusedWindow = null;
      this.clearAccessibilityTree();
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
      this.displayAccessibilityTree(data.tree, this.focusedWindow);
    } else if (data.error) {
      console.warn(
        `❌ Accessibility tree error for PID ${data.pid}: ${data.error}`
      );
      this.showError(data.error);
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
    // Set max-height to allow scrolling when content exceeds container
    this.treeContainer.style.maxHeight = `${Math.min(window.h - 20, 800)}px`;

    // Create tree content
    const treeContent = this.createTreeElement(tree);
    this.treeContainer.appendChild(treeContent);

    this.windowContainer.appendChild(this.treeContainer);
    console.log(`✅ Displayed accessibility tree for ${window.name}`);
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

    // Title (if present)
    if (node.title) {
      const titleSpan = document.createElement("span");
      titleSpan.className = "tree-title";
      titleSpan.textContent = ` "${node.title}"`;
      nodeInfo.appendChild(titleSpan);
    }

    // Value (if present)
    if (node.value) {
      const valueSpan = document.createElement("span");
      valueSpan.className = "tree-value";
      valueSpan.textContent = ` = ${node.value}`;
      nodeInfo.appendChild(valueSpan);
    }

    // Enabled state
    if (!node.enabled) {
      const disabledSpan = document.createElement("span");
      disabledSpan.className = "tree-disabled";
      disabledSpan.textContent = " (disabled)";
      nodeInfo.appendChild(disabledSpan);
    }

    // Children count for nodes with children
    if (hasChildren) {
      const childCountSpan = document.createElement("span");
      childCountSpan.className = "tree-value";
      childCountSpan.textContent = ` [${node.children.length}]`;
      nodeInfo.appendChild(childCountSpan);
    }

    nodeContent.appendChild(nodeInfo);
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
        // Set max-height to allow scrolling when content exceeds container
        this.treeContainer.style.maxHeight = `${Math.min(
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
