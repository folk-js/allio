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
  // Position and size for UI element positioning
  position?: [number, number]; // [x, y] screen coordinates
  size?: [number, number]; // [width, height] dimensions
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
  private regexPanel: HTMLElement | null = null;
  private currentTargetElement: {
    elementId: string;
    currentValue: string;
  } | null = null;
  private currentTreeData: UITreeNode | null = null; // Store the current tree data for position lookups

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.wsClient = new InterlayClient();
    this.setupWebSocketListener();

    // Clean up timer when page unloads
    window.addEventListener("beforeunload", () => {
      this.stopRefreshTimer();
    });
  }

  /**
   * Parse CFString and CFNumber debug output and extract the actual content
   * Example: '<CFString 0x155fff590 [0x20c4c0998]>{contents = "hello world"}' -> 'hello world'
   * Example: '<CFNumber 0x9c12b19d15f2a744 [0x20c4c0998]>{value = +1, type = kCFNumberSInt32Type}' -> '1'
   */
  private parseCFStringValue(value: string): string {
    if (!value) return value;

    // Check if this is a CFString debug representation
    const cfStringMatch = value.match(/\{contents = "(.*?)"\}/);
    if (cfStringMatch && cfStringMatch[1] !== undefined) {
      return cfStringMatch[1];
    }

    // Check if this is a CFNumber debug representation
    const cfNumberMatch = value.match(/\{value = \+?(-?\d+(?:\.\d+)?)/);
    if (cfNumberMatch && cfNumberMatch[1] !== undefined) {
      return cfNumberMatch[1];
    }

    // Handle other potential CFString formats
    const simpleCFMatch = value.match(/CFString.*?"(.*?)"/);
    if (simpleCFMatch && simpleCFMatch[1] !== undefined) {
      return simpleCFMatch[1];
    }

    // If it's not a CF format, return as-is
    return value;
  }

  /**
   * Format values with special handling for different UI element types
   */
  private formatValueForRole(value: string, role: string): string {
    const cleanValue = this.parseCFStringValue(value);

    // Special formatting for radio buttons and checkboxes
    if (role === "AXRadioButton" || role.includes("RadioButton")) {
      if (cleanValue === "1") {
        return "selected";
      } else if (cleanValue === "0") {
        return "unselected";
      }
    }

    if (role === "AXCheckBox" || role.includes("CheckBox")) {
      if (cleanValue === "1") {
        return "checked";
      } else if (cleanValue === "0") {
        return "unchecked";
      }
    }

    // For sliders and progress indicators, keep numeric values
    if (role === "AXSlider" || role === "AXProgressIndicator") {
      return cleanValue;
    }

    return cleanValue;
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

    // Store the updated tree data
    this.currentTreeData = tree;

    // Find the content wrapper
    const contentWrapper = this.treeContainer.querySelector(".tree-content");
    if (contentWrapper) {
      // Clear existing content
      contentWrapper.innerHTML = "";

      // Create new tree content with preserved expansion state
      const treeContent = this.createTreeElement(tree);
      contentWrapper.appendChild(treeContent);

      console.log(`🔄 Refreshed tree content for ${window.name}`);
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
    // Set height to match window height exactly
    this.treeContainer.style.height = `${Math.min(window.h, 800)}px`;

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

    // Store the current tree data for position lookups
    this.currentTreeData = tree;

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

      // Parse and format value based on role
      const formattedValue = this.formatValueForRole(node.value, node.role);

      // For text fields, show more detailed value information
      if (node.role === "AXTextField" || node.role === "AXTextArea") {
        let valueText = ` = "${formattedValue}"`;
        if (node.character_count !== undefined) {
          valueText += ` (${node.character_count} chars)`;
        }
        valueSpan.textContent = valueText;
      } else {
        valueSpan.textContent = ` = ${formattedValue}`;
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
      // Create container for input elements
      const inputContainer = document.createElement("div");
      inputContainer.className = "tree-input-container";

      const textInput = document.createElement("input");
      textInput.type = "text";
      textInput.className = "tree-text-input";

      // Parse CFString format for the input value
      const cleanValue = this.parseCFStringValue(node.value || "");
      textInput.value = cleanValue;
      textInput.placeholder = "Type to update...";
      textInput.title = `Live edit ${node.role}`;

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

      // Add regex find & replace button
      const regexButton = document.createElement("button");
      regexButton.className = "tree-regex-button";
      regexButton.textContent = ".*";
      regexButton.title = "Open regex find & replace";

      regexButton.addEventListener("click", (e) => {
        e.stopPropagation();

        // Use raw parsed value for regex operations (not role-formatted)
        this.openRegexPanel(
          node.element_id!,
          cleanValue,
          node.position,
          node.size
        );
      });

      // Add elements to container
      inputContainer.appendChild(textInput);
      inputContainer.appendChild(regexButton);

      // Prevent container clicks from affecting tree expansion
      inputContainer.addEventListener("click", (e) => {
        e.stopPropagation();
      });

      nodeContent.appendChild(inputContainer);
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
        // Set height to match window height exactly
        this.treeContainer.style.height = `${Math.min(
          referenceWindow.h,
          800
        )}px`;
      } else {
        console.warn("⚠️ No reference window available for positioning tree");
      }
    }

    // Also update regex panel position if it's open
    this.updateRegexPanelPosition();
  }

  private updateRegexPanelPosition() {
    if (
      this.regexPanel &&
      this.regexPanel.classList.contains("positioned-relative")
    ) {
      // Use the last non-overlay window if available, otherwise fall back to focused window
      const referenceWindow = this.lastNonOverlayWindow || this.focusedWindow;
      if (referenceWindow) {
        // Get the target element's position and size from when the panel was opened
        if (this.currentTargetElement) {
          // Find the element in the current tree to get updated position
          const elementPath = this.currentTargetElement.elementId
            .split("-")
            .map((s) => parseInt(s, 10));

          const elementPosition = this.getElementPositionFromTree(elementPath);

          if (elementPosition) {
            const [x, y] = elementPosition.position;
            const [width, height] = elementPosition.size;

            // Calculate new position (consistent with openRegexPanel)
            const panelX = Math.max(10, x + width / 2 - 140); // Center panel (280px wide / 2 = 140px offset)
            const panelY = y + height + 6; // 6px below the element

            // Ensure panel doesn't go off-screen
            const maxX = window.screen.width - 300; // Panel width + margin
            const maxY = window.screen.height - 180; // Estimated panel height + margin

            const finalX = Math.min(panelX, maxX);
            const finalY = Math.min(panelY, maxY);

            this.regexPanel.style.left = `${finalX}px`;
            this.regexPanel.style.top = `${finalY}px`;
          }
        }
      }
    }
  }

  private getElementPositionFromTree(
    elementPath: number[]
  ): { position: [number, number]; size: [number, number] } | null {
    if (!this.currentTreeData) {
      return null;
    }

    // Traverse the tree following the element path
    let currentNode = this.currentTreeData;

    for (let i = 0; i < elementPath.length; i++) {
      const pathIndex = elementPath[i];

      if (!currentNode.children || pathIndex >= currentNode.children.length) {
        return null;
      }
      currentNode = currentNode.children[pathIndex];
    }

    // Return position and size if available
    if (currentNode.position && currentNode.size) {
      return {
        position: [currentNode.position[0], currentNode.position[1]],
        size: [currentNode.size[0], currentNode.size[1]],
      };
    }

    return null;
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

  private openRegexPanel(
    elementId: string,
    currentValue: string,
    elementPosition?: [number, number],
    elementSize?: [number, number]
  ) {
    // Close existing panel if open
    this.closeRegexPanel();

    // Parse CFString format for the current value
    const cleanCurrentValue = this.parseCFStringValue(currentValue);

    this.currentTargetElement = { elementId, currentValue: cleanCurrentValue };

    // Create regex panel
    this.regexPanel = document.createElement("div");
    this.regexPanel.className = "regex-panel";

    // Calculate position - prefer positioning below the element if position is available
    let panelStyle = `
      position: fixed;
      background: rgba(30, 30, 30, 0.98);
      color: white;
      padding: 12px;
      border-radius: 8px;
      border: 1px solid rgba(255, 255, 255, 0.1);
      min-width: 280px;
      max-width: 400px;
      z-index: 2000;
      backdrop-filter: blur(20px);
      box-shadow: 0 6px 24px rgba(0, 0, 0, 0.6);
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", sans-serif;
      font-size: 13px;
      pointer-events: auto;
    `;

    if (elementPosition && elementSize) {
      // Position the panel below the element, centered horizontally
      const [x, y] = elementPosition;
      const [width, height] = elementSize;

      // Calculate position below the element
      const panelX = Math.max(10, x + width / 2 - 140); // Center panel (280px wide / 2 = 140px offset)
      const panelY = y + height + 6; // 6px below the element

      // Ensure panel doesn't go off-screen
      const maxX = window.screen.width - 300; // Panel width + margin
      const maxY = window.screen.height - 180; // Estimated panel height + margin

      const finalX = Math.min(panelX, maxX);
      const finalY = Math.min(panelY, maxY);

      panelStyle += `
        left: ${finalX}px;
        top: ${finalY}px;
        transform: none;
      `;

      // Add class for element-relative positioning animation
      this.regexPanel.classList.add("positioned-relative");

      console.log(
        `🎯 Positioning regex panel at (${finalX}, ${finalY}) below element at (${x}, ${y})`
      );
    } else {
      // Fallback to center positioning
      panelStyle += `
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
      `;

      console.log(`📍 No element position available, centering regex panel`);
    }

    this.regexPanel.style.cssText = panelStyle;

    // Panel header
    const header = document.createElement("div");
    header.style.cssText = `
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 10px;
      padding-bottom: 6px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    `;

    const title = document.createElement("h3");
    title.textContent = "Find & Replace";
    title.style.cssText = `
      margin: 0;
      color: #ffffff;
      font-size: 13px;
      font-weight: 600;
    `;

    const closeButton = document.createElement("button");
    closeButton.textContent = "✕";
    closeButton.style.cssText = `
      background: none;
      border: none;
      color: #999;
      font-size: 14px;
      cursor: pointer;
      padding: 2px 6px;
      border-radius: 4px;
      transition: color 0.2s;
    `;
    closeButton.addEventListener("mouseover", () => {
      closeButton.style.color = "#fff";
    });
    closeButton.addEventListener("mouseout", () => {
      closeButton.style.color = "#999";
    });
    closeButton.addEventListener("click", () => this.closeRegexPanel());

    header.appendChild(title);
    header.appendChild(closeButton);

    // Pattern input
    const patternLabel = document.createElement("label");
    patternLabel.textContent = "Pattern:";
    patternLabel.style.cssText = `
      display: block;
      margin-bottom: 4px;
      color: #ccc;
      font-size: 12px;
    `;

    const patternInput = document.createElement("input");
    patternInput.type = "text";
    patternInput.placeholder =
      "/find/gi or just: find or \\b(\\w) for word starts";
    patternInput.style.cssText = `
      width: 100%;
      padding: 6px 8px;
      margin-bottom: 8px;
      background: rgba(255, 255, 255, 0.1);
      border: 1px solid rgba(255, 255, 255, 0.2);
      border-radius: 4px;
      color: white;
      font-size: 12px;
      font-family: "SF Mono", Monaco, monospace;
      box-sizing: border-box;
    `;

    // Replace input
    const replaceLabel = document.createElement("label");
    replaceLabel.textContent = "Replace:";
    replaceLabel.style.cssText = `
      display: block;
      margin-bottom: 4px;
      color: #ccc;
      font-size: 12px;
    `;

    const replaceInput = document.createElement("input");
    replaceInput.type = "text";
    replaceInput.placeholder = "replacement or $1:upper, $1:lower, $1:title";
    replaceInput.style.cssText = `
      width: 100%;
      padding: 6px 8px;
      margin-bottom: 8px;
      background: rgba(255, 255, 255, 0.1);
      border: 1px solid rgba(255, 255, 255, 0.2);
      border-radius: 4px;
      color: white;
      font-size: 12px;
      font-family: "SF Mono", Monaco, monospace;
      box-sizing: border-box;
    `;

    // Preview
    const previewLabel = document.createElement("label");
    previewLabel.textContent = "Preview:";
    previewLabel.style.cssText = `
      display: block;
      margin-bottom: 4px;
      color: #ccc;
      font-size: 12px;
    `;

    const previewDiv = document.createElement("div");
    previewDiv.style.cssText = `
      padding: 6px 8px;
      margin-bottom: 10px;
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid rgba(255, 255, 255, 0.1);
      border-radius: 4px;
      color: #ddd;
      font-size: 12px;
      font-family: "SF Mono", Monaco, monospace;
      min-height: 20px;
      max-height: 60px;
      overflow-y: auto;
      word-break: break-all;
    `;
    previewDiv.textContent = cleanCurrentValue || "(empty)";

    // Buttons
    const buttonContainer = document.createElement("div");
    buttonContainer.style.cssText = `
      display: flex;
      gap: 8px;
      justify-content: flex-end;
    `;

    const applyButton = document.createElement("button");
    applyButton.textContent = "Apply";
    applyButton.style.cssText = `
      padding: 6px 16px;
      background: #007aff;
      border: none;
      border-radius: 4px;
      color: white;
      font-size: 12px;
      font-weight: 500;
      cursor: pointer;
      transition: background 0.2s;
    `;
    applyButton.addEventListener("mouseover", () => {
      applyButton.style.background = "#0056b3";
    });
    applyButton.addEventListener("mouseout", () => {
      applyButton.style.background = "#007aff";
    });

    const cancelButton = document.createElement("button");
    cancelButton.textContent = "Cancel";
    cancelButton.style.cssText = `
      padding: 6px 16px;
      background: rgba(255, 255, 255, 0.1);
      border: 1px solid rgba(255, 255, 255, 0.2);
      border-radius: 4px;
      color: #ccc;
      font-size: 12px;
      font-weight: 500;
      cursor: pointer;
      transition: background 0.2s;
    `;
    cancelButton.addEventListener("mouseover", () => {
      cancelButton.style.background = "rgba(255, 255, 255, 0.2)";
    });
    cancelButton.addEventListener("mouseout", () => {
      cancelButton.style.background = "rgba(255, 255, 255, 0.1)";
    });
    cancelButton.addEventListener("click", () => this.closeRegexPanel());

    // Update preview function
    const updatePreview = () => {
      const pattern = patternInput.value.trim();
      const replacement = replaceInput.value;

      if (!pattern) {
        previewDiv.textContent = cleanCurrentValue || "(empty)";
        return;
      }

      try {
        // Parse regex pattern with flags (e.g., /pattern/gi)
        let regex: RegExp;
        const regexMatch = pattern.match(/^\/(.+)\/([gimuy]*)$/);

        if (regexMatch) {
          // Full regex format with flags
          regex = new RegExp(regexMatch[1], regexMatch[2]);
        } else {
          // Plain pattern, default to global
          regex = new RegExp(pattern, "g");
        }

        // Enhanced replacement with transformation support
        const result = this.applyRegexWithTransforms(
          this.currentTargetElement!.currentValue,
          regex,
          replacement
        );
        previewDiv.textContent = result || "(empty)";
      } catch (e) {
        previewDiv.textContent = "⚠️ Invalid regex pattern";
        previewDiv.style.color = "#ff6b6b";
        return;
      }

      previewDiv.style.color = "#ddd";
    };

    // Apply regex function
    const applyRegex = () => {
      const pattern = patternInput.value.trim();
      const replacement = replaceInput.value;

      if (!pattern) return;

      try {
        // Parse regex pattern with flags (e.g., /pattern/gi)
        let regex: RegExp;
        const regexMatch = pattern.match(/^\/(.+)\/([gimuy]*)$/);

        if (regexMatch) {
          // Full regex format with flags
          regex = new RegExp(regexMatch[1], regexMatch[2]);
        } else {
          // Plain pattern, default to global
          regex = new RegExp(pattern, "g");
        }

        // Enhanced replacement with transformation support
        const result = this.applyRegexWithTransforms(
          this.currentTargetElement!.currentValue,
          regex,
          replacement
        );

        // Apply the change
        this.writeToElement(this.currentTargetElement!.elementId, result);

        // Update stored current value
        this.currentTargetElement!.currentValue = result;

        // Close panel
        this.closeRegexPanel();
      } catch (e) {
        console.error("Regex application failed:", e);
      }
    };

    // Event listeners
    patternInput.addEventListener("input", updatePreview);
    replaceInput.addEventListener("input", updatePreview);
    applyButton.addEventListener("click", applyRegex);

    // Handle Enter key
    const handleEnter = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        applyRegex();
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.closeRegexPanel();
      }
    };
    patternInput.addEventListener("keydown", handleEnter);
    replaceInput.addEventListener("keydown", handleEnter);

    // Assemble panel
    buttonContainer.appendChild(cancelButton);
    buttonContainer.appendChild(applyButton);

    this.regexPanel.appendChild(header);
    this.regexPanel.appendChild(patternLabel);
    this.regexPanel.appendChild(patternInput);
    this.regexPanel.appendChild(replaceLabel);
    this.regexPanel.appendChild(replaceInput);
    this.regexPanel.appendChild(previewLabel);
    this.regexPanel.appendChild(previewDiv);
    this.regexPanel.appendChild(buttonContainer);

    document.body.appendChild(this.regexPanel);

    // Focus the pattern input
    patternInput.focus();
  }

  private closeRegexPanel() {
    if (this.regexPanel) {
      this.regexPanel.remove();
      this.regexPanel = null;
    }
    this.currentTargetElement = null;
  }

  private applyRegexWithTransforms(
    text: string,
    regex: RegExp,
    replacement: string
  ): string {
    // Check if replacement contains transformation syntax like $1:upper, $1:lower, $1:title
    const hasTransforms = /\$\d+:(upper|lower|title|capitalize)/i.test(
      replacement
    );

    if (!hasTransforms) {
      // Simple replacement without transforms
      return text.replace(regex, replacement);
    }

    // Use function-based replacement for transformations
    return text.replace(regex, (...args) => {
      const match = args[0]; // Full match
      const captures = args.slice(1, -2); // Captured groups (exclude offset and input string)

      let result = replacement;

      // Replace each transformation
      result = result.replace(
        /\$(\d+):(upper|lower|title|capitalize)/gi,
        (transformMatch, groupNum, transform) => {
          const groupIndex = parseInt(groupNum) - 1; // Convert to 0-based index

          if (groupIndex >= 0 && groupIndex < captures.length) {
            const capturedText = captures[groupIndex];

            switch (transform.toLowerCase()) {
              case "upper":
                return capturedText.toUpperCase();
              case "lower":
                return capturedText.toLowerCase();
              case "title":
              case "capitalize":
                return (
                  capturedText.charAt(0).toUpperCase() +
                  capturedText.slice(1).toLowerCase()
                );
              default:
                return capturedText;
            }
          }

          return transformMatch; // Return original if group not found
        }
      );

      // Replace standard group references like $1, $2, etc.
      result = result.replace(/\$(\d+)/g, (groupMatch, groupNum) => {
        const groupIndex = parseInt(groupNum) - 1;
        return groupIndex >= 0 && groupIndex < captures.length
          ? captures[groupIndex]
          : groupMatch;
      });

      return result;
    });
  }
}

// Initialize the overlay when DOM is loaded
document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
