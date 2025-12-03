import { AXIO, AXNode, ElementUpdate, Window } from "@axio/client";

class AXTreeOverlay {
  private windowContainer: HTMLElement;
  private axio: AXIO;
  private treeContainer: HTMLElement | null = null;
  private lastFocusedWindow: Window | null = null;
  private regexPanel: HTMLElement | null = null;
  private currentTargetElement: {
    node: AXNode;
    currentValue: string;
  } | null = null;
  private renderedNodeCount: number = 0; // Track rendered DOM elements
  private hoverOutline: HTMLElement | null = null; // Visual outline for hovered elements
  private expandedNodes: Set<string> = new Set(); // Track which nodes are expanded
  private loadingNodes: Set<string> = new Set(); // Track which nodes are currently loading
  private nodeElements: Map<string, { element: HTMLElement; node: AXNode }> =
    new Map(); // Track rendered nodes
  private isClickthroughEnabled: boolean = true; // Track current clickthrough state (start enabled)

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.axio = new AXIO();
    this.setupWebSocketListener();
    this.setupCursorTransparency();
  }

  /**
   * Setup automatic cursor transparency based on whether mouse is over interactive elements
   * Uses global mouse position from backend (works even when window is not focused)
   */
  private setupCursorTransparency() {
    // Listen for global mouse position from backend
    this.axio.onMousePosition((x, y) => {
      // Check what element is at this position
      const elementUnderCursor = document.elementFromPoint(x, y);

      // Check if cursor is over sidebar or regex panel
      const isOverSidebar =
        elementUnderCursor && this.isElementInSidebar(elementUnderCursor);

      // Enable clickthrough when NOT over sidebar (transparent to apps below)
      // Disable clickthrough when over sidebar (interactive)
      const shouldEnableClickthrough = !isOverSidebar;

      // Only update if state changed (avoid spamming backend)
      if (shouldEnableClickthrough !== this.isClickthroughEnabled) {
        this.isClickthroughEnabled = shouldEnableClickthrough;
        this.axio.setClickthrough(shouldEnableClickthrough).catch((err) => {
          console.error("Failed to set clickthrough:", err);
        });
      }
    });
  }

  /**
   * Check if element is inside sidebar or regex panel
   */
  private isElementInSidebar(element: Element): boolean {
    let current: Element | null = element;
    while (current) {
      // Check if it's the tree container or regex panel
      if (current === this.treeContainer || current === this.regexPanel) {
        return true;
      }
      current = current.parentElement;
    }
    return false;
  }

  /**
   * Extract position tuple from AXNode bounds
   */
  private getPosition(node: AXNode): [number, number] | undefined {
    return node.bounds
      ? [node.bounds.position.x, node.bounds.position.y]
      : undefined;
  }

  /**
   * Extract size tuple from AXNode bounds
   */
  private getSize(node: AXNode): [number, number] | undefined {
    return node.bounds
      ? [node.bounds.size.width, node.bounds.size.height]
      : undefined;
  }

  private async setupWebSocketListener() {
    try {
      // Set up window update handler
      this.axio.onWindowUpdate((windows) => {
        this.updateWindows(windows);
      });

      // Set up focused window change handler
      this.axio.onFocusedWindowChange((focusedWindow) => {
        this.handleFocusedWindowChange(focusedWindow);
      });

      // Set up element update handler (receives AXObserver notifications)
      this.axio.onElementUpdate((update) => {
        console.log("🔄 Element updated:", update);
        this.handleElementUpdate(update);
      });

      // Connect to websocket
      await this.axio.connect();

      console.log("📡 AXIO connected");
    } catch (error) {
      console.error("❌ Failed to connect AXIO:", error);
    }
  }

  private updateWindows(windows: Window[]) {
    // Just update the tree position if the window geometry changed
    // Focus changes are now handled by onFocusedWindowChange
    if (this.lastFocusedWindow) {
      const currentWindow = windows.find(
        (w) => w.id === this.lastFocusedWindow!.id
      );
      if (currentWindow) {
        this.lastFocusedWindow = currentWindow;
        this.updateTreePosition();
      }
    }
  }

  /**
   * Handle focused window changes (called by AXIO when focus changes)
   */
  private handleFocusedWindowChange(focusedWindow: Window | null) {
    // No focused window means either:
    // - The overlay itself is focused (most common)
    // - Desktop or other non-application window is focused
    // In either case, preserve the last tree for interaction
    if (!focusedWindow) {
      if (this.treeContainer && this.lastFocusedWindow) {
        console.log("🖱️ No focused window - preserving last tree");
        this.updateTreePosition();
      }
      return;
    }

    // Check if this is a new window or if root just arrived for current window
    const isNewWindow =
      !this.lastFocusedWindow || focusedWindow.id !== this.lastFocusedWindow.id;
    const rootJustArrived =
      focusedWindow.root && !this.lastFocusedWindow?.root && !isNewWindow;

    if (isNewWindow || rootJustArrived) {
      console.log(`🎯 Focus changed to "${focusedWindow.title}"`);
      this.lastFocusedWindow = focusedWindow;

      // Root is automatically populated by the backend
      if (focusedWindow.root) {
        this.displayAccessibilityTree(focusedWindow.root, focusedWindow);
      } else {
        console.log("⏳ Waiting for root to be pushed by backend...");
      }
    } else {
      // Same window still focused - just update position in case window moved
      this.lastFocusedWindow = focusedWindow;
      this.updateTreePosition();
    }
  }

  private displayAccessibilityTree(tree: AXNode, window: Window) {
    // Clear existing tree and reset state
    this.clearAccessibilityTree();

    // Create tree container positioned to the right of the focused window
    this.treeContainer = document.createElement("div");
    this.treeContainer.className = "accessibility-tree";

    // Position to the right of the window
    const rightX = window.x + window.w + 10; // 10px margin
    this.treeContainer.style.left = `${rightX}px`;
    this.treeContainer.style.top = `${window.y}px`;
    // Set height to match window height exactly
    this.treeContainer.style.height = `${window.h}px`;

    // Add color key legend
    const legend = document.createElement("div");
    legend.className = "tree-legend";
    legend.innerHTML = `
      <span class="legend-item"><span class="tree-role">role</span></span>
      <span class="legend-item"><span class="tree-subrole">subrole</span></span>
      <span class="legend-item"><span class="tree-label">label</span></span>
      <span class="legend-item"><span class="tree-value-string">string</span></span>
      <span class="legend-item"><span class="tree-value-number">number</span></span>
      <span class="legend-item"><span class="tree-value-boolean">bool</span></span>
      <span class="legend-item"><span class="tree-description">desc</span></span>
    `;
    this.treeContainer.appendChild(legend);

    // Create scrollable content wrapper
    const contentWrapper = document.createElement("div");
    contentWrapper.className = "tree-content";

    // Create tree content and add it to the wrapper
    console.log(`🏗️ Starting tree element creation...`);
    this.renderedNodeCount = 0; // Reset counter
    const treeContent = this.createTreeElement(tree);
    console.log(`🎯 Rendered ${this.renderedNodeCount} DOM elements`);
    contentWrapper.appendChild(treeContent);

    // Add wrapper to container
    this.treeContainer.appendChild(contentWrapper);

    this.windowContainer.appendChild(this.treeContainer);
    console.log(`✅ Displayed accessibility tree for ${window.title}`);
  }

  private createTreeElement(node: AXNode, nodeId?: string): HTMLElement {
    try {
      // Use node's ID or generate one
      if (!nodeId) {
        nodeId =
          node.id || `${node.role}-${Math.random().toString(36).substr(2, 9)}`;
      }

      const nodeElement = document.createElement("div");
      nodeElement.className = "tree-node";
      // Note: AXNode doesn't have depth; indentation is handled by CSS tree-children padding

      // Create node content
      const nodeContent = document.createElement("div");
      nodeContent.className = "tree-node-content";

      // Expand/collapse/load indicator
      const childrenCount = node.children_count ?? 0;
      const loadedChildren = node.children ?? [];
      const hasLoadedChildren = loadedChildren.length > 0;
      const hasUnloadedChildren =
        childrenCount > 0 && loadedChildren.length === 0;
      const isExpanded = this.expandedNodes.has(nodeId!);
      const isLoading = this.loadingNodes.has(nodeId!);

      const indicator = document.createElement("span");
      indicator.className = "tree-indicator";

      if (isLoading) {
        indicator.textContent = "⋯"; // Loading indicator
        indicator.style.cursor = "default";
      } else if (hasUnloadedChildren) {
        indicator.textContent = "+"; // Unloaded children
        indicator.style.cursor = "pointer";
        indicator.title = `Load ${childrenCount} children`;
      } else if (hasLoadedChildren) {
        indicator.textContent = isExpanded ? "▾" : "▸"; // Expanded/collapsed
        indicator.style.cursor = "pointer";
        indicator.title = isExpanded ? "Collapse" : "Expand";
      } else {
        indicator.textContent = "•"; // Leaf node
        indicator.style.cursor = "default";
      }

      // Add click handler for expand/collapse/load
      if (hasUnloadedChildren || hasLoadedChildren) {
        indicator.addEventListener("click", async (e) => {
          e.stopPropagation();

          if (hasUnloadedChildren && !isLoading) {
            // Load children
            await this.loadNodeChildren(nodeId!, node, nodeElement);
          } else if (hasLoadedChildren) {
            // Toggle expand/collapse
            this.toggleNodeExpansion(nodeId!, nodeElement);
          }
        });
      }

      nodeContent.appendChild(indicator);

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

      // Label (if present)
      if (node.label) {
        const labelSpan = document.createElement("span");
        labelSpan.className = "tree-label";
        labelSpan.textContent = ` "${node.label}"`;
        nodeInfo.appendChild(labelSpan);
      }

      // Value (if present) - with type-specific colors
      if (node.value) {
        const valueSpan = document.createElement("span");

        // Set color based on type
        switch (node.value.type) {
          case "String":
            valueSpan.className = "tree-value-string";
            valueSpan.textContent = ` = "${String(node.value.value)}"`;
            break;
          case "Integer":
          case "Float":
            valueSpan.className = "tree-value-number";
            valueSpan.textContent = ` = ${node.value.value}`;
            break;
          case "Boolean":
            valueSpan.className = "tree-value-boolean";
            valueSpan.textContent = ` = ${String(node.value.value)}`;
            break;
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

      // Description (if present)
      if (node.description) {
        const descSpan = document.createElement("span");
        descSpan.className = "tree-description";
        descSpan.textContent = ` desc:"${node.description}"`;
        nodeInfo.appendChild(descSpan);
      }

      // State indicators - simplified
      if (node.focused) {
        const stateSpan = document.createElement("span");
        stateSpan.className = "tree-state-focused";
        stateSpan.textContent = " [focused]";
        nodeInfo.appendChild(stateSpan);
      }
      if (node.enabled === false) {
        const stateSpan = document.createElement("span");
        stateSpan.className = "tree-state-disabled";
        stateSpan.textContent = " [disabled]";
        nodeInfo.appendChild(stateSpan);
      }

      // Children count for nodes with children (loaded or unloaded)
      if (hasLoadedChildren || hasUnloadedChildren) {
        const childCountSpan = document.createElement("span");
        childCountSpan.className = "tree-count";
        const count = hasLoadedChildren ? loadedChildren.length : childrenCount;
        childCountSpan.textContent = ` (${count})`;
        nodeInfo.appendChild(childCountSpan);
      }

      nodeContent.appendChild(nodeInfo);

      // Add hover outline for elements with position/size data
      const position = this.getPosition(node);
      const size = this.getSize(node);
      if (position && size) {
        nodeContent.style.cursor = "pointer";

        nodeContent.addEventListener("mouseenter", () => {
          this.showHoverOutline(position, size);
        });

        nodeContent.addEventListener("mouseleave", () => {
          this.hideHoverOutline();
        });
      }

      // Add live text input for writable elements
      if (
        node.id &&
        (node.role === "textbox" ||
          node.role === "searchbox" ||
          node.role === "unknown") // temporary: accept unknown for unmapped roles
      ) {
        // Container for input and regex button
        const inputContainer = document.createElement("div");
        inputContainer.className = "tree-input-container";

        // Live text input
        const textInput = document.createElement("input");
        textInput.className = "tree-text-input";
        textInput.type = "text";

        // Use the raw value for editing
        const cleanValue = node.value ? String(node.value.value) : "";
        textInput.value = cleanValue;
        textInput.placeholder = "Enter text...";

        // Add write functionality on Enter key
        textInput.addEventListener("keydown", async (e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            e.stopPropagation();

            // Use node's setValue method
            if (node.setValue) {
              try {
                await node.setValue(textInput.value);
                console.log(`✅ Wrote "${textInput.value}" to node`);
              } catch (error) {
                console.error("❌ Failed to write:", error);
              }
            }
          }
        });

        // Prevent input clicks from affecting tree expansion
        textInput.addEventListener("click", (e) => {
          e.stopPropagation();
        });

        // Regex button
        const regexButton = document.createElement("button");
        regexButton.className = "tree-regex-button";
        regexButton.textContent = ".*";
        regexButton.title = "Open regex find & replace";

        regexButton.addEventListener("click", (e) => {
          e.stopPropagation();

          // Use raw parsed value for regex operations (not role-formatted)
          this.openRegexPanel(node, cleanValue, position, size);
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
      if (hasLoadedChildren) {
        const childrenContainer = document.createElement("div");
        childrenContainer.className = "tree-children";

        // Hide children if not expanded
        if (!isExpanded) {
          childrenContainer.style.display = "none";
        }

        for (const child of loadedChildren) {
          childrenContainer.appendChild(this.createTreeElement(child));
        }

        nodeElement.appendChild(childrenContainer);
      }

      // Store node element for later updates
      this.nodeElements.set(nodeId!, { element: nodeElement, node });

      // Auto-watch leaf nodes (nodes with no children) since they can't be expanded
      // This ensures text fields and other leaf elements get watched for changes
      const isLeafNode = !hasLoadedChildren && !hasUnloadedChildren;
      if (isLeafNode && nodeId && node.id) {
        console.log(`👁️ Auto-watching leaf node: ${node.role}`);
        this.axio.watchNodeByElementId(node.id, nodeId).catch((err) => {
          console.error(`Failed to auto-watch leaf node ${nodeId}:`, err);
        });
      }

      this.renderedNodeCount++; // Increment counter for each node rendered
      return nodeElement;
    } catch (error) {
      console.error("Error creating tree element:", error);
      return document.createElement("div"); // Return a placeholder to avoid breaking rendering
    }
  }

  private updateTreePosition() {
    if (this.treeContainer && this.lastFocusedWindow) {
      const rightX = this.lastFocusedWindow.x + this.lastFocusedWindow.w + 10;
      this.treeContainer.style.left = `${rightX}px`;
      this.treeContainer.style.top = `${this.lastFocusedWindow.y}px`;
      // Set height to match window height exactly
      this.treeContainer.style.height = `${this.lastFocusedWindow.h}px`;
    }

    // Also update regex panel position if it's open
    this.updateRegexPanelPosition();
  }

  private updateRegexPanelPosition() {
    if (
      this.regexPanel &&
      this.regexPanel.classList.contains("positioned-relative") &&
      this.lastFocusedWindow
    ) {
      // Get the target element's position and size from when the panel was opened
      if (this.currentTargetElement) {
        const node = this.currentTargetElement.node;

        // Use the node's bounds directly
        if (node.bounds) {
          const x = node.bounds.position.x;
          const y = node.bounds.position.y;
          const width = node.bounds.size.width;
          const height = node.bounds.size.height;

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

  private clearAccessibilityTree() {
    if (this.treeContainer) {
      this.treeContainer.remove();
      this.treeContainer = null;
    }
    // Clear hover outline
    this.hideHoverOutline();
    // Clear node tracking state
    this.expandedNodes.clear();
    this.loadingNodes.clear();
    this.nodeElements.clear();
  }

  private showHoverOutline(position: [number, number], size: [number, number]) {
    // Create outline element if it doesn't exist
    if (!this.hoverOutline) {
      this.hoverOutline = document.createElement("div");
      this.hoverOutline.style.cssText = `
        position: absolute;
        pointer-events: none;
        border: 2px solid #007aff;
        background: rgba(0, 122, 255, 0.1);
        z-index: 999;
        transition: all 0.1s ease-out;
      `;
      this.windowContainer.appendChild(this.hoverOutline);
    }

    // Update position and size
    const [x, y] = position;
    const [width, height] = size;

    this.hoverOutline.style.left = `${x}px`;
    this.hoverOutline.style.top = `${y}px`;
    this.hoverOutline.style.width = `${width}px`;
    this.hoverOutline.style.height = `${height}px`;
    this.hoverOutline.style.display = "block";
  }

  private hideHoverOutline() {
    if (this.hoverOutline) {
      this.hoverOutline.style.display = "none";
    }
  }

  private openRegexPanel(
    node: AXNode,
    currentValue: string,
    elementPosition?: [number, number],
    elementSize?: [number, number]
  ) {
    // Close existing panel if open
    this.closeRegexPanel();

    this.currentTargetElement = { node, currentValue };

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

    // Apply regex function
    const applyRegex = async () => {
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

        // Apply the change using node's setValue method
        if (this.currentTargetElement!.node.setValue) {
          await this.currentTargetElement!.node.setValue(result);
          console.log(`✅ Applied regex to node`);
        }

        // Update stored current value
        this.currentTargetElement!.currentValue = result;

        // Close panel
        this.closeRegexPanel();
      } catch (e) {
        console.error("Regex application failed:", e);
      }
    };

    // Event listeners
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

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  private filterEmptyGroups(node: AXNode): AXNode | null {
    // First, recursively filter children
    const children = node.children ?? [];
    const filteredChildren = children
      .map((child) => this.filterEmptyGroups(child))
      .filter((child): child is AXNode => child !== null);

    // Check if this is an empty group that should be filtered out
    if (node.role === "group") {
      // Filter out group if it has no meaningful content
      const hasLabel = node.label && node.label.trim() !== "";
      const hasValue = node.value !== undefined;
      const hasDescription = node.description && node.description.trim() !== "";
      const hasPlaceholder = node.placeholder && node.placeholder.trim() !== "";
      const hasLoadedChildren = filteredChildren.length > 0;
      const hasUnloadedChildren = (node.children_count ?? 0) > 0; // Unloaded children to lazy-load

      // Keep the group only if it has some meaningful content or children (loaded or unloaded)
      if (
        !hasLabel &&
        !hasValue &&
        !hasDescription &&
        !hasPlaceholder &&
        !hasLoadedChildren &&
        !hasUnloadedChildren
      ) {
        return null; // Filter out this empty group
      }
    }

    // Return the node with filtered children
    return { ...node, children: filteredChildren };
  }

  /**
   * Handle typed element update from AXObserver (backend push notification)
   * Updates the node data and DOM elements based on the update type
   */
  private handleElementUpdate(update: ElementUpdate) {
    const stored = this.nodeElements.get(update.element_id);

    if (!stored) {
      console.warn(`Element ${update.element_id} not found in rendered nodes`);
      return;
    }

    const { element, node } = stored;

    switch (update.update_type) {
      case "ValueChanged":
        // Update the node's value
        (node as any).value = update.value;
        console.log(
          `  ✏️  Value changed for ${update.element_id}:`,
          update.value
        );

        // Update the value display in the DOM
        const valueSpan = element.querySelector(
          ".tree-node-content .tree-value-string, .tree-node-content .tree-value-number, .tree-node-content .tree-value-boolean"
        ) as HTMLElement;

        if (valueSpan) {
          switch (update.value.type) {
            case "String":
              valueSpan.className = "tree-value-string";
              valueSpan.textContent = ` = "${String(update.value.value)}"`;
              break;
            case "Integer":
            case "Float":
              valueSpan.className = "tree-value-number";
              valueSpan.textContent = ` = ${update.value.value}`;
              break;
            case "Boolean":
              valueSpan.className = "tree-value-boolean";
              valueSpan.textContent = ` = ${String(update.value.value)}`;
              break;
          }
        }

        // Also update input fields if present
        const inputField = element.querySelector(
          ".tree-text-input"
        ) as HTMLInputElement;
        if (inputField) {
          inputField.value = String(update.value.value);
        }
        break;

      case "LabelChanged":
        // Update the node's label
        (node as any).label = update.label;
        console.log(
          `  🏷️  Label changed for ${update.element_id}:`,
          update.label
        );

        // Update label in DOM
        const labelSpan = element.querySelector(".tree-label") as HTMLElement;
        if (labelSpan) {
          labelSpan.textContent = ` "${update.label}"`;
        }
        break;

      case "ElementDestroyed":
        // Remove element from DOM and tracking
        console.log(`  💀 Element destroyed: ${update.element_id}`);
        element.remove();
        this.nodeElements.delete(update.element_id);
        break;
    }
  }

  /**
   * Toggle expansion of a node's children
   */

  private toggleNodeExpansion(nodeId: string, nodeElement: HTMLElement) {
    const childrenContainer = nodeElement.querySelector(
      ".tree-children"
    ) as HTMLElement;
    if (!childrenContainer) return;

    const isExpanded = this.expandedNodes.has(nodeId);

    if (isExpanded) {
      // Collapse
      this.expandedNodes.delete(nodeId);
      childrenContainer.style.display = "none";

      // Unwatch this node (stop receiving updates)
      const stored = this.nodeElements.get(nodeId);
      if (stored) {
        this.axio.unwatchNodeByElementId(stored.node.id).catch((err) => {
          console.error(`Failed to unwatch node ${nodeId}:`, err);
        });
      }

      // Update indicator
      const indicator = nodeElement.querySelector(".tree-indicator");
      if (indicator) {
        indicator.textContent = "▸";
        indicator.setAttribute("title", "Expand");
      }
    } else {
      // Expand
      this.expandedNodes.add(nodeId);
      childrenContainer.style.display = "block";

      // Watch this node (start receiving updates)
      const stored = this.nodeElements.get(nodeId);
      if (stored) {
        this.axio.watchNodeByElementId(stored.node.id, nodeId).catch((err) => {
          console.error(`Failed to watch node ${nodeId}:`, err);
        });
      }

      // Update indicator
      const indicator = nodeElement.querySelector(".tree-indicator");
      if (indicator) {
        indicator.textContent = "▾";
        indicator.setAttribute("title", "Collapse");
      }
    }
  }

  /**
   * Load children for a node using lazy loading
   */
  private async loadNodeChildren(
    nodeId: string,
    node: AXNode,
    nodeElement: HTMLElement
  ) {
    // Mark as loading
    this.loadingNodes.add(nodeId);

    // Update indicator
    const indicator = nodeElement.querySelector(".tree-indicator");
    if (indicator) {
      indicator.textContent = "⋯";
      indicator.setAttribute("title", "Loading...");
    }

    try {
      console.log(
        `📥 Loading children for ${node.role} (${
          node.children_count ?? "?"
        } children)`
      );

      // Fetch children using the node's getChildren method
      if (!node.getChildren) {
        throw new Error("Node does not have getChildren method");
      }

      const children = await node.getChildren();
      console.log(`✅ Loaded ${children.length} children`);

      // Update the stored node with loaded children
      const updatedNode = { ...node, children };
      this.nodeElements.set(nodeId, {
        element: nodeElement,
        node: updatedNode,
      });

      // Create and add children container
      const childrenContainer = document.createElement("div");
      childrenContainer.className = "tree-children";

      for (const child of children) {
        childrenContainer.appendChild(this.createTreeElement(child));
      }

      nodeElement.appendChild(childrenContainer);

      // Auto-expand after loading
      this.expandedNodes.add(nodeId);

      // Watch this node for changes (now that it's expanded)
      this.axio.watchNodeByElementId(node.id, nodeId).catch((err) => {
        console.error(`Failed to watch node ${nodeId}:`, err);
      });

      // Update indicator
      if (indicator && indicator instanceof HTMLElement) {
        indicator.textContent = "▾";
        indicator.setAttribute("title", "Collapse");
        indicator.style.cursor = "pointer";
      }
    } catch (error) {
      console.error("Failed to load children:", error);

      // Update indicator to show error
      if (indicator) {
        indicator.textContent = "✗";
        indicator.setAttribute("title", `Failed to load: ${error}`);
      }
    } finally {
      this.loadingNodes.delete(nodeId);
    }
  }
}

// Initialize the overlay when DOM is loaded
document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
