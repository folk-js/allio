import { AXIO, AXElement, AXWindow } from "@axio/client";

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

  private ports = new Map<string, Port>();
  private connections: Connection[] = [];
  private portElements = new Map<string, HTMLElement>();
  private windowContainers = new Map<string, HTMLElement>();
  private edgeGroups = new Map<
    string,
    { left: HTMLElement; right: HTMLElement }
  >();

  // SVG defs for masks
  private maskDefs: SVGDefsElement;

  // Debug mode - visualize masks
  private debugMasks = false; // Set to false to disable
  private debugOverlays = new Map<string, HTMLElement>();

  // Mode state
  private creationMode = false;

  // Drag connection state
  private connectingFrom: Port | null = null;
  private tempLine: SVGPathElement | null = null;

  constructor() {
    this.container = document.getElementById("portContainer")!;
    this.svg = document.getElementById("connections") as unknown as SVGElement;
    this.menuBar = document.getElementById("menuBar")!;
    this.axio = new AXIO();

    // Create SVG defs for masks
    this.maskDefs = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "defs"
    );
    this.svg.appendChild(this.maskDefs);

    this.init();
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

    // Mouse tracking for clickthrough and drag connections
    this.axio.on("mouse:position", ({ x, y }) => {
      // Update temp connection line if dragging
      if (this.connectingFrom && this.tempLine) {
        this.updateTempLine(x, y);
      }

      // Clickthrough logic:
      // - In creation mode: disabled (so we receive clicks, we enable briefly during elementAt)
      // - Not in creation mode: clickthrough unless over a port or menu
      const el = document.elementFromPoint(x, y);
      const overInteractive = el?.closest(".port, #menuBar");

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

  /** Get z-index for a window (higher = more in front) */
  private getWindowZIndex(windowId: string): number {
    const index = this.axio.depthOrder.indexOf(windowId);
    if (index === -1) return 0;
    // Front windows get higher z-index
    // Reserve 0-999 for windows, 1000+ for menu bar
    return 1000 - index;
  }

  /** Generate a clip-path that hides regions occluded by windows in front */
  private generateWindowMask(window: AXWindow): string {
    const windows = this.windows;
    const targetIndex = windows.findIndex((w) => w.id === window.id);
    if (targetIndex === -1) return "";

    // Get ALL windows in front (earlier in depthOrder = in front)
    const windowsInFront = windows.slice(0, targetIndex);

    if (windowsInFront.length === 0) {
      return ""; // No clip needed - this is the frontmost window
    }

    // Convert to window-relative rectangles
    const rects = windowsInFront.map((fw) => ({
      x: fw.bounds.x - window.bounds.x,
      y: fw.bounds.y - window.bounds.y,
      w: fw.bounds.w,
      h: fw.bounds.h,
    }));

    // Compute union of rectangles to avoid overlap issues with fill rules
    const unionRects = this.computeRectUnion(rects);

    // Build path: outer boundary + union holes
    const paths: string[] = [];

    // Outer boundary (clockwise)
    paths.push("M -5000 -5000 L 5000 -5000 L 5000 5000 L -5000 5000 Z");

    // Each union rect as counter-clockwise hole
    for (const rect of unionRects) {
      const { x, y, w, h } = rect;
      paths.push(
        `M ${x} ${y} L ${x} ${y + h} L ${x + w} ${y + h} L ${x + w} ${y} Z`
      );
    }

    // Debug: visualize the mask
    if (this.debugMasks) {
      this.createDebugOverlay(window, windowsInFront);
    }

    return `path(evenodd, "${paths.join(" ")}")`;
  }

  /** Compute union of axis-aligned rectangles, returning non-overlapping rects */
  private computeRectUnion(
    rects: Array<{ x: number; y: number; w: number; h: number }>
  ): Array<{ x: number; y: number; w: number; h: number }> {
    if (rects.length <= 1) return rects;

    // Simple approach: sweep line algorithm
    // Collect all unique x and y coordinates
    const xs = new Set<number>();
    const ys = new Set<number>();

    for (const r of rects) {
      xs.add(r.x);
      xs.add(r.x + r.w);
      ys.add(r.y);
      ys.add(r.y + r.h);
    }

    const sortedXs = Array.from(xs).sort((a, b) => a - b);
    const sortedYs = Array.from(ys).sort((a, b) => a - b);

    // Create grid cells and mark which are covered
    const grid: boolean[][] = [];
    for (let i = 0; i < sortedYs.length - 1; i++) {
      grid[i] = [];
      for (let j = 0; j < sortedXs.length - 1; j++) {
        grid[i][j] = false;
      }
    }

    // Mark cells covered by any rect
    for (const r of rects) {
      const x1Idx = sortedXs.indexOf(r.x);
      const x2Idx = sortedXs.indexOf(r.x + r.w);
      const y1Idx = sortedYs.indexOf(r.y);
      const y2Idx = sortedYs.indexOf(r.y + r.h);

      for (let i = y1Idx; i < y2Idx; i++) {
        for (let j = x1Idx; j < x2Idx; j++) {
          grid[i][j] = true;
        }
      }
    }

    // Extract non-overlapping rectangles from marked cells
    // Simple greedy: for each marked cell, extend as far right and down as possible
    const result: Array<{ x: number; y: number; w: number; h: number }> = [];

    for (let i = 0; i < grid.length; i++) {
      for (let j = 0; j < grid[i].length; j++) {
        if (grid[i][j]) {
          // Find max width
          let maxJ = j;
          while (maxJ < grid[i].length && grid[i][maxJ]) maxJ++;

          // Find max height with this width
          let maxI = i;
          outer: while (maxI < grid.length) {
            for (let k = j; k < maxJ; k++) {
              if (!grid[maxI][k]) break outer;
            }
            maxI++;
          }

          // Mark cells as used
          for (let ii = i; ii < maxI; ii++) {
            for (let jj = j; jj < maxJ; jj++) {
              grid[ii][jj] = false;
            }
          }

          // Add rectangle
          result.push({
            x: sortedXs[j],
            y: sortedYs[i],
            w: sortedXs[maxJ] - sortedXs[j],
            h: sortedYs[maxI] - sortedYs[i],
          });
        }
      }
    }

    return result;
  }

  /** Debug helper: create visual overlay showing what's being masked */
  private createDebugOverlay(window: AXWindow, windowsInFront: AXWindow[]) {
    const debugId = `debug-${window.id}`;

    // Remove old overlay
    const old = this.debugOverlays.get(debugId);
    if (old) old.remove();

    const overlay = document.createElement("div");
    overlay.style.position = "absolute";
    overlay.style.pointerEvents = "none";
    overlay.style.border = "2px solid cyan";
    overlay.style.left = `${window.bounds.x}px`;
    overlay.style.top = `${window.bounds.y}px`;
    overlay.style.width = `${window.bounds.w}px`;
    overlay.style.height = `${window.bounds.h}px`;
    overlay.style.zIndex = "10000";

    // Add red rectangles for occluding windows
    for (const frontWindow of windowsInFront) {
      const occluder = document.createElement("div");
      occluder.style.position = "absolute";
      occluder.style.left = `${frontWindow.bounds.x - window.bounds.x}px`;
      occluder.style.top = `${frontWindow.bounds.y - window.bounds.y}px`;
      occluder.style.width = `${frontWindow.bounds.w}px`;
      occluder.style.height = `${frontWindow.bounds.h}px`;
      occluder.style.background = "rgba(255, 0, 0, 0.3)";
      occluder.style.border = "1px solid red";
      overlay.appendChild(occluder);
    }

    this.container.appendChild(overlay);
    this.debugOverlays.set(debugId, overlay);
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

    // Clean up debug overlays
    if (this.debugMasks) {
      for (const overlay of this.debugOverlays.values()) {
        overlay.remove();
      }
      this.debugOverlays.clear();
    }

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
    const zIndex = this.getWindowZIndex(window.id);
    const clipPath = this.generateWindowMask(window);

    Object.assign(container.style, {
      left: `${x}px`,
      top: `${y}px`,
      width: `${w}px`,
      height: `${h}px`,
      zIndex: zIndex.toString(),
    });

    // Apply clip-path if needed
    if (clipPath) {
      container.style.clipPath = clipPath;
    } else {
      container.style.clipPath = "";
    }
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
      const spacing = 34; // port size + gap
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
      this.svg.appendChild(path);

      // Arrowhead
      this.drawArrow(
        conn.targetPort.x,
        conn.targetPort.y,
        conn.sourcePort.x,
        conn.sourcePort.y
      );
    }
  }

  private makePath(x1: number, y1: number, x2: number, y2: number): string {
    const dx = x2 - x1;
    const curve = Math.min(Math.abs(dx) / 2, 80);
    return `M ${x1},${y1} C ${x1 + curve},${y1} ${
      x2 - curve
    },${y2} ${x2},${y2}`;
  }

  private drawArrow(x: number, y: number, fromX: number, fromY: number) {
    const angle = Math.atan2(y - fromY, x - fromX);
    const size = 8;
    const points = [
      `${x},${y}`,
      `${x - size * Math.cos(angle - Math.PI / 6)},${
        y - size * Math.sin(angle - Math.PI / 6)
      }`,
      `${x - size * Math.cos(angle + Math.PI / 6)},${
        y - size * Math.sin(angle + Math.PI / 6)
      }`,
    ].join(" ");

    const polygon = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "polygon"
    );
    polygon.classList.add("connection-arrow");
    polygon.setAttribute("points", points);
    this.svg.appendChild(polygon);
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
