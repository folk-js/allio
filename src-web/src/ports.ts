import { AXIO, AXNode, AXWindow } from "@axio/client";

/**
 * Port Types
 */
type PortType = "input" | "output";

interface Port {
  id: string;
  windowId: string;
  element: AXNode;
  type: PortType;
  x: number; // Screen position of the port circle
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
 * Features:
 * - Toolbar on each window for creating ports
 * - Click elements to create input/output ports
 * - Visual port representation (circles on window edges)
 * - Draw arrows between ports
 * - Propagate values between connected elements
 */
class PortsDemo {
  private portContainer: HTMLElement;
  private connectionsContainer: SVGElement;
  private clickCaptureOverlay: HTMLElement;
  private axio: AXIO;

  // State
  private windows: AXWindow[] = [];
  private ports: Map<string, Port> = new Map(); // portId -> Port
  private connections: Connection[] = [];
  private windowContainers: Map<string, HTMLElement> = new Map(); // windowId -> container
  private edgeGroups: Map<string, { left: HTMLElement; right: HTMLElement }> =
    new Map(); // windowId -> edge groups
  private toolbars: Map<
    string,
    { input: HTMLButtonElement | null; output: HTMLButtonElement | null }
  > = new Map(); // windowId -> buttons
  private portElements: Map<string, HTMLElement> = new Map(); // portId -> DOM element

  // Port creation mode
  private portCreationMode: PortType | null = null;
  private activeWindowId: string | null = null;
  private elementHighlight: HTMLElement | null = null;
  private isClickthroughEnabled: boolean = true;

  // Connection drawing
  private connectingFromPort: Port | null = null;
  private tempConnectionLine: SVGPathElement | null = null;

  constructor() {
    this.portContainer = document.getElementById("portContainer")!;
    this.connectionsContainer = document.getElementById(
      "connections"
    ) as unknown as SVGElement;
    this.clickCaptureOverlay = document.getElementById("clickCaptureOverlay")!;
    this.axio = new AXIO();

    this.setupWebSocket();
    this.setupMouseTracking();
    this.setupConnectionDrawing();
    this.setupClickCapture();
  }

  private async setupWebSocket() {
    try {
      this.axio.on("windows", (windows) => this.updateWindows(windows));
      this.axio.on("update", (update) => this.handleElementUpdate(update));

      await this.axio.connect();
      console.log("📡 Ports Demo connected");
    } catch (error) {
      console.error("❌ Failed to connect:", error);
    }
  }

  private setupMouseTracking() {
    // Listen for global mouse position from backend
    this.axio.on("mouse", ({ x, y }) => {
      // Update temp connection line if drawing
      if (this.connectingFromPort && this.tempConnectionLine) {
        this.updateTempConnection(x, y);
      }

      // Update click capture overlay gradient position
      if (this.portCreationMode) {
        this.clickCaptureOverlay.style.setProperty("--mouse-x", `${x}px`);
        this.clickCaptureOverlay.style.setProperty("--mouse-y", `${y}px`);
      }

      // Check what element is at this position for clickthrough
      const elementUnderCursor = document.elementFromPoint(x, y);
      const isOverInteractive = this.isElementInteractive(elementUnderCursor);

      // Enable clickthrough when NOT over interactive elements
      // Port creation mode no longer affects clickthrough - overlay stays transparent
      const shouldEnableClickthrough = !isOverInteractive;

      // Only update if state changed
      if (shouldEnableClickthrough !== this.isClickthroughEnabled) {
        this.isClickthroughEnabled = shouldEnableClickthrough;
        this.axio.setClickthrough(shouldEnableClickthrough).catch((err) => {
          console.error("Failed to set clickthrough:", err);
        });
      }
    });
  }

  private isElementInteractive(element: Element | null): boolean {
    if (!element) return false;

    let current: Element | null = element;
    while (current) {
      // Check if it's a toolbar, port, add button, or any button
      if (
        current.classList.contains("port-toolbar") ||
        current.classList.contains("port") ||
        current.classList.contains("port-add-button") ||
        current.tagName === "BUTTON"
      ) {
        return true;
      }
      current = current.parentElement;
    }
    return false;
  }

  private updateWindows(windows: AXWindow[]) {
    this.windows = windows;

    // Detect closed windows and clean up
    const existingWindowIds = new Set(windows.map((w) => w.id));
    const closedWindows = new Set<string>();

    for (const [windowId] of this.windowContainers.entries()) {
      if (!existingWindowIds.has(windowId)) {
        closedWindows.add(windowId);
        // Remove window container (this removes all child elements)
        const container = this.windowContainers.get(windowId);
        if (container) container.remove();
        this.windowContainers.delete(windowId);
        this.edgeGroups.delete(windowId);
        this.toolbars.delete(windowId);
      }
    }

    // Clean up ports and connections for closed windows
    if (closedWindows.size > 0) {
      this.cleanupClosedWindows(closedWindows);
    }

    // Update toolbars for each window
    for (const window of windows) {
      this.updateToolbar(window);
    }

    // Update port positions (in case windows moved)
    this.updatePortPositions();

    // Redraw connections
    this.redrawConnections();
  }

  private cleanupClosedWindows(closedWindowIds: Set<string>) {
    // Find all ports belonging to closed windows
    const portsToRemove: string[] = [];
    for (const [portId, port] of this.ports.entries()) {
      if (closedWindowIds.has(port.windowId)) {
        portsToRemove.push(portId);
      }
    }

    // Remove these ports
    for (const portId of portsToRemove) {
      this.deletePort(portId);
    }

    console.log(
      `🧹 Cleaned up ${portsToRemove.length} ports from ${closedWindowIds.size} closed window(s)`
    );
  }

  private updateToolbar(window: AXWindow) {
    // Check if we already have a container for this window
    let windowContainer = this.windowContainers.get(window.id);
    let edgeGroups = this.edgeGroups.get(window.id);

    if (!windowContainer) {
      // Create window container
      windowContainer = document.createElement("div");
      windowContainer.className = "window-container";
      windowContainer.setAttribute("data-window-id", window.id);

      // Create edge groups
      const leftEdge = document.createElement("div");
      leftEdge.className = "edge-group left";
      leftEdge.setAttribute("data-edge", "left");

      const rightEdge = document.createElement("div");
      rightEdge.className = "edge-group right";
      rightEdge.setAttribute("data-edge", "right");

      // Create "+" button for input ports (left edge)
      const inputBtn = document.createElement("button");
      inputBtn.className = "port-add-button input";
      inputBtn.textContent = "+";
      inputBtn.title = "Create input port (hover + press C)";
      inputBtn.setAttribute("data-window-id", window.id);
      inputBtn.addEventListener("click", () => {
        this.togglePortCreationMode(window.id, "input", inputBtn);
      });

      // Create "+" button for output ports (right edge)
      const outputBtn = document.createElement("button");
      outputBtn.className = "port-add-button output";
      outputBtn.textContent = "+";
      outputBtn.title = "Create output port (hover + press C)";
      outputBtn.setAttribute("data-window-id", window.id);
      outputBtn.addEventListener("click", () => {
        this.togglePortCreationMode(window.id, "output", outputBtn);
      });

      // Add buttons to edge groups (they'll be first in the flex column)
      leftEdge.appendChild(inputBtn);
      rightEdge.appendChild(outputBtn);

      // Add edge groups to window container
      windowContainer.appendChild(leftEdge);
      windowContainer.appendChild(rightEdge);

      // Add container to port container
      this.portContainer.appendChild(windowContainer);

      // Store references
      this.windowContainers.set(window.id, windowContainer);
      edgeGroups = { left: leftEdge, right: rightEdge };
      this.edgeGroups.set(window.id, edgeGroups);
      this.toolbars.set(window.id, { input: inputBtn, output: outputBtn });
    }

    // Update window container position and size
    windowContainer.style.left = `${window.x}px`;
    windowContainer.style.top = `${window.y}px`;
    windowContainer.style.width = `${window.w}px`;
    windowContainer.style.height = `${window.h}px`;
  }

  private togglePortCreationMode(
    windowId: string,
    type: PortType,
    button: HTMLButtonElement
  ) {
    const toolbar = this.toolbars.get(windowId);
    if (!toolbar) return;

    // Toggle mode
    if (this.portCreationMode === type && this.activeWindowId === windowId) {
      // Disable mode
      this.portCreationMode = null;
      this.activeWindowId = null;
      button.style.transform = "scale(1)";
      button.style.opacity = "1";
      this.hideElementHighlight();

      // Hide click capture overlay
      this.clickCaptureOverlay.classList.remove(
        "active",
        "input-mode",
        "output-mode"
      );

      console.log(`❌ Disabled port creation mode`);
    } else {
      // Enable mode
      this.portCreationMode = type;
      this.activeWindowId = windowId;

      // Update button states in ALL toolbars (allow only one mode at a time)
      for (const [_, buttons] of this.toolbars) {
        if (buttons.input) {
          buttons.input.style.transform = "scale(1)";
          buttons.input.style.opacity = "1";
        }
        if (buttons.output) {
          buttons.output.style.transform = "scale(1)";
          buttons.output.style.opacity = "1";
        }
      }

      // Highlight the active button
      button.style.transform = "scale(1.3)";
      button.style.opacity = "1";

      // Show click capture overlay with appropriate styling (visual only - pointer-events: none)
      this.clickCaptureOverlay.classList.add("active", `${type}-mode`);

      console.log(
        `🎯 Enabled ${type} port creation mode for window ${windowId}`
      );
    }
  }

  private setupClickCapture() {
    // Since overlay has pointer-events: none, we need to detect clicks differently
    // We'll use keyboard shortcuts: press 'c' while hovering over an element to create a port

    let lastMouseX = 0;
    let lastMouseY = 0;

    // Track mouse position
    this.axio.on("mouse", ({ x, y }) => {
      lastMouseX = x;
      lastMouseY = y;
    });

    // Listen for keyboard shortcut
    document.addEventListener("keydown", async (e) => {
      // Only handle if in port creation mode
      if (!this.portCreationMode || !this.activeWindowId) {
        return;
      }

      // Press 'c' to create port at current mouse position
      if (e.key === "c" || e.key === "C") {
        console.log(`📍 Creating port at (${lastMouseX}, ${lastMouseY})`);

        // Determine which window the cursor is currently over
        const targetWindow = this.getWindowAtPosition(lastMouseX, lastMouseY);
        if (!targetWindow) {
          console.log("⚠️  Cursor is not over any window");
          e.preventDefault();
          e.stopPropagation();
          return;
        }

        // Get element at mouse position
        try {
          const element = await this.axio.elementAt(lastMouseX, lastMouseY);

          if (element) {
            console.log(
              `🔌 Creating ${this.portCreationMode} port for`,
              element,
              `on window ${targetWindow.id}`
            );
            this.createPort(targetWindow.id, element, this.portCreationMode);

            // Show brief success feedback
            this.showPortCreationFeedback(lastMouseX, lastMouseY);
          } else {
            console.log("⚠️  No element found at mouse position");
          }
        } catch (error) {
          console.error("❌ Failed to get element at position:", error);
        }

        e.preventDefault();
        e.stopPropagation();
      }
    });
  }

  private getWindowAtPosition(x: number, y: number): AXWindow | null {
    // Check which window contains the given point
    for (const window of this.windows) {
      if (
        x >= window.x &&
        x <= window.x + window.w &&
        y >= window.y &&
        y <= window.y + window.h
      ) {
        return window;
      }
    }
    return null;
  }

  private createPort(windowId: string, element: AXNode, type: PortType) {
    // Check if port of this type already exists for this element
    const existingPort = Array.from(this.ports.values()).find(
      (p) => p.element.id === element.id && p.type === type
    );
    if (existingPort) {
      console.log(`⚠️  ${type} port already exists for this element`);
      return;
    }

    // Calculate port position on window edge (for connections)
    const window = this.windows.find((w) => w.id === windowId);
    if (!window || !element.bounds) {
      console.error(
        "Cannot create port: window or element bounds not available"
      );
      return;
    }

    const portPosition = this.calculatePortPosition(window, type);

    // Create port object
    const port: Port = {
      id: `port-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      windowId,
      element,
      type,
      x: portPosition.x,
      y: portPosition.y,
    };

    this.ports.set(port.id, port);

    // Create visual representation (will be added to appropriate edge group)
    this.createPortElement(port);

    // Watch the element for value changes
    if (element.id) {
      this.axio.watch(element.id).catch((err) => {
        console.error("Failed to watch port element:", err);
      });
    }

    console.log(`✅ Created ${type} port:`, port);
  }

  private calculatePortPosition(
    window: AXWindow,
    type: PortType
  ): { x: number; y: number } {
    // Calculate where the port will be positioned in screen coordinates
    // This is used for drawing connections, while CSS handles visual positioning

    // Get all existing ports for this window on the same edge
    const existingPortsOnEdge = Array.from(this.ports.values()).filter(
      (p) => p.windowId === window.id && p.type === type
    );

    const portSpacing = 10; // from CSS gap
    const portSize = 24; // port diameter
    const addButtonSize = 24; // add button size (same as port)

    // Calculate total height (add button + all ports including this new one)
    const totalItemCount = existingPortsOnEdge.length + 1 + 1; // existing ports + new port + add button
    const totalHeight =
      totalItemCount * portSize + (totalItemCount - 1) * portSpacing;

    // Starting Y position (centered vertically on window)
    const startY = window.y + (window.h - totalHeight) / 2;

    // Y position for this port (add button first, then each port)
    const portIndex = existingPortsOnEdge.length;
    const portY =
      startY +
      addButtonSize +
      portSpacing +
      portIndex * (portSize + portSpacing) +
      portSize / 2; // Center of port

    let x: number;
    if (type === "input") {
      // Left edge - center of port circle
      x = window.x;
    } else {
      // Right edge - center of port circle
      x = window.x + window.w;
    }

    return { x, y: portY };
  }

  private createPortElement(port: Port) {
    const portEl = document.createElement("div");
    portEl.className = `port ${port.type}`;
    portEl.setAttribute("data-port-id", port.id);
    portEl.setAttribute("draggable", "false");

    // Click handler: Shift+Click to delete, regular click for legacy connection mode
    portEl.addEventListener("click", (e) => {
      e.stopPropagation();

      if (e.shiftKey) {
        // Shift+Click to delete
        this.deletePort(port.id);
      } else {
        // Regular click for connection (legacy mode)
        this.handlePortClick(port);
      }
    });

    // Drag handler for creating connections
    portEl.addEventListener("mousedown", (e) => {
      if (e.shiftKey) return; // Don't start drag when deleting
      e.stopPropagation();

      if (port.type === "output") {
        this.startDragConnection(port, e);
      }
    });

    // Hover handler for tooltip
    portEl.addEventListener("mouseenter", () => {
      this.showPortTooltip(port, portEl);
    });

    portEl.addEventListener("mouseleave", () => {
      this.hidePortTooltip();
    });

    // Add to appropriate edge group
    const edgeGroups = this.edgeGroups.get(port.windowId);
    if (edgeGroups) {
      const targetEdge =
        port.type === "input" ? edgeGroups.left : edgeGroups.right;
      targetEdge.appendChild(portEl);
    }

    this.portElements.set(port.id, portEl);
  }

  private deletePort(portId: string) {
    const port = this.ports.get(portId);
    if (!port) return;

    console.log(`🗑️  Deleting port: ${portId}`);

    // Remove port element from DOM (automatically removes from edge group)
    const portEl = this.portElements.get(portId);
    if (portEl) {
      portEl.remove();
      this.portElements.delete(portId);
    }

    // Remove all connections involving this port
    this.connections = this.connections.filter(
      (c) => c.sourcePort.id !== portId && c.targetPort.id !== portId
    );

    // Unwatch the element
    if (port.element.id) {
      this.axio.unwatch(port.element.id).catch((err) => {
        console.error("Failed to unwatch element:", err);
      });
    }

    // Remove from ports map
    this.ports.delete(portId);

    // Redraw connections
    this.redrawConnections();

    // Update port positions (for connection drawing) - CSS handles visual layout
    this.updatePortPositions();

    console.log(`✅ Port deleted`);
  }

  private handlePortClick(port: Port) {
    if (!this.connectingFromPort) {
      // Start connection
      if (port.type === "output") {
        this.connectingFromPort = port;
        const portEl = this.portElements.get(port.id);
        if (portEl) {
          portEl.classList.add("connecting");
        }
        console.log("🔗 Started connection from output port:", port);

        // Create temp connection line
        this.tempConnectionLine = document.createElementNS(
          "http://www.w3.org/2000/svg",
          "path"
        );
        this.tempConnectionLine.classList.add("temp-connection");
        this.connectionsContainer.appendChild(this.tempConnectionLine);
      } else {
        console.log("⚠️  Connections must start from output ports");
      }
    } else {
      // Complete connection
      if (port.type === "input" && port.id !== this.connectingFromPort.id) {
        this.createConnection(this.connectingFromPort, port);
      } else {
        console.log("⚠️  Connections must end at input ports");
      }

      // Clean up
      const portEl = this.portElements.get(this.connectingFromPort.id);
      if (portEl) {
        portEl.classList.remove("connecting");
      }
      if (this.tempConnectionLine) {
        this.tempConnectionLine.remove();
        this.tempConnectionLine = null;
      }
      this.connectingFromPort = null;
    }
  }

  private updateTempConnection(mouseX: number, mouseY: number) {
    if (!this.tempConnectionLine || !this.connectingFromPort) return;

    const path = this.createConnectionPath(
      this.connectingFromPort.x,
      this.connectingFromPort.y,
      mouseX,
      mouseY
    );
    this.tempConnectionLine.setAttribute("d", path);
  }

  private createConnection(sourcePort: Port, targetPort: Port) {
    // Check if connection already exists
    const existingConnection = this.connections.find(
      (c) =>
        c.sourcePort.id === sourcePort.id && c.targetPort.id === targetPort.id
    );
    if (existingConnection) {
      console.log("⚠️  Connection already exists");
      return;
    }

    const connection: Connection = {
      id: `conn-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      sourcePort,
      targetPort,
    };

    this.connections.push(connection);
    console.log("✅ Created connection:", connection);

    // Redraw connections
    this.redrawConnections();

    // Propagate initial value
    this.propagateValue(connection);
  }

  private redrawConnections() {
    // Clear existing connection lines (except temp)
    const existingLines = this.connectionsContainer.querySelectorAll(
      ".connection-line, .connection-arrow"
    );
    existingLines.forEach((line) => line.remove());

    // Draw all connections
    for (const connection of this.connections) {
      this.drawConnection(connection);
    }
  }

  private drawConnection(connection: Connection) {
    const { sourcePort, targetPort } = connection;

    // Create path
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.classList.add("connection-line");
    path.setAttribute(
      "d",
      this.createConnectionPath(
        sourcePort.x,
        sourcePort.y,
        targetPort.x,
        targetPort.y
      )
    );
    this.connectionsContainer.appendChild(path);

    // Create arrowhead at target
    this.drawArrowhead(targetPort.x, targetPort.y, sourcePort.x, sourcePort.y);
  }

  private createConnectionPath(
    x1: number,
    y1: number,
    x2: number,
    y2: number
  ): string {
    // Create a smooth curved path
    const dx = x2 - x1;
    const dy = y2 - y1;
    const dist = Math.sqrt(dx * dx + dy * dy);
    const curvature = Math.min(dist / 3, 100);

    // Control points for cubic bezier
    const cx1 = x1 + (dx / dist) * curvature;
    const cy1 = y1 + (dy / dist) * curvature;
    const cx2 = x2 - (dx / dist) * curvature;
    const cy2 = y2 - (dy / dist) * curvature;

    return `M ${x1},${y1} C ${cx1},${cy1} ${cx2},${cy2} ${x2},${y2}`;
  }

  private drawArrowhead(x: number, y: number, fromX: number, fromY: number) {
    const angle = Math.atan2(y - fromY, x - fromX);
    const arrowSize = 8;

    const points = [
      { x: x, y: y },
      {
        x: x - arrowSize * Math.cos(angle - Math.PI / 6),
        y: y - arrowSize * Math.sin(angle - Math.PI / 6),
      },
      {
        x: x - arrowSize * Math.cos(angle + Math.PI / 6),
        y: y - arrowSize * Math.sin(angle + Math.PI / 6),
      },
    ];

    const polygon = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "polygon"
    );
    polygon.classList.add("connection-arrow");
    polygon.setAttribute(
      "points",
      points.map((p) => `${p.x},${p.y}`).join(" ")
    );
    this.connectionsContainer.appendChild(polygon);
  }

  private updatePortPositions() {
    // Recalculate stored x/y coordinates for connection drawing
    // Visual positioning is handled by CSS flexbox

    for (const port of this.ports.values()) {
      const window = this.windows.find((w) => w.id === port.windowId);
      if (!window) continue;

      // Get all ports on this edge (for calculating position in list)
      const portsOnEdge = Array.from(this.ports.values())
        .filter((p) => p.windowId === port.windowId && p.type === port.type)
        .sort((a, b) => {
          // Sort by DOM order in edge group
          const edgeGroups = this.edgeGroups.get(port.windowId);
          if (!edgeGroups) return 0;
          const edge =
            port.type === "input" ? edgeGroups.left : edgeGroups.right;
          const aEl = this.portElements.get(a.id);
          const bEl = this.portElements.get(b.id);
          if (!aEl || !bEl) return 0;
          return (
            Array.from(edge.children).indexOf(aEl) -
            Array.from(edge.children).indexOf(bEl)
          );
        });

      const portIndex = portsOnEdge.indexOf(port);
      if (portIndex === -1) continue;

      // Calculate position for connection drawing
      const portSpacing = 10;
      const portSize = 24;
      const addButtonSize = 24;

      const totalItemCount = portsOnEdge.length + 1; // ports + add button
      const totalHeight =
        totalItemCount * portSize + (totalItemCount - 1) * portSpacing;
      const startY = window.y + (window.h - totalHeight) / 2;

      // Y position (add button is first, then ports)
      const portY =
        startY +
        addButtonSize +
        portSpacing +
        portIndex * (portSize + portSpacing) +
        portSize / 2;

      // X position
      const x = port.type === "input" ? window.x : window.x + window.w;

      // Update stored position
      port.x = x;
      port.y = portY;
    }
  }

  private showPortTooltip(port: Port, portEl: HTMLElement) {
    const tooltip = document.createElement("div");
    tooltip.className = "port-tooltip";
    tooltip.id = "port-tooltip";

    const role = document.createElement("span");
    role.className = "role";
    role.textContent = port.element.role;

    const label = document.createElement("span");
    label.textContent = port.element.label || "(no label)";

    const value = document.createElement("span");
    value.className = "value";
    if (port.element.value) {
      value.textContent = String(port.element.value.value);
    } else {
      value.textContent = "(no value)";
    }

    tooltip.appendChild(role);
    tooltip.appendChild(document.createTextNode(" "));
    tooltip.appendChild(label);
    tooltip.appendChild(value);

    // Position tooltip near port
    const rect = portEl.getBoundingClientRect();
    tooltip.style.left = `${rect.right + 10}px`;
    tooltip.style.top = `${rect.top}px`;

    this.portContainer.appendChild(tooltip);
  }

  private hidePortTooltip() {
    const tooltip = document.getElementById("port-tooltip");
    if (tooltip) {
      tooltip.remove();
    }
  }

  private hideElementHighlight() {
    if (this.elementHighlight) {
      this.elementHighlight.remove();
      this.elementHighlight = null;
    }
  }

  private showPortCreationFeedback(x: number, y: number) {
    // Create a brief visual feedback circle
    const feedback = document.createElement("div");
    feedback.style.cssText = `
      position: absolute;
      left: ${x - 20}px;
      top: ${y - 20}px;
      width: 40px;
      height: 40px;
      border-radius: 50%;
      border: 3px solid ${
        this.portCreationMode === "input" ? "#34c759" : "#007aff"
      };
      pointer-events: none;
      z-index: 9999;
      animation: feedbackPulse 0.6s ease-out;
    `;

    // Add animation
    const style = document.createElement("style");
    style.textContent = `
      @keyframes feedbackPulse {
        0% {
          transform: scale(0.5);
          opacity: 1;
        }
        100% {
          transform: scale(1.5);
          opacity: 0;
        }
      }
    `;
    document.head.appendChild(style);

    this.portContainer.appendChild(feedback);

    // Remove after animation
    setTimeout(() => {
      feedback.remove();
      style.remove();
    }, 600);
  }

  private setupConnectionDrawing() {
    // Handle escape key to cancel connection
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        if (this.connectingFromPort) {
          this.cancelDragConnection();
        }
      }
    });

    // Global mouse move handler for drag connection
    document.addEventListener("mousemove", (e) => {
      if (this.connectingFromPort && this.tempConnectionLine) {
        this.updateTempConnection(e.clientX, e.clientY);
      }
    });

    // Global mouse up handler to complete or cancel drag connection
    document.addEventListener("mouseup", (e) => {
      if (this.connectingFromPort) {
        // Check if we're over an input port
        const element = document.elementFromPoint(e.clientX, e.clientY);
        if (element && element.classList.contains("port")) {
          const portId = element.getAttribute("data-port-id");
          if (portId) {
            const targetPort = this.ports.get(portId);
            if (targetPort && targetPort.type === "input") {
              this.createConnection(this.connectingFromPort, targetPort);
            }
          }
        }

        // Clean up drag state
        this.cancelDragConnection();
      }
    });
  }

  private startDragConnection(port: Port, e: MouseEvent) {
    this.connectingFromPort = port;
    const portEl = this.portElements.get(port.id);
    if (portEl) {
      portEl.classList.add("connecting");
    }
    console.log("🔗 Started drag connection from output port:", port);

    // Create temp connection line
    this.tempConnectionLine = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "path"
    );
    this.tempConnectionLine.classList.add("temp-connection");
    this.connectionsContainer.appendChild(this.tempConnectionLine);

    // Initial draw
    this.updateTempConnection(e.clientX, e.clientY);
  }

  private cancelDragConnection() {
    if (this.connectingFromPort) {
      const portEl = this.portElements.get(this.connectingFromPort.id);
      if (portEl) {
        portEl.classList.remove("connecting");
      }
      if (this.tempConnectionLine) {
        this.tempConnectionLine.remove();
        this.tempConnectionLine = null;
      }
      this.connectingFromPort = null;
      console.log("❌ Cancelled connection");
    }
  }

  private handleElementUpdate(update: any) {
    // When an element value changes, propagate to connected inputs
    if (update.update_type !== "ValueChanged") return;

    // Find port for this element
    const sourcePort = Array.from(this.ports.values()).find(
      (p) => p.element.id === update.element_id && p.type === "output"
    );

    if (!sourcePort) return;

    // Update the port's element value
    if (sourcePort.element.value) {
      (sourcePort.element.value as any).value = update.value.value;
    }

    // Find connections from this port
    const connections = this.connections.filter(
      (c) => c.sourcePort.id === sourcePort.id
    );

    for (const connection of connections) {
      this.propagateValue(connection);
    }
  }

  private async propagateValue(connection: Connection) {
    const sourceValue = connection.sourcePort.element.value;
    if (!sourceValue) {
      console.log("⚠️  Source port has no value to propagate");
      return;
    }

    const targetElement = connection.targetPort.element;
    const valueStr = String(sourceValue.value);

    console.log(
      `🔄 Propagating value "${valueStr}" from ${connection.sourcePort.element.role} to ${targetElement.role}`
    );

    // Write value to target element
    try {
      await this.axio.write(targetElement.id, valueStr);
      console.log("✅ Value propagated successfully");
    } catch (error) {
      console.error("❌ Failed to propagate value:", error);
    }
  }
}

// Initialize the demo when DOM is loaded
document.addEventListener("DOMContentLoaded", () => {
  new PortsDemo();
});
