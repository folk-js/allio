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
   * Parse CFString debug output and extract the actual content
   * Example: '<CFString 0x155fff590 [0x20c4c0998]>{contents = "hello world"}' -> 'hello world'
   */
  private parseCFStringValue(value: string): string {
    if (!value) return value;

    // Check if this is a CFString debug representation
    const cfStringMatch = value.match(/\{contents = "(.*?)"\}/);
    if (cfStringMatch && cfStringMatch[1] !== undefined) {
      return cfStringMatch[1];
    }

    // Handle other potential CFString formats
    const simpleCFMatch = value.match(/CFString.*?"(.*?)"/);
    if (simpleCFMatch && simpleCFMatch[1] !== undefined) {
      return simpleCFMatch[1];
    }

    // If it's not a CFString format, return as-is
    return value;
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

      // Parse CFString format and get clean value
      const cleanValue = this.parseCFStringValue(node.value);

      // For text fields, show more detailed value information
      if (node.role === "AXTextField" || node.role === "AXTextArea") {
        let valueText = ` = "${cleanValue}"`;
        if (node.character_count !== undefined) {
          valueText += ` (${node.character_count} chars)`;
        }
        valueSpan.textContent = valueText;
      } else {
        valueSpan.textContent = ` = ${cleanValue}`;
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

      // Parse CFString format for the input value
      const cleanValue = this.parseCFStringValue(node.value || "");
      textInput.value = cleanValue;
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

      // Add regex find & replace button
      const regexButton = document.createElement("button");
      regexButton.className = "tree-regex-button";
      regexButton.textContent = ".*";
      regexButton.title = "Open regex find & replace";
      regexButton.style.cssText = `
        margin-left: 4px;
        padding: 2px 6px;
        font-size: 10px;
        background: #ff9800;
        color: white;
        border: none;
        border-radius: 3px;
        cursor: pointer;
        font-family: monospace;
        outline: none;
        transition: background-color 0.2s;
      `;

      regexButton.addEventListener("click", (e) => {
        e.stopPropagation();
        // Use clean value for regex operations and pass element position/size
        this.openRegexPanel(
          node.element_id!,
          cleanValue,
          node.position,
          node.size
        );
      });

      regexButton.addEventListener("mouseenter", () => {
        regexButton.style.backgroundColor = "#ffb74d";
      });

      regexButton.addEventListener("mouseleave", () => {
        regexButton.style.backgroundColor = "#ff9800";
      });

      nodeContent.appendChild(regexButton);
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
      background: rgba(0, 0, 0, 0.95);
      color: white;
      padding: 20px;
      border-radius: 8px;
      border: 1px solid rgba(255, 255, 255, 0.3);
      min-width: 400px;
      max-width: 600px;
      z-index: 2000;
      backdrop-filter: blur(15px);
      box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
        sans-serif;
      pointer-events: auto;
    `;

    if (elementPosition && elementSize) {
      // Position the panel below the element, centered horizontally
      const [x, y] = elementPosition;
      const [width, height] = elementSize;

      // Calculate position below the element
      const panelX = Math.max(10, x + width / 2 - 200); // Center panel (400px wide / 2 = 200px offset)
      const panelY = y + height + 10; // 10px below the element

      // Ensure panel doesn't go off-screen
      const maxX = window.screen.width - 420; // Panel width + some margin
      const maxY = window.screen.height - 400; // Estimated panel height + margin

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
        `🎯 Positioning regex panel relative to element at (${x}, ${y}) size (${width}x${height}) → panel at (${finalX}, ${finalY})`
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
      margin-bottom: 15px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.2);
      padding-bottom: 10px;
    `;

    const title = document.createElement("h3");
    title.textContent = "Regex Find & Replace";
    title.style.cssText = `
      margin: 0;
      color: #4fc3f7;
      font-size: 16px;
    `;

    const closeButton = document.createElement("button");
    closeButton.textContent = "×";
    closeButton.style.cssText = `
      background: none;
      border: none;
      color: #fff;
      font-size: 20px;
      cursor: pointer;
      padding: 0;
      width: 24px;
      height: 24px;
      display: flex;
      align-items: center;
      justify-content: center;
    `;
    closeButton.addEventListener("click", () => this.closeRegexPanel());

    header.appendChild(title);
    header.appendChild(closeButton);
    this.regexPanel.appendChild(header);

    // Current value display
    const currentValueSection = document.createElement("div");
    currentValueSection.style.marginBottom = "15px";

    const currentValueLabel = document.createElement("label");
    currentValueLabel.textContent = "Current Value:";
    currentValueLabel.style.cssText = `
      display: block;
      margin-bottom: 5px;
      font-size: 12px;
      color: #aaa;
    `;

    const currentValueDisplay = document.createElement("div");
    // Show the clean value, not the CFString format
    currentValueDisplay.textContent = cleanCurrentValue || "(empty)";
    currentValueDisplay.style.cssText = `
      background: #1a1a1a;
      padding: 8px;
      border-radius: 4px;
      border: 1px solid #333;
      font-family: "Monaco", "Menlo", "Consolas", monospace;
      font-size: 11px;
      max-height: 60px;
      overflow-y: auto;
      word-break: break-all;
    `;

    // Add a small note if the original value was a CFString
    if (currentValue !== cleanCurrentValue) {
      const cfStringNote = document.createElement("div");
      cfStringNote.textContent = `(Parsed from CFString format)`;
      cfStringNote.style.cssText = `
        font-size: 10px;
        color: #666;
        font-style: italic;
        margin-top: 4px;
      `;
      currentValueSection.appendChild(currentValueLabel);
      currentValueSection.appendChild(currentValueDisplay);
      currentValueSection.appendChild(cfStringNote);
    } else {
      currentValueSection.appendChild(currentValueLabel);
      currentValueSection.appendChild(currentValueDisplay);
    }

    this.regexPanel.appendChild(currentValueSection);

    // Find input
    const findSection = document.createElement("div");
    findSection.style.marginBottom = "10px";

    const findLabel = document.createElement("label");
    findLabel.textContent = "Find (Regex Pattern):";
    findLabel.style.cssText = `
      display: block;
      margin-bottom: 5px;
      font-size: 12px;
      color: #aaa;
    `;

    const findInput = document.createElement("input");
    findInput.type = "text";
    findInput.placeholder = "Enter regex pattern...";
    findInput.style.cssText = `
      width: 100%;
      padding: 8px;
      background: #2a2a2a;
      color: white;
      border: 1px solid #4fc3f7;
      border-radius: 4px;
      font-family: "Monaco", "Menlo", "Consolas", monospace;
      font-size: 12px;
      outline: none;
      box-sizing: border-box;
    `;

    findSection.appendChild(findLabel);
    findSection.appendChild(findInput);
    this.regexPanel.appendChild(findSection);

    // Replace input
    const replaceSection = document.createElement("div");
    replaceSection.style.marginBottom = "10px";

    const replaceLabel = document.createElement("label");
    replaceLabel.textContent = "Replace With:";
    replaceLabel.style.cssText = `
      display: block;
      margin-bottom: 5px;
      font-size: 12px;
      color: #aaa;
    `;

    const replaceInput = document.createElement("input");
    replaceInput.type = "text";
    replaceInput.placeholder = "Enter replacement text...";
    replaceInput.style.cssText = `
      width: 100%;
      padding: 8px;
      background: #2a2a2a;
      color: white;
      border: 1px solid #4fc3f7;
      border-radius: 4px;
      font-family: "Monaco", "Menlo", "Consolas", monospace;
      font-size: 12px;
      outline: none;
      box-sizing: border-box;
    `;

    replaceSection.appendChild(replaceLabel);
    replaceSection.appendChild(replaceInput);
    this.regexPanel.appendChild(replaceSection);

    // Flags section
    const flagsSection = document.createElement("div");
    flagsSection.style.cssText = `
      margin-bottom: 15px;
      display: flex;
      gap: 15px;
      align-items: center;
    `;

    const flagsLabel = document.createElement("span");
    flagsLabel.textContent = "Flags:";
    flagsLabel.style.cssText = `
      font-size: 12px;
      color: #aaa;
    `;

    // Global flag
    const globalFlag = document.createElement("label");
    globalFlag.style.cssText = `
      display: flex;
      align-items: center;
      gap: 5px;
      font-size: 12px;
      cursor: pointer;
    `;
    const globalCheckbox = document.createElement("input");
    globalCheckbox.type = "checkbox";
    globalCheckbox.checked = true; // Default to global
    globalFlag.appendChild(globalCheckbox);
    globalFlag.appendChild(document.createTextNode("Global (g)"));

    // Case insensitive flag
    const caseFlag = document.createElement("label");
    caseFlag.style.cssText = `
      display: flex;
      align-items: center;
      gap: 5px;
      font-size: 12px;
      cursor: pointer;
    `;
    const caseCheckbox = document.createElement("input");
    caseCheckbox.type = "checkbox";
    caseFlag.appendChild(caseCheckbox);
    caseFlag.appendChild(document.createTextNode("Ignore Case (i)"));

    // Multiline flag
    const multilineFlag = document.createElement("label");
    multilineFlag.style.cssText = `
      display: flex;
      align-items: center;
      gap: 5px;
      font-size: 12px;
      cursor: pointer;
    `;
    const multilineCheckbox = document.createElement("input");
    multilineCheckbox.type = "checkbox";
    multilineFlag.appendChild(multilineCheckbox);
    multilineFlag.appendChild(document.createTextNode("Multiline (m)"));

    flagsSection.appendChild(flagsLabel);
    flagsSection.appendChild(globalFlag);
    flagsSection.appendChild(caseFlag);
    flagsSection.appendChild(multilineFlag);
    this.regexPanel.appendChild(flagsSection);

    // Preview section
    const previewSection = document.createElement("div");
    previewSection.style.marginBottom = "15px";

    const previewLabel = document.createElement("label");
    previewLabel.textContent = "Preview:";
    previewLabel.style.cssText = `
      display: block;
      margin-bottom: 5px;
      font-size: 12px;
      color: #aaa;
    `;

    const previewDisplay = document.createElement("div");
    previewDisplay.style.cssText = `
      background: #1a1a1a;
      padding: 8px;
      border-radius: 4px;
      border: 1px solid #333;
      font-family: "Monaco", "Menlo", "Consolas", monospace;
      font-size: 11px;
      max-height: 80px;
      overflow-y: auto;
      word-break: break-all;
      color: #a5d6a7;
    `;
    previewDisplay.textContent = "(Enter pattern to see preview)";

    previewSection.appendChild(previewLabel);
    previewSection.appendChild(previewDisplay);
    this.regexPanel.appendChild(previewSection);

    // Update preview on input changes
    const updatePreview = () => {
      const pattern = findInput.value;
      const replacement = replaceInput.value;

      if (!pattern) {
        previewDisplay.textContent = "(Enter pattern to see preview)";
        previewDisplay.style.color = "#aaa";
        return;
      }

      try {
        const flags =
          (globalCheckbox.checked ? "g" : "") +
          (caseCheckbox.checked ? "i" : "") +
          (multilineCheckbox.checked ? "m" : "");

        const regex = new RegExp(pattern, flags);
        // Use the clean current value for preview
        const result = cleanCurrentValue.replace(regex, replacement);

        if (result === cleanCurrentValue) {
          previewDisplay.textContent = "(No matches found)";
          previewDisplay.style.color = "#ffb74d";
        } else {
          previewDisplay.textContent = result;
          previewDisplay.style.color = "#a5d6a7";
        }
      } catch (error) {
        previewDisplay.textContent = `Error: ${(error as Error).message}`;
        previewDisplay.style.color = "#f44336";
      }
    };

    findInput.addEventListener("input", updatePreview);
    replaceInput.addEventListener("input", updatePreview);
    globalCheckbox.addEventListener("change", updatePreview);
    caseCheckbox.addEventListener("change", updatePreview);
    multilineCheckbox.addEventListener("change", updatePreview);

    // Action buttons
    const buttonSection = document.createElement("div");
    buttonSection.style.cssText = `
      display: flex;
      gap: 10px;
      justify-content: flex-end;
    `;

    const cancelButton = document.createElement("button");
    cancelButton.textContent = "Cancel";
    cancelButton.style.cssText = `
      padding: 8px 16px;
      background: #666;
      color: white;
      border: none;
      border-radius: 4px;
      cursor: pointer;
      font-size: 12px;
    `;
    cancelButton.addEventListener("click", () => this.closeRegexPanel());

    const applyButton = document.createElement("button");
    applyButton.textContent = "Apply Replace";
    applyButton.style.cssText = `
      padding: 8px 16px;
      background: #4caf50;
      color: white;
      border: none;
      border-radius: 4px;
      cursor: pointer;
      font-size: 12px;
    `;

    applyButton.addEventListener("click", () => {
      this.applyRegexReplace(
        findInput.value,
        replaceInput.value,
        globalCheckbox.checked,
        caseCheckbox.checked,
        multilineCheckbox.checked
      );
    });

    buttonSection.appendChild(cancelButton);
    buttonSection.appendChild(applyButton);
    this.regexPanel.appendChild(buttonSection);

    // Add to document
    document.body.appendChild(this.regexPanel);

    // Focus the find input
    findInput.focus();

    // Close on Escape key
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        this.closeRegexPanel();
        document.removeEventListener("keydown", handleKeyDown);
      }
    };
    document.addEventListener("keydown", handleKeyDown);
  }

  private applyRegexReplace(
    pattern: string,
    replacement: string,
    global: boolean,
    ignoreCase: boolean,
    multiline: boolean
  ) {
    if (!this.currentTargetElement || !pattern) {
      return;
    }

    try {
      const flags =
        (global ? "g" : "") + (ignoreCase ? "i" : "") + (multiline ? "m" : "");

      const regex = new RegExp(pattern, flags);
      const result = this.currentTargetElement.currentValue.replace(
        regex,
        replacement
      );

      // Apply the result
      this.writeToElement(this.currentTargetElement.elementId, result);

      // Show success feedback
      this.showWriteFeedback(
        true,
        `Regex replace applied: ${pattern} → ${replacement}`
      );

      // Close panel
      this.closeRegexPanel();
    } catch (error) {
      this.showWriteFeedback(false, `Regex error: ${(error as Error).message}`);
    }
  }

  private closeRegexPanel() {
    if (this.regexPanel) {
      this.regexPanel.remove();
      this.regexPanel = null;
    }
    this.currentTargetElement = null;
  }
}

// Initialize the overlay when DOM is loaded
document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
