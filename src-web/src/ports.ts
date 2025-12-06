import { AXIO, AXElement, AXWindow, OcclusionManager } from "@axio/client";

type PortType = "input" | "output";

interface Port {
  id: string;
  windowId: string;
  element: AXElement;
  type: PortType;
  x: number;
  y: number;
}

interface Connection {
  id: string;
  sourcePort: Port;
  targetPort: Port;
}

/**
 * Ports Demo - Connect UI elements across windows
 *
 * Usage:
 * - Click the menu bar to enter/exit port creation mode
 * - In creation mode, click anywhere to create ports for that element
 * - Drag from output (right) to input (left) to connect
 * - Shift+click a port to delete it
 */
class PortsDemo {
  private container: HTMLElement;
  private svg: SVGElement;
  private menuBar: HTMLElement;
  private axio: AXIO;
  private occlusion: OcclusionManager;

  private ports = new Map<string, Port>();
  private connections: Connection[] = [];
  private portElements = new Map<string, HTMLElement>();
  private windowContainers = new Map<string, HTMLElement>();
  private edgeGroups = new Map<
    string,
    { left: HTMLElement; right: HTMLElement }
  >();

  // Mode state
  private creationMode = false;

  // Drag connection state
  private connectingFrom: Port | null = null;
  private tempLine: SVGPathElement | null = null;

  // Hover state
  private hoveredPort: Port | null = null;
  private boundsOverlay: HTMLElement | null = null;
  private infoPanel: HTMLElement | null = null;
  private wiringSvg: SVGSVGElement | null = null;
  private wiringPath: SVGPathElement | null = null;

  constructor() {
    this.container = document.getElementById("portContainer")!;
    this.svg = document.getElementById("connections") as unknown as SVGElement;
    this.menuBar = document.getElementById("menuBar")!;
    this.axio = new AXIO();
    this.occlusion = new OcclusionManager(this.axio);
    this.createHoverOverlay();
    this.init();
  }

  private createHoverOverlay() {
    // Bounds overlay - shows element rectangle
    this.boundsOverlay = document.createElement("div");
    this.boundsOverlay.style.cssText = `
      position: absolute;
      pointer-events: none;
      border: 2px solid var(--port-output);
      border-radius: 4px;
      background: rgba(107, 143, 199, 0.1);
      box-sizing: border-box;
      display: none;
      z-index: 999;
    `;
    document.body.appendChild(this.boundsOverlay);

    // Internal wiring SVG - shows connection from element to port
    this.wiringSvg = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "svg"
    );
    this.wiringSvg.style.cssText = `
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      pointer-events: none;
      z-index: 998;
      display: none;
    `;

    // Add keyframes animation inside SVG
    const style = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "style"
    );
    style.textContent = `
      @keyframes wiringFlowOut {
        to { stroke-dashoffset: -10; }
      }
      @keyframes wiringFlowIn {
        to { stroke-dashoffset: 10; }
      }
      .wiring-path-output {
        animation: wiringFlowOut 0.4s linear infinite;
      }
      .wiring-path-input {
        animation: wiringFlowIn 0.4s linear infinite;
      }
    `;
    this.wiringSvg.appendChild(style);

    this.wiringPath = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "path"
    );
    this.wiringPath.setAttribute("fill", "none");
    this.wiringPath.setAttribute("stroke-width", "2");
    this.wiringPath.setAttribute("stroke-dasharray", "6,4");
    this.wiringPath.setAttribute("opacity", "0.7");
    this.wiringSvg.appendChild(this.wiringPath);
    document.body.appendChild(this.wiringSvg);

    // Info panel - shows element details
    this.infoPanel = document.createElement("div");
    this.infoPanel.style.cssText = `
      position: absolute;
      pointer-events: none;
      background: rgba(30, 30, 30, 0.95);
      border: 1px solid rgba(255, 255, 255, 0.15);
      border-radius: 6px;
      padding: 8px 12px;
      font-size: 11px;
      color: rgba(255, 255, 255, 0.9);
      backdrop-filter: blur(10px);
      display: none;
      z-index: 1000;
      max-width: 280px;
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", sans-serif;
    `;
    document.body.appendChild(this.infoPanel);
  }

  private async onPortHoverEnter(port: Port) {
    this.hoveredPort = port;

    // Refresh element data from AXIO
    try {
      const freshElement = await this.axio.refresh(port.element.id);
      port.element = freshElement;
    } catch {
      // Element might be gone, use cached data
    }

    this.showHoverOverlay(port);
  }

  private onPortHoverLeave() {
    this.hoveredPort = null;
    this.hideHoverOverlay();
  }

  private showHoverOverlay(port: Port) {
    const { element } = port;
    if (!element.bounds || !this.boundsOverlay || !this.infoPanel) return;

    // Position bounds overlay
    const { x, y, w, h } = element.bounds;
    this.boundsOverlay.style.left = `${x}px`;
    this.boundsOverlay.style.top = `${y}px`;
    this.boundsOverlay.style.width = `${w}px`;
    this.boundsOverlay.style.height = `${h}px`;
    this.boundsOverlay.style.display = "block";

    // Build info content
    const lines: string[] = [];
    lines.push(
      `<div style="color: var(--port-output); font-weight: 600; margin-bottom: 4px;">${
        element.role
      }${element.subrole ? ` / ${element.subrole}` : ""}</div>`
    );

    if (element.label) {
      lines.push(
        `<div><span style="opacity: 0.6;">Label:</span> ${this.escapeHtml(
          element.label
        )}</div>`
      );
    }
    if (element.value) {
      const val = element.value.value;
      const displayVal = typeof val === "string" ? `"${val}"` : String(val);
      lines.push(
        `<div><span style="opacity: 0.6;">Value:</span> <span style="color: var(--port-input);">${this.escapeHtml(
          displayVal
        )}</span></div>`
      );
    }
    if (element.description) {
      lines.push(
        `<div style="opacity: 0.7; font-style: italic; margin-top: 2px;">${this.escapeHtml(
          element.description
        )}</div>`
      );
    }
    if (element.enabled === false) {
      lines.push(
        `<div style="color: #ff6b6b; margin-top: 2px;">Disabled</div>`
      );
    }
    if (element.actions.length > 0) {
      lines.push(
        `<div style="opacity: 0.5; margin-top: 4px; font-size: 10px;">Actions: ${element.actions.join(
          ", "
        )}</div>`
      );
    }

    this.infoPanel.innerHTML = lines.join("");

    // Position info panel near bounds (below or above depending on space)
    const panelHeight = 100; // estimate
    const belowY = y + h + 8;
    const aboveY = y - panelHeight - 8;

    this.infoPanel.style.left = `${x}px`;
    this.infoPanel.style.top =
      belowY + panelHeight < window.innerHeight
        ? `${belowY}px`
        : `${Math.max(8, aboveY)}px`;
    this.infoPanel.style.display = "block";

    // Draw internal wiring from element to port
    this.drawWiringLine(port, element.bounds);
  }

  private drawWiringLine(
    port: Port,
    bounds: { x: number; y: number; w: number; h: number }
  ) {
    if (!this.wiringSvg || !this.wiringPath) return;

    const { x, y, w, h } = bounds;

    // Calculate connection points
    // Element side: edge of bounds closest to port
    // Port side: port position
    const portX = port.x;
    const portY = port.y;

    let elemX: number, elemY: number;
    if (port.type === "input") {
      // Input port is on left, connect from left edge of element
      elemX = x;
      elemY = Math.max(y, Math.min(y + h, portY)); // Clamp to element vertical range
    } else {
      // Output port is on right, connect from right edge of element
      elemX = x + w;
      elemY = Math.max(y, Math.min(y + h, portY));
    }

    // Only show wiring if there's meaningful distance (at least 20px)
    const distance = Math.abs(portX - elemX);
    if (distance < 20) {
      this.wiringSvg.style.display = "none";
      return;
    }

    // Draw bezier curve
    const curve = Math.min(distance / 2, 50);
    const d =
      port.type === "input"
        ? `M ${elemX} ${elemY} C ${elemX - curve} ${elemY} ${
            portX + curve
          } ${portY} ${portX} ${portY}`
        : `M ${elemX} ${elemY} C ${elemX + curve} ${elemY} ${
            portX - curve
          } ${portY} ${portX} ${portY}`;

    this.wiringPath.setAttribute("d", d);

    // Set class for animation direction and color based on port type
    // Input: data flows IN to element (animate towards element), green
    // Output: data flows OUT from element (animate towards port), blue
    if (port.type === "input") {
      this.wiringPath.setAttribute("class", "wiring-path-input");
      this.wiringPath.setAttribute("stroke", "var(--port-input)");
    } else {
      this.wiringPath.setAttribute("class", "wiring-path-output");
      this.wiringPath.setAttribute("stroke", "var(--port-output)");
    }

    this.wiringSvg.style.display = "block";
  }

  private hideHoverOverlay() {
    if (this.boundsOverlay) this.boundsOverlay.style.display = "none";
    if (this.infoPanel) this.infoPanel.style.display = "none";
    if (this.wiringSvg) this.wiringSvg.style.display = "none";
  }

  private escapeHtml(str: string): string {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  private async init() {
    // Window updates
    const render = () => this.render();
    this.axio.on("sync:init", render);
    this.axio.on("window:added", render);
    this.axio.on("window:removed", render);
    this.axio.on("window:changed", render);

    // Element updates for value propagation
    this.axio.on("element:changed", ({ element }) =>
      this.handleElementUpdate(element)
    );

    // Mouse tracking for clickthrough, drag connections, and port hover
    this.axio.on("mouse:position", ({ x, y }) => {
      // Update temp connection line if dragging
      if (this.connectingFrom && this.tempLine) {
        this.updateTempLine(x, y);
      }

      // Detect element under cursor
      const el = document.elementFromPoint(x, y);
      const overInteractive = el?.closest(".port, #menuBar");
      const portEl = el?.closest(".port") as HTMLElement | null;

      // Port hover detection
      if (portEl) {
        // Find which port this element belongs to
        const portId = [...this.portElements.entries()].find(
          ([, element]) => element === portEl
        )?.[0];
        const port = portId ? this.ports.get(portId) : null;

        if (port && port !== this.hoveredPort) {
          this.onPortHoverEnter(port);
        }
      } else if (this.hoveredPort) {
        this.onPortHoverLeave();
      }

      // Clickthrough logic:
      // - In creation mode: disabled (so we receive clicks, we enable briefly during elementAt)
      // - Not in creation mode: clickthrough unless over a port or menu
      if (this.creationMode) {
        // In creation mode, disable clickthrough so we receive clicks
        this.axio.setClickthrough(false);
      } else {
        // Otherwise, clickthrough unless over interactive elements
        this.axio.setClickthrough(!overInteractive);
      }
    });

    // Menu bar click - toggle creation mode
    this.menuBar.addEventListener("click", () => this.toggleCreationMode());

    // Global click - create ports when in creation mode
    document.addEventListener("click", (e) => this.onGlobalClick(e));

    // Escape to exit creation mode
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        if (this.creationMode) {
          this.toggleCreationMode();
        } else if (this.connectingFrom) {
          this.cancelConnection();
        }
      }
    });

    // Mouse for drag connections
    document.addEventListener("mouseup", (e) => this.onMouseUp(e));

    await this.axio.connect();
    this.updateMenuBar();
  }

  private toggleCreationMode() {
    this.creationMode = !this.creationMode;
    this.updateMenuBar();
    // In creation mode, clickthrough stays disabled so we receive clicks
    // We temporarily enable it only during elementAt calls
  }

  private updateMenuBar() {
    if (this.creationMode) {
      this.menuBar.classList.add("active");
      this.menuBar.innerHTML = `<span class="mode-indicator">●</span> Creating ports — click elements | <kbd>Esc</kbd> to exit`;
    } else {
      this.menuBar.classList.remove("active");
      this.menuBar.innerHTML = `Click to create ports | Drag output → input to connect | <kbd>Shift</kbd>+click to delete`;
    }
  }

  private async onGlobalClick(e: MouseEvent) {
    // Only handle in creation mode
    if (!this.creationMode) return;

    // Ignore clicks on the menu bar itself
    if ((e.target as Element)?.closest("#menuBar")) return;

    // Ignore clicks on ports (those are for connecting)
    if ((e.target as Element)?.closest(".port")) return;

    await this.createPortsAtPosition(e.clientX, e.clientY);
  }

  private async createPortsAtPosition(x: number, y: number) {
    const window = this.getWindowAt(x, y);
    if (!window) return;

    try {
      // elementAt now uses tracked windows (which exclude our overlay) so no clickthrough dance needed
      const element = await this.axio.elementAt(x, y);
      if (!element?.bounds) return;

      // Create both input and output ports for this element
      this.createPort(window.id, element, "input");
      this.createPort(window.id, element, "output");

      // Watch for value changes
      this.axio.watch(element.id);

      this.showFeedback(x, y);
    } catch (err) {
      console.error("Failed to get element:", err);
    }
  }

  /** Get all windows sorted by z-order (frontmost first) */
  private get windows(): AXWindow[] {
    return this.axio.depthOrder
      .map((id) => this.axio.windows.get(id))
      .filter((w): w is AXWindow => !!w);
  }

  private getWindowAt(x: number, y: number): AXWindow | null {
    // Iterate frontmost-first due to z-order sorting
    for (const w of this.windows) {
      const b = w.bounds;
      if (x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) {
        return w;
      }
    }
    return null;
  }

  private render() {
    const windows = this.windows;

    // Clean up closed windows
    const currentIds = new Set(windows.map((w) => w.id));
    for (const [id, container] of this.windowContainers) {
      if (!currentIds.has(id)) {
        container.remove();
        this.windowContainers.delete(id);
        this.edgeGroups.delete(id);
        // Remove ports for this window
        for (const [portId, port] of this.ports) {
          if (port.windowId === id) this.deletePort(portId);
        }
      }
    }

    // Update/create window containers
    for (const window of windows) {
      this.updateWindowContainer(window);
    }

    this.updatePortPositions();
    this.redrawConnections();
  }

  private updateWindowContainer(window: AXWindow) {
    let container = this.windowContainers.get(window.id);

    if (!container) {
      container = document.createElement("div");
      container.className = "window-container";

      const left = document.createElement("div");
      left.className = "edge-group left";

      const right = document.createElement("div");
      right.className = "edge-group right";

      container.appendChild(left);
      container.appendChild(right);
      this.container.appendChild(container);

      this.windowContainers.set(window.id, container);
      this.edgeGroups.set(window.id, { left, right });
    }

    const { x, y, w, h } = window.bounds;
    const zIndex = this.occlusion.getZIndex(window.id);
    const clipPath = this.occlusion.getClipPath(window.id);

    Object.assign(container.style, {
      left: `${x}px`,
      top: `${y}px`,
      width: `${w}px`,
      height: `${h}px`,
      zIndex: zIndex.toString(),
      clipPath: clipPath,
    });
  }

  private createPort(windowId: string, element: AXElement, type: PortType) {
    // Check if already exists
    const exists = [...this.ports.values()].some(
      (p) => p.element.id === element.id && p.type === type
    );
    if (exists) return;

    const window = this.windows.find((w) => w.id === windowId);
    if (!window) return;

    const port: Port = {
      id: `port-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`,
      windowId,
      element,
      type,
      x: type === "input" ? window.bounds.x : window.bounds.x + window.bounds.w,
      y: window.bounds.y + window.bounds.h / 2,
    };

    this.ports.set(port.id, port);
    this.createPortElement(port);
    this.updatePortPositions();
  }

  private createPortElement(port: Port) {
    const el = document.createElement("div");
    el.className = `port ${port.type}`;
    const displayText =
      port.element.label ||
      (port.element.value ? String(port.element.value.value) : null) ||
      "(no label)";
    el.title = `${port.element.role}: ${displayText}`;

    el.addEventListener("click", (e) => {
      e.stopPropagation();
      if (e.shiftKey) {
        this.deletePort(port.id);
      } else if (port.type === "input" && this.connectingFrom) {
        this.completeConnection(port);
      }
    });

    el.addEventListener("mousedown", (e) => {
      if (e.shiftKey) return;
      e.stopPropagation();
      if (port.type === "output") {
        this.startConnection(port);
      }
    });

    const edges = this.edgeGroups.get(port.windowId);
    if (edges) {
      (port.type === "input" ? edges.left : edges.right).appendChild(el);
    }

    this.portElements.set(port.id, el);
  }

  private deletePort(portId: string) {
    const port = this.ports.get(portId);
    if (!port) return;

    // Clear hover if this port is hovered
    if (this.hoveredPort?.id === portId) {
      this.onPortHoverLeave();
    }

    this.portElements.get(portId)?.remove();
    this.portElements.delete(portId);
    this.ports.delete(portId);

    // Remove connections
    this.connections = this.connections.filter(
      (c) => c.sourcePort.id !== portId && c.targetPort.id !== portId
    );

    this.axio.unwatch(port.element.id).catch(() => {});
    this.redrawConnections();
  }

  private startConnection(port: Port) {
    this.connectingFrom = port;
    this.portElements.get(port.id)?.classList.add("connecting");

    this.tempLine = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "path"
    );
    this.tempLine.classList.add("temp-connection");

    // Apply absolute clip-path for SVG element
    const clipPath = this.occlusion.getAbsoluteClipPath(port.windowId);
    if (clipPath) {
      this.tempLine.style.clipPath = clipPath;
    }

    this.svg.appendChild(this.tempLine);
  }

  private completeConnection(targetPort: Port) {
    if (!this.connectingFrom) return;

    // Don't connect same element or existing connection
    const exists = this.connections.some(
      (c) =>
        c.sourcePort.id === this.connectingFrom!.id &&
        c.targetPort.id === targetPort.id
    );
    if (!exists && this.connectingFrom.element.id !== targetPort.element.id) {
      const connection: Connection = {
        id: `conn-${Date.now()}`,
        sourcePort: this.connectingFrom,
        targetPort,
      };
      this.connections.push(connection);
      this.propagateValue(connection);
    }

    this.cancelConnection();
  }

  private cancelConnection() {
    if (this.connectingFrom) {
      this.portElements
        .get(this.connectingFrom.id)
        ?.classList.remove("connecting");
    }
    this.tempLine?.remove();
    this.tempLine = null;
    this.connectingFrom = null;
    this.redrawConnections();
  }

  private onMouseUp(e: MouseEvent) {
    if (!this.connectingFrom) return;

    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (el?.classList.contains("port")) {
      const portId = [...this.portElements.entries()].find(
        ([_, v]) => v === el
      )?.[0];
      if (portId) {
        const port = this.ports.get(portId);
        if (port?.type === "input") {
          this.completeConnection(port);
          return;
        }
      }
    }
    this.cancelConnection();
  }

  private updateTempLine(x: number, y: number) {
    if (!this.tempLine || !this.connectingFrom) return;
    this.tempLine.setAttribute(
      "d",
      this.makePath(this.connectingFrom.x, this.connectingFrom.y, x, y)
    );
  }

  private updatePortPositions() {
    for (const port of this.ports.values()) {
      const window = this.windows.find((w) => w.id === port.windowId);
      if (!window) continue;

      // Get ports on same edge, sorted by DOM order
      const portsOnEdge = [...this.ports.values()].filter(
        (p) => p.windowId === port.windowId && p.type === port.type
      );

      const idx = portsOnEdge.indexOf(port);
      const spacing = 26; // port height (20px) + gap (6px)
      const totalHeight = portsOnEdge.length * spacing;
      const startY =
        window.bounds.y + (window.bounds.h - totalHeight) / 2 + spacing / 2;

      port.x =
        port.type === "input"
          ? window.bounds.x
          : window.bounds.x + window.bounds.w;
      port.y = startY + idx * spacing;
    }
  }

  private redrawConnections() {
    this.svg
      .querySelectorAll(".connection-line, .connection-arrow")
      .forEach((el) => el.remove());

    for (const conn of this.connections) {
      const path = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "path"
      );
      path.classList.add("connection-line");
      path.setAttribute(
        "d",
        this.makePath(
          conn.sourcePort.x,
          conn.sourcePort.y,
          conn.targetPort.x,
          conn.targetPort.y
        )
      );

      // Clip connection by windows in front of BOTH endpoints
      // Use the backmost window's clip-path (more restrictive)
      const sourceZ = this.occlusion.getZIndex(conn.sourcePort.windowId);
      const targetZ = this.occlusion.getZIndex(conn.targetPort.windowId);
      const backmostWindowId =
        sourceZ < targetZ ? conn.sourcePort.windowId : conn.targetPort.windowId;
      const clipPath = this.occlusion.getAbsoluteClipPath(backmostWindowId);
      if (clipPath) {
        path.style.clipPath = clipPath;
      }

      this.svg.appendChild(path);
    }
  }

  private makePath(x1: number, y1: number, x2: number, y2: number): string {
    const dx = x2 - x1;
    const curve = Math.min(Math.abs(dx) / 2, 80);
    return `M ${x1},${y1} C ${x1 + curve},${y1} ${
      x2 - curve
    },${y2} ${x2},${y2}`;
  }

  private handleElementUpdate(element: AXElement) {
    // Update port element reference and propagate
    for (const port of this.ports.values()) {
      if (port.element.id === element.id) {
        port.element = element;
        if (port.type === "output") {
          for (const conn of this.connections) {
            if (conn.sourcePort.id === port.id) {
              this.propagateValue(conn);
            }
          }
        }
      }
    }
  }

  private async propagateValue(conn: Connection) {
    const value = conn.sourcePort.element.value;
    if (!value) return;

    try {
      await this.axio.write(conn.targetPort.element.id, String(value.value));
    } catch (err) {
      console.error("Failed to propagate:", err);
    }
  }

  private showFeedback(x: number, y: number) {
    const el = document.createElement("div");
    el.className = "feedback";
    el.style.left = `${x - 15}px`;
    el.style.top = `${y - 15}px`;
    this.container.appendChild(el);
    setTimeout(() => el.remove(), 400);
  }
}

document.addEventListener("DOMContentLoaded", () => new PortsDemo());
