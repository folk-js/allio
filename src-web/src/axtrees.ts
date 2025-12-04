import { AXIO, AXElement, AXWindow } from "@axio/client";

class AXTreeOverlay {
  private windowContainer: HTMLElement;
  private axio: AXIO;
  private treeContainer: HTMLElement | null = null;
  private regexPanel: HTMLElement | null = null;
  private currentTargetElement: {
    element: AXElement;
    currentValue: string;
  } | null = null;
  private renderedNodeCount: number = 0;
  private hoverOutline: HTMLElement | null = null;
  private expandedNodes: Set<string> = new Set();
  private loadingNodes: Set<string> = new Set();
  private nodeElements: Map<
    string,
    { domElement: HTMLElement; element: AXElement }
  > = new Map();
  private isClickthroughEnabled: boolean = true;

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.axio = new AXIO();
    this.setupWebSocketListener();
    this.setupCursorTransparency();
  }

  private setupCursorTransparency() {
    this.axio.on("mouse", ({ x, y }) => {
      const elementUnderCursor = document.elementFromPoint(x, y);
      const isOverSidebar =
        elementUnderCursor && this.isElementInSidebar(elementUnderCursor);
      const shouldEnableClickthrough = !isOverSidebar;

      if (shouldEnableClickthrough !== this.isClickthroughEnabled) {
        this.isClickthroughEnabled = shouldEnableClickthrough;
        this.axio.setClickthrough(shouldEnableClickthrough).catch((err) => {
          console.error("Failed to set clickthrough:", err);
        });
      }
    });
  }

  private isElementInSidebar(element: Element): boolean {
    let current: Element | null = element;
    while (current) {
      if (current === this.treeContainer || current === this.regexPanel) {
        return true;
      }
      current = current.parentElement;
    }
    return false;
  }

  private getPosition(element: AXElement): [number, number] | undefined {
    return element.bounds
      ? [element.bounds.position.x, element.bounds.position.y]
      : undefined;
  }

  private getSize(element: AXElement): [number, number] | undefined {
    return element.bounds
      ? [element.bounds.size.width, element.bounds.size.height]
      : undefined;
  }

  private async setupWebSocketListener() {
    try {
      this.axio.on("windows", (windows) => this.updateWindows(windows));
      this.axio.on("focus", (focused) =>
        this.handleFocusedWindowChange(focused)
      );
      this.axio.on("elements", (elements) => {
        for (const element of elements) {
          this.handleElementUpdate(element);
        }
        this.checkForRoot();
      });
      this.axio.on("destroyed", (elementId) => {
        this.handleElementDestroyed(elementId);
      });

      await this.axio.connect();
      console.log("📡 AXIO connected");
    } catch (error) {
      console.error("❌ Failed to connect AXIO:", error);
    }
  }

  private updateWindows(_windows: AXWindow[]) {
    this.checkForRoot();
    this.updateTreePosition();
  }

  /** Check if we now have a root for the focused window and display tree */
  private checkForRoot() {
    const focused = this.axio.focused;
    if (!focused || this.treeContainer) return;

    const root = this.axio.getRoot(focused);
    if (root) {
      this.displayAccessibilityTree(root, focused);
    }
  }

  private handleFocusedWindowChange(focused: AXWindow | null) {
    this.clearAccessibilityTree();
    if (focused) {
      this.checkForRoot();
    }
  }

  private displayAccessibilityTree(root: AXElement, window: AXWindow) {
    this.clearAccessibilityTree();

    this.treeContainer = document.createElement("div");
    this.treeContainer.className = "accessibility-tree";

    const rightX = window.x + window.w + 10;
    this.treeContainer.style.left = `${rightX}px`;
    this.treeContainer.style.top = `${window.y}px`;
    this.treeContainer.style.height = `${window.h}px`;

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

    const contentWrapper = document.createElement("div");
    contentWrapper.className = "tree-content";

    console.log(`🏗️ Starting tree element creation...`);
    this.renderedNodeCount = 0;
    const treeContent = this.createTreeElement(root);
    console.log(`🎯 Rendered ${this.renderedNodeCount} DOM elements`);
    contentWrapper.appendChild(treeContent);

    this.treeContainer.appendChild(contentWrapper);
    this.windowContainer.appendChild(this.treeContainer);
    console.log(`✅ Displayed accessibility tree for ${window.title}`);
  }

  private createTreeElement(element: AXElement): HTMLElement {
    try {
      const nodeElement = document.createElement("div");
      nodeElement.className = "tree-node";

      const nodeContent = document.createElement("div");
      nodeContent.className = "tree-node-content";

      // Determine children state
      // children_ids: null = not discovered, [] = no children, [...] = has children
      const childrenDiscovered = element.children_ids !== null;
      const children = this.axio.getChildren(element);
      const hasChildren = children.length > 0;
      const isExpanded = this.expandedNodes.has(element.id);
      const isLoading = this.loadingNodes.has(element.id);

      const indicator = document.createElement("span");
      indicator.className = "tree-indicator";

      if (isLoading) {
        indicator.textContent = "⋯";
        indicator.style.cursor = "default";
      } else if (!childrenDiscovered) {
        indicator.textContent = "+";
        indicator.style.cursor = "pointer";
        indicator.title = "Load children";
      } else if (hasChildren) {
        indicator.textContent = isExpanded ? "▾" : "▸";
        indicator.style.cursor = "pointer";
        indicator.title = isExpanded ? "Collapse" : "Expand";
      } else {
        indicator.textContent = "•";
        indicator.style.cursor = "default";
      }

      if (!childrenDiscovered || hasChildren) {
        indicator.addEventListener("click", async (e) => {
          e.stopPropagation();

          if (!childrenDiscovered && !isLoading) {
            await this.loadNodeChildren(element, nodeElement);
          } else if (hasChildren) {
            this.toggleNodeExpansion(element.id, nodeElement);
          }
        });
      }

      nodeContent.appendChild(indicator);

      const nodeInfo = document.createElement("span");
      nodeInfo.style.flex = "1";

      const roleSpan = document.createElement("span");
      roleSpan.className = "tree-role";
      roleSpan.textContent = element.role;
      nodeInfo.appendChild(roleSpan);

      if (element.subrole && element.subrole !== element.role) {
        const subroleSpan = document.createElement("span");
        subroleSpan.className = "tree-subrole";
        subroleSpan.textContent = `:${element.subrole}`;
        nodeInfo.appendChild(subroleSpan);
      }

      if (element.label) {
        const labelSpan = document.createElement("span");
        labelSpan.className = "tree-label";
        labelSpan.textContent = ` "${element.label}"`;
        nodeInfo.appendChild(labelSpan);
      }

      if (element.value) {
        const valueSpan = document.createElement("span");
        switch (element.value.type) {
          case "String":
            valueSpan.className = "tree-value-string";
            valueSpan.textContent = ` = "${String(element.value.value)}"`;
            break;
          case "Integer":
          case "Float":
            valueSpan.className = "tree-value-number";
            valueSpan.textContent = ` = ${element.value.value}`;
            break;
          case "Boolean":
            valueSpan.className = "tree-value-boolean";
            valueSpan.textContent = ` = ${String(element.value.value)}`;
            break;
        }
        nodeInfo.appendChild(valueSpan);
      }

      if (element.placeholder) {
        const placeholderSpan = document.createElement("span");
        placeholderSpan.className = "tree-placeholder";
        placeholderSpan.textContent = ` placeholder:"${element.placeholder}"`;
        nodeInfo.appendChild(placeholderSpan);
      }

      if (element.description) {
        const descSpan = document.createElement("span");
        descSpan.className = "tree-description";
        descSpan.textContent = ` desc:"${element.description}"`;
        nodeInfo.appendChild(descSpan);
      }

      if (element.focused) {
        const stateSpan = document.createElement("span");
        stateSpan.className = "tree-state-focused";
        stateSpan.textContent = " [focused]";
        nodeInfo.appendChild(stateSpan);
      }
      if (element.enabled === false) {
        const stateSpan = document.createElement("span");
        stateSpan.className = "tree-state-disabled";
        stateSpan.textContent = " [disabled]";
        nodeInfo.appendChild(stateSpan);
      }

      if (!childrenDiscovered || hasChildren) {
        const childCountSpan = document.createElement("span");
        childCountSpan.className = "tree-count";
        const count = childrenDiscovered ? children.length : "?";
        childCountSpan.textContent = ` (${count})`;
        nodeInfo.appendChild(childCountSpan);
      }

      nodeContent.appendChild(nodeInfo);

      const position = this.getPosition(element);
      const size = this.getSize(element);
      if (position && size) {
        nodeContent.style.cursor = "pointer";
        nodeContent.addEventListener("mouseenter", () => {
          this.showHoverOutline(position, size);
        });
        nodeContent.addEventListener("mouseleave", () => {
          this.hideHoverOutline();
        });
      }

      if (
        element.id &&
        (element.role === "textbox" ||
          element.role === "searchbox" ||
          element.role === "unknown")
      ) {
        const inputContainer = document.createElement("div");
        inputContainer.className = "tree-input-container";

        const textInput = document.createElement("input");
        textInput.className = "tree-text-input";
        textInput.type = "text";

        const cleanValue = element.value ? String(element.value.value) : "";
        textInput.value = cleanValue;
        textInput.placeholder = "Enter text...";

        textInput.addEventListener("keydown", async (e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            e.stopPropagation();
            try {
              await this.axio.write(element.id, textInput.value);
              console.log(`✅ Wrote "${textInput.value}" to element`);
            } catch (error) {
              console.error("❌ Failed to write:", error);
            }
          }
        });

        textInput.addEventListener("click", (e) => e.stopPropagation());

        const regexButton = document.createElement("button");
        regexButton.className = "tree-regex-button";
        regexButton.textContent = ".*";
        regexButton.title = "Open regex find & replace";

        regexButton.addEventListener("click", (e) => {
          e.stopPropagation();
          this.openRegexPanel(element, cleanValue, position, size);
        });

        inputContainer.appendChild(textInput);
        inputContainer.appendChild(regexButton);
        inputContainer.addEventListener("click", (e) => e.stopPropagation());

        nodeContent.appendChild(inputContainer);
      }

      nodeElement.appendChild(nodeContent);

      if (hasChildren) {
        const childrenContainer = document.createElement("div");
        childrenContainer.className = "tree-children";

        if (!isExpanded) {
          childrenContainer.style.display = "none";
        }

        for (const child of children) {
          childrenContainer.appendChild(this.createTreeElement(child));
        }

        nodeElement.appendChild(childrenContainer);
      }

      this.nodeElements.set(element.id, { domElement: nodeElement, element });
      this.renderedNodeCount++;
      return nodeElement;
    } catch (error) {
      console.error("Error creating tree element:", error);
      return document.createElement("div");
    }
  }

  private updateTreePosition() {
    const focused = this.axio.focused;
    if (this.treeContainer && focused) {
      const rightX = focused.x + focused.w + 10;
      this.treeContainer.style.left = `${rightX}px`;
      this.treeContainer.style.top = `${focused.y}px`;
      this.treeContainer.style.height = `${focused.h}px`;
    }
    this.updateRegexPanelPosition();
  }

  private updateRegexPanelPosition() {
    if (
      this.regexPanel &&
      this.regexPanel.classList.contains("positioned-relative") &&
      this.axio.focused
    ) {
      if (this.currentTargetElement) {
        const element = this.currentTargetElement.element;
        if (element.bounds) {
          const x = element.bounds.position.x;
          const y = element.bounds.position.y;
          const width = element.bounds.size.width;
          const height = element.bounds.size.height;

          const panelX = Math.max(10, x + width / 2 - 140);
          const panelY = y + height + 6;

          const maxX = window.screen.width - 300;
          const maxY = window.screen.height - 180;

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
    this.hideHoverOutline();
    this.expandedNodes.clear();
    this.loadingNodes.clear();
    this.nodeElements.clear();
  }

  private showHoverOutline(position: [number, number], size: [number, number]) {
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
    element: AXElement,
    currentValue: string,
    elementPosition?: [number, number],
    elementSize?: [number, number]
  ) {
    this.closeRegexPanel();

    this.currentTargetElement = { element, currentValue };

    this.regexPanel = document.createElement("div");
    this.regexPanel.className = "regex-panel";

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
      const [x, y] = elementPosition;
      const [width, height] = elementSize;

      const panelX = Math.max(10, x + width / 2 - 140);
      const panelY = y + height + 6;

      const maxX = window.screen.width - 300;
      const maxY = window.screen.height - 180;

      const finalX = Math.min(panelX, maxX);
      const finalY = Math.min(panelY, maxY);

      panelStyle += `
        left: ${finalX}px;
        top: ${finalY}px;
        transform: none;
      `;

      this.regexPanel.classList.add("positioned-relative");
    } else {
      panelStyle += `
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
      `;
    }

    this.regexPanel.style.cssText = panelStyle;

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

    const applyRegex = async () => {
      const pattern = patternInput.value.trim();
      const replacement = replaceInput.value;

      if (!pattern) return;

      try {
        let regex: RegExp;
        const regexMatch = pattern.match(/^\/(.+)\/([gimuy]*)$/);

        if (regexMatch) {
          regex = new RegExp(regexMatch[1], regexMatch[2]);
        } else {
          regex = new RegExp(pattern, "g");
        }

        const result = this.applyRegexWithTransforms(
          this.currentTargetElement!.currentValue,
          regex,
          replacement
        );

        await this.axio.write(this.currentTargetElement!.element.id, result);
        console.log(`✅ Applied regex to element`);

        this.currentTargetElement!.currentValue = result;
        this.closeRegexPanel();
      } catch (e) {
        console.error("Regex application failed:", e);
      }
    };

    applyButton.addEventListener("click", applyRegex);

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

    buttonContainer.appendChild(cancelButton);
    buttonContainer.appendChild(applyButton);

    this.regexPanel.appendChild(header);
    this.regexPanel.appendChild(patternLabel);
    this.regexPanel.appendChild(patternInput);
    this.regexPanel.appendChild(replaceLabel);
    this.regexPanel.appendChild(replaceInput);
    this.regexPanel.appendChild(buttonContainer);

    document.body.appendChild(this.regexPanel);
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
    const hasTransforms = /\$\d+:(upper|lower|title|capitalize)/i.test(
      replacement
    );

    if (!hasTransforms) {
      return text.replace(regex, replacement);
    }

    return text.replace(regex, (...args) => {
      const captures = args.slice(1, -2);
      let result = replacement;

      result = result.replace(
        /\$(\d+):(upper|lower|title|capitalize)/gi,
        (transformMatch, groupNum, transform) => {
          const groupIndex = parseInt(groupNum) - 1;

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
          return transformMatch;
        }
      );

      result = result.replace(/\$(\d+)/g, (groupMatch, groupNum) => {
        const groupIndex = parseInt(groupNum) - 1;
        return groupIndex >= 0 && groupIndex < captures.length
          ? captures[groupIndex]
          : groupMatch;
      });

      return result;
    });
  }

  private handleElementUpdate(element: AXElement) {
    const stored = this.nodeElements.get(element.id);

    if (!stored) {
      return;
    }

    const { domElement } = stored;

    // Update value display
    if (element.value) {
      const valueSpan = domElement.querySelector(
        ".tree-node-content .tree-value-string, .tree-node-content .tree-value-number, .tree-node-content .tree-value-boolean"
      ) as HTMLElement;

      if (valueSpan) {
        switch (element.value.type) {
          case "String":
            valueSpan.className = "tree-value-string";
            valueSpan.textContent = ` = "${String(element.value.value)}"`;
            break;
          case "Integer":
          case "Float":
            valueSpan.className = "tree-value-number";
            valueSpan.textContent = ` = ${element.value.value}`;
            break;
          case "Boolean":
            valueSpan.className = "tree-value-boolean";
            valueSpan.textContent = ` = ${String(element.value.value)}`;
            break;
        }
      }

      const inputField = domElement.querySelector(
        ".tree-text-input"
      ) as HTMLInputElement;
      if (inputField) {
        inputField.value = String(element.value.value);
      }
    }

    // Update label
    if (element.label) {
      const labelSpan = domElement.querySelector(".tree-label") as HTMLElement;
      if (labelSpan) {
        labelSpan.textContent = ` "${element.label}"`;
      }
    }

    // Update stored element reference
    stored.element = element;
  }

  private handleElementDestroyed(elementId: string) {
    const stored = this.nodeElements.get(elementId);
    if (stored) {
      stored.domElement.remove();
      this.nodeElements.delete(elementId);
    }
  }

  private toggleNodeExpansion(elementId: string, nodeElement: HTMLElement) {
    const childrenContainer = nodeElement.querySelector(
      ".tree-children"
    ) as HTMLElement;
    if (!childrenContainer) return;

    const isExpanded = this.expandedNodes.has(elementId);

    if (isExpanded) {
      this.expandedNodes.delete(elementId);
      childrenContainer.style.display = "none";

      this.axio.unwatch(elementId).catch((err) => {
        console.error(`Failed to unwatch element:`, err);
      });

      const indicator = nodeElement.querySelector(".tree-indicator");
      if (indicator) {
        indicator.textContent = "▸";
        indicator.setAttribute("title", "Expand");
      }
    } else {
      this.expandedNodes.add(elementId);
      childrenContainer.style.display = "block";

      this.axio.watch(elementId).catch((err) => {
        console.error(`Failed to watch element:`, err);
      });

      const indicator = nodeElement.querySelector(".tree-indicator");
      if (indicator) {
        indicator.textContent = "▾";
        indicator.setAttribute("title", "Collapse");
      }
    }
  }

  private async loadNodeChildren(element: AXElement, nodeElement: HTMLElement) {
    this.loadingNodes.add(element.id);

    const indicator = nodeElement.querySelector(".tree-indicator");
    if (indicator) {
      indicator.textContent = "⋯";
      indicator.setAttribute("title", "Loading...");
    }

    try {
      console.log(`📥 Loading children for ${element.role}`);

      const children = await this.axio.children(element.id);
      console.log(`✅ Loaded ${children.length} children`);

      // Update stored element reference
      const updatedElement = this.axio.get(element.id);
      if (updatedElement) {
        this.nodeElements.set(element.id, {
          domElement: nodeElement,
          element: updatedElement,
        });
      }

      const childrenContainer = document.createElement("div");
      childrenContainer.className = "tree-children";

      for (const child of children) {
        childrenContainer.appendChild(this.createTreeElement(child));
      }

      nodeElement.appendChild(childrenContainer);

      this.expandedNodes.add(element.id);

      this.axio.watch(element.id).catch((err) => {
        console.error(`Failed to watch element:`, err);
      });

      if (indicator && indicator instanceof HTMLElement) {
        indicator.textContent = "▾";
        indicator.setAttribute("title", "Collapse");
        indicator.style.cursor = "pointer";
      }
    } catch (error) {
      console.error("Failed to load children:", error);

      if (indicator) {
        indicator.textContent = "✗";
        indicator.setAttribute("title", `Failed to load: ${error}`);
      }
    } finally {
      this.loadingNodes.delete(element.id);
    }
  }
}

document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
