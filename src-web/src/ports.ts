import { Allio, AX, AllioOcclusion, AllioPassthrough } from "allio";

type PortType = "input" | "output";

interface Port {
  id: string;
  windowId: AX.WindowId;
  element: AX.Element;
  type: PortType;
  isTransform: boolean;
}

interface Connection {
  id: string;
  sourceId: string;
  targetId: string;
}

interface PortPosition {
  x: number;
  y: number;
}

interface DragState {
  sourceElement: AX.Element;
  sourceWindow: AX.Window;
  targetElement: AX.Element | null;
  targetWindow: AX.Window | null;
}

// State
const state = {
  ports: new Map<string, Port>(),
  connections: [] as Connection[],
  creationMode: false,
  connectingFrom: null as Port | null,
  hoveredPort: null as Port | null,
  // Drag-to-connect state (creation mode)
  dragging: null as DragState | null,
};

// Transform input cache: elementId -> last input value
const transformInputCache = new Map<AX.ElementId, unknown>();

// DOM elements
const dom = {
  container: document.getElementById("portContainer")!,
  svg: document.getElementById("connections") as unknown as SVGSVGElement,
  menuBar: document.getElementById("menuBar")!,
  portElements: new Map<string, HTMLElement>(),
  windowContainers: new Map<AX.WindowId, HTMLElement>(),
  edgeGroups: new Map<AX.WindowId, { left: HTMLElement; right: HTMLElement }>(),
  tempLine: null as SVGPathElement | null,
  hoverOverlay: null as HTMLElement | null,
  infoPanel: null as HTMLElement | null,
  wiringSvg: null as SVGSVGElement | null,
  wiringPath: null as SVGPathElement | null,
  // Drag preview elements
  dragSourceOverlay: null as HTMLElement | null,
  dragTargetOverlay: null as HTMLElement | null,
  dragLine: null as SVGPathElement | null,
};

// Services
let allio: Allio;
let occlusion: AllioOcclusion;
let passthrough: AllioPassthrough;

// Computed port positions (updated on render)
const portPositions = new Map<string, PortPosition>();

// --- Initialization ---

async function init() {
  allio = new Allio();
  occlusion = new AllioOcclusion(allio);
  passthrough = new AllioPassthrough(allio);

  createHoverOverlay();
  createDragOverlays();
  setupEventListeners();

  await allio.connect();
  updateMenuBar();
}

function setupEventListeners() {
  // Window updates trigger re-render
  const render = () => renderAll();
  allio.on("sync:init", render);
  allio.on("window:added", render);
  allio.on("window:removed", render);
  allio.on("window:changed", render);

  // Element value changes trigger propagation
  allio.on("element:changed", ({ element }) => handleElementUpdate(element));

  // Mouse tracking for connections, hover, and drag preview
  allio.on("mouse:position", ({ x, y }) => {
    if (state.connectingFrom && dom.tempLine) {
      updateTempLine(x, y);
    }
    if (state.dragging) {
      updateDragPreview(x, y);
    }
    if (!state.dragging) {
      updatePortHover(x, y);
    }
  });

  // Menu bar toggles creation mode
  dom.menuBar.addEventListener("click", toggleCreationMode);

  // Keyboard shortcuts
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (state.dragging) cancelDrag();
      else if (state.creationMode) toggleCreationMode();
      else if (state.connectingFrom) cancelConnection();
    }
  });

  // Mouse events for drag-to-connect and port connections
  document.addEventListener("mousedown", onMouseDown);
  document.addEventListener("mouseup", onMouseUp);
}

// --- Mode Management ---

function toggleCreationMode() {
  state.creationMode = !state.creationMode;
  passthrough.mode = state.creationMode ? "opaque" : "auto";
  updateMenuBar();
}

function updateMenuBar() {
  dom.menuBar.classList.toggle("active", state.creationMode);
  dom.menuBar.innerHTML = state.creationMode
    ? `<span class="mode-indicator">●</span> Drag to connect elements | <kbd>Shift</kbd> on release for transform | <kbd>Esc</kbd> to exit`
    : `Click to enter creation mode | Drag output → input to connect | <kbd>Shift</kbd>+click to delete`;
}

// --- Drag-to-Connect (Creation Mode) ---

async function onMouseDown(e: MouseEvent) {
  // Handle port connection start (output ports)
  const portEl = (e.target as Element)?.closest(".port") as HTMLElement | null;
  if (portEl) {
    const portId = findPortIdByElement(portEl);
    const port = portId ? state.ports.get(portId) : null;
    if (port?.type === "output" && !e.shiftKey) {
      e.stopPropagation();
      startConnection(port);
      return;
    }
  }

  // Start drag-to-connect in creation mode
  if (!state.creationMode) return;
  if ((e.target as Element)?.closest("#menuBar, .port")) return;

  const window = getWindowAt(e.clientX, e.clientY);
  if (!window) return;

  try {
    const element = await allio.elementAt(e.clientX, e.clientY);

    // No tracked window at this position, or no bounds
    if (!element?.bounds) return;

    // Chromium/Electron lazy init: retry on next frame if we got a fallback
    if (element.is_fallback) {
      const x = e.clientX;
      const y = e.clientY;
      requestAnimationFrame(async () => {
        try {
          const retried = await allio.elementAt(x, y);
          if (!retried?.bounds) return;

          state.dragging = {
            sourceElement: retried,
            sourceWindow: window,
            targetElement: null,
            targetWindow: null,
          };
          showDragSource(retried);
        } catch {
          // Ignore retry errors
        }
      });
      return;
    }

    state.dragging = {
      sourceElement: element,
      sourceWindow: window,
      targetElement: null,
      targetWindow: null,
    };

    showDragSource(element);
  } catch (err) {
    console.error("Failed to start drag:", err);
  }
}

async function updateDragPreview(x: number, y: number) {
  if (!state.dragging) return;

  const source = state.dragging.sourceElement;
  if (!source.bounds) return;

  // Update drag line from source to cursor
  const sourceX = source.bounds.x + source.bounds.w;
  const sourceY = source.bounds.y + source.bounds.h / 2;
  updateDragLine(sourceX, sourceY, x, y);

  // Check what element is under cursor
  try {
    const targetElement = await allio.elementAt(x, y);
    const targetWindow = getWindowAt(x, y);

    // Skip fallback elements - next mouse move will retry naturally
    if (targetElement?.is_fallback) return;

    // Update target if changed
    if (targetElement?.id !== state.dragging.targetElement?.id) {
      state.dragging.targetElement = targetElement ?? null;
      state.dragging.targetWindow = targetWindow;

      if (targetElement?.bounds && targetElement.id !== source.id) {
        showDragTarget(targetElement);
      } else {
        hideDragTarget();
      }
    }
  } catch {
    // Ignore errors during preview
  }
}

async function onMouseUp(e: MouseEvent) {
  // Handle drag-to-connect completion
  if (state.dragging) {
    const isTransform = e.shiftKey;
    await completeDrag(isTransform);
    return;
  }

  // Handle port connection completion
  if (!state.connectingFrom) return;

  const el = document.elementFromPoint(e.clientX, e.clientY);
  const portEl = el?.closest(".port") as HTMLElement | null;

  if (portEl) {
    const portId = findPortIdByElement(portEl);
    const port = portId ? state.ports.get(portId) : null;
    if (port?.type === "input") {
      completeConnection(port);
      return;
    }
  }

  cancelConnection();
}

async function completeDrag(isTransform: boolean) {
  if (!state.dragging) return;

  const { sourceElement, sourceWindow, targetElement, targetWindow } =
    state.dragging;

  // Clean up drag visuals
  cancelDrag();

  // Need valid different elements
  if (
    !targetElement ||
    !targetWindow ||
    sourceElement.id === targetElement.id
  ) {
    return;
  }

  // Create port pairs for both elements (if they don't exist)
  const sourceExists = [...state.ports.values()].some(
    (p) => p.element.id === sourceElement.id
  );
  const targetExists = [...state.ports.values()].some(
    (p) => p.element.id === targetElement.id
  );

  if (!sourceExists) {
    createPortPair(sourceWindow.id, sourceElement, false);
    allio.watch(sourceElement.id);
  }

  if (!targetExists) {
    createPortPair(targetWindow.id, targetElement, isTransform);
    allio.watch(targetElement.id);
  }

  // Find output port of source and input port of target
  const sourceOutputPort = [...state.ports.values()].find(
    (p) => p.element.id === sourceElement.id && p.type === "output"
  );
  const targetInputPort = [...state.ports.values()].find(
    (p) => p.element.id === targetElement.id && p.type === "input"
  );

  if (!sourceOutputPort || !targetInputPort) return;

  // Create connection
  const exists = state.connections.some(
    (c) =>
      c.sourceId === sourceOutputPort.id && c.targetId === targetInputPort.id
  );

  if (!exists) {
    const connection: Connection = {
      id: `conn-${Date.now()}`,
      sourceId: sourceOutputPort.id,
      targetId: targetInputPort.id,
    };
    state.connections.push(connection);
    redrawConnections();
    propagateValue(connection);
  }

  showFeedback(
    targetElement.bounds!.x + targetElement.bounds!.w / 2,
    targetElement.bounds!.y + targetElement.bounds!.h / 2
  );
}

function cancelDrag() {
  state.dragging = null;
  hideDragSource();
  hideDragTarget();
  hideDragLine();
}

// --- Drag Overlay Management ---

function createDragOverlays() {
  dom.dragSourceOverlay = createOverlayElement(`
    position: absolute;
    pointer-events: none;
    border: 2px solid var(--port-output);
    border-radius: 4px;
    background: rgba(107, 143, 199, 0.15);
    box-sizing: border-box;
    display: none;
    z-index: 997;
  `);

  dom.dragTargetOverlay = createOverlayElement(`
    position: absolute;
    pointer-events: none;
    border: 2px solid var(--port-input);
    border-radius: 4px;
    background: rgba(99, 168, 125, 0.15);
    box-sizing: border-box;
    display: none;
    z-index: 997;
  `);

  dom.dragLine = document.createElementNS("http://www.w3.org/2000/svg", "path");
  dom.dragLine.classList.add("temp-connection");
  dom.dragLine.style.display = "none";
  dom.svg.appendChild(dom.dragLine);

  document.body.append(dom.dragSourceOverlay, dom.dragTargetOverlay);
}

function showDragSource(element: AX.Element) {
  if (!dom.dragSourceOverlay || !element.bounds || !state.dragging) return;
  const { x, y, w, h } = element.bounds;
  const window = state.dragging.sourceWindow;
  const container = dom.windowContainers.get(window.id);
  if (!container) return;

  // Move overlay into window container so it inherits the container's clip-path
  container.appendChild(dom.dragSourceOverlay);

  // Position with window-relative coordinates
  Object.assign(dom.dragSourceOverlay.style, {
    left: `${x - window.bounds.x}px`,
    top: `${y - window.bounds.y}px`,
    width: `${w}px`,
    height: `${h}px`,
    display: "block",
  });
}

function hideDragSource() {
  if (dom.dragSourceOverlay) dom.dragSourceOverlay.style.display = "none";
}

function showDragTarget(element: AX.Element) {
  if (!dom.dragTargetOverlay || !element.bounds) return;
  const targetWindow = state.dragging?.targetWindow;
  if (!targetWindow) return;

  const container = dom.windowContainers.get(targetWindow.id);
  if (!container) return;

  // Move overlay into window container so it inherits the container's clip-path
  container.appendChild(dom.dragTargetOverlay);

  const { x, y, w, h } = element.bounds;
  // Position with window-relative coordinates
  Object.assign(dom.dragTargetOverlay.style, {
    left: `${x - targetWindow.bounds.x}px`,
    top: `${y - targetWindow.bounds.y}px`,
    width: `${w}px`,
    height: `${h}px`,
    display: "block",
  });
}

function hideDragTarget() {
  if (dom.dragTargetOverlay) dom.dragTargetOverlay.style.display = "none";
}

function updateDragLine(x1: number, y1: number, x2: number, y2: number) {
  if (!dom.dragLine) return;
  dom.dragLine.setAttribute("d", makeBezierPath(x1, y1, x2, y2));
  dom.dragLine.style.display = "block";
}

function hideDragLine() {
  if (dom.dragLine) dom.dragLine.style.display = "none";
}

// --- Port Creation ---

function createPortPair(
  windowId: AX.WindowId,
  element: AX.Element,
  isTransform: boolean
) {
  // Skip if ports already exist for this element
  const exists = [...state.ports.values()].some(
    (p) => p.element.id === element.id
  );
  if (exists) return;

  const baseId = `port-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;

  const inputPort: Port = {
    id: `${baseId}-in`,
    windowId,
    element,
    type: "input",
    isTransform,
  };

  const outputPort: Port = {
    id: `${baseId}-out`,
    windowId,
    element,
    type: "output",
    isTransform,
  };

  state.ports.set(inputPort.id, inputPort);
  state.ports.set(outputPort.id, outputPort);

  createPortElement(inputPort);
  createPortElement(outputPort);
  updatePortPositions();
}

function createPortElement(port: Port) {
  const el = document.createElement("div");
  el.className = `port ${port.type}${port.isTransform ? " transform" : ""}`;
  el.setAttribute("ax-io", "opaque");
  el.title = formatPortTitle(port);

  el.addEventListener("click", (e) => {
    e.stopPropagation();
    if (e.shiftKey) {
      deletePort(port.id);
    } else if (port.type === "input" && state.connectingFrom) {
      completeConnection(port);
    }
  });

  const edges = dom.edgeGroups.get(port.windowId);
  if (edges) {
    (port.type === "input" ? edges.left : edges.right).appendChild(el);
  }

  dom.portElements.set(port.id, el);
}

function formatPortTitle(port: Port): string {
  const displayText =
    port.element.label ||
    (port.element.value ? String(port.element.value.value) : null) ||
    "(no label)";
  return port.isTransform
    ? `Transform: ${port.element.role} (text → function)`
    : `${port.element.role}: ${displayText}`;
}

function deletePort(portId: string) {
  const port = state.ports.get(portId);
  if (!port) return;

  // Clear hover state
  if (state.hoveredPort?.id === portId) {
    clearHoverOverlay();
    state.hoveredPort = null;
  }

  // Remove DOM element
  dom.portElements.get(portId)?.remove();
  dom.portElements.delete(portId);
  state.ports.delete(portId);

  // Remove related connections
  state.connections = state.connections.filter(
    (c) => c.sourceId !== portId && c.targetId !== portId
  );

  // Clear transform cache and unwatch if no more ports for this element
  const hasOtherPorts = [...state.ports.values()].some(
    (p) => p.element.id === port.element.id
  );
  if (!hasOtherPorts) {
    transformInputCache.delete(port.element.id);
    allio.unwatch(port.element.id).catch(() => {});
  }

  redrawConnections();
}

// --- Connections ---

function startConnection(port: Port) {
  state.connectingFrom = port;
  dom.portElements.get(port.id)?.classList.add("connecting");

  dom.tempLine = document.createElementNS("http://www.w3.org/2000/svg", "path");
  dom.tempLine.classList.add("temp-connection");

  const clipPath = occlusion.getAbsoluteClipPath(port.windowId);
  if (clipPath) dom.tempLine.style.clipPath = clipPath;

  dom.svg.appendChild(dom.tempLine);
}

function completeConnection(targetPort: Port) {
  if (!state.connectingFrom) return;
  if (state.connectingFrom.element.id === targetPort.element.id) {
    cancelConnection();
    return;
  }

  const exists = state.connections.some(
    (c) =>
      c.sourceId === state.connectingFrom!.id && c.targetId === targetPort.id
  );

  if (!exists) {
    const connection: Connection = {
      id: `conn-${Date.now()}`,
      sourceId: state.connectingFrom.id,
      targetId: targetPort.id,
    };
    state.connections.push(connection);
    propagateValue(connection);
  }

  cancelConnection();
}

function cancelConnection() {
  if (state.connectingFrom) {
    dom.portElements
      .get(state.connectingFrom.id)
      ?.classList.remove("connecting");
  }
  dom.tempLine?.remove();
  dom.tempLine = null;
  state.connectingFrom = null;
  redrawConnections();
}

function updateTempLine(x: number, y: number) {
  if (!dom.tempLine || !state.connectingFrom) return;
  const pos = portPositions.get(state.connectingFrom.id);
  if (pos) {
    dom.tempLine.setAttribute("d", makeBezierPath(pos.x, pos.y, x, y));
  }
}

// --- Rendering ---

function renderAll() {
  const windows = getWindowsSorted();

  // Clean up removed windows
  const currentIds = new Set(windows.map((w) => w.id));
  for (const [id, container] of dom.windowContainers) {
    if (!currentIds.has(id)) {
      container.remove();
      dom.windowContainers.delete(id);
      dom.edgeGroups.delete(id);
      // Remove orphaned ports
      for (const [portId, port] of state.ports) {
        if (port.windowId === id) deletePort(portId);
      }
    }
  }

  // Update window containers
  for (const window of windows) {
    renderWindowContainer(window);
  }

  updatePortPositions();
  redrawConnections();
}

function renderWindowContainer(window: AX.Window) {
  let container = dom.windowContainers.get(window.id);

  if (!container) {
    container = document.createElement("div");
    container.className = "window-container";

    const left = document.createElement("div");
    left.className = "edge-group left";

    const right = document.createElement("div");
    right.className = "edge-group right";

    container.append(left, right);
    dom.container.appendChild(container);

    dom.windowContainers.set(window.id, container);
    dom.edgeGroups.set(window.id, { left, right });
  }

  const { x, y, w, h } = window.bounds;
  Object.assign(container.style, {
    left: `${x}px`,
    top: `${y}px`,
    width: `${w}px`,
    height: `${h}px`,
    zIndex: String(occlusion.getZIndex(window.id)),
    clipPath: occlusion.getClipPath(window.id),
  });
}

function updatePortPositions() {
  const windows = getWindowsSorted();

  for (const port of state.ports.values()) {
    const window = windows.find((w) => w.id === port.windowId);
    if (!window) continue;

    const portsOnEdge = [...state.ports.values()].filter(
      (p) => p.windowId === port.windowId && p.type === port.type
    );

    const idx = portsOnEdge.indexOf(port);
    const spacing = 26;
    const totalHeight = portsOnEdge.length * spacing;
    const startY =
      window.bounds.y + (window.bounds.h - totalHeight) / 2 + spacing / 2;

    portPositions.set(port.id, {
      x:
        port.type === "input"
          ? window.bounds.x
          : window.bounds.x + window.bounds.w,
      y: startY + idx * spacing,
    });
  }
}

function redrawConnections() {
  dom.svg.querySelectorAll(".connection-line").forEach((el) => el.remove());

  for (const conn of state.connections) {
    const sourcePos = portPositions.get(conn.sourceId);
    const targetPos = portPositions.get(conn.targetId);
    const sourcePort = state.ports.get(conn.sourceId);
    const targetPort = state.ports.get(conn.targetId);

    if (!sourcePos || !targetPos || !sourcePort || !targetPort) continue;

    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.classList.add("connection-line");
    path.setAttribute(
      "d",
      makeBezierPath(sourcePos.x, sourcePos.y, targetPos.x, targetPos.y)
    );

    // Clip by backmost window
    const sourceZ = occlusion.getZIndex(sourcePort.windowId);
    const targetZ = occlusion.getZIndex(targetPort.windowId);
    const backmostId =
      sourceZ < targetZ ? sourcePort.windowId : targetPort.windowId;
    const clipPath = occlusion.getAbsoluteClipPath(backmostId);
    if (clipPath) path.style.clipPath = clipPath;

    dom.svg.appendChild(path);
  }
}

function makeBezierPath(
  x1: number,
  y1: number,
  x2: number,
  y2: number
): string {
  const curve = Math.min(Math.abs(x2 - x1) / 2, 80);
  return `M ${x1},${y1} C ${x1 + curve},${y1} ${x2 - curve},${y2} ${x2},${y2}`;
}

// --- Hover Overlay ---

function createHoverOverlay() {
  dom.hoverOverlay = createOverlayElement(`
    position: absolute;
    pointer-events: none;
    border: 2px solid var(--port-output);
    border-radius: 4px;
    background: rgba(107, 143, 199, 0.1);
    box-sizing: border-box;
    display: none;
    z-index: 999;
  `);

  dom.wiringSvg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  Object.assign(dom.wiringSvg.style, {
    position: "absolute",
    inset: "0",
    width: "100%",
    height: "100%",
    pointerEvents: "none",
    zIndex: "998",
    display: "none",
  });

  const style = document.createElementNS("http://www.w3.org/2000/svg", "style");
  style.textContent = `
    @keyframes wiringFlowOut { to { stroke-dashoffset: -10; } }
    @keyframes wiringFlowIn { to { stroke-dashoffset: 10; } }
    .wiring-out { animation: wiringFlowOut 0.4s linear infinite; }
    .wiring-in { animation: wiringFlowIn 0.4s linear infinite; }
  `;
  dom.wiringSvg.appendChild(style);

  dom.wiringPath = document.createElementNS(
    "http://www.w3.org/2000/svg",
    "path"
  );
  dom.wiringPath.setAttribute("fill", "none");
  dom.wiringPath.setAttribute("stroke-width", "2");
  dom.wiringPath.setAttribute("stroke-dasharray", "6,4");
  dom.wiringPath.setAttribute("opacity", "0.7");
  dom.wiringSvg.appendChild(dom.wiringPath);

  dom.infoPanel = createOverlayElement(`
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
  `);

  document.body.append(dom.hoverOverlay, dom.wiringSvg, dom.infoPanel);
}

function createOverlayElement(styles: string): HTMLElement {
  const el = document.createElement("div");
  el.style.cssText = styles;
  return el;
}

function updatePortHover(x: number, y: number) {
  const el = document.elementFromPoint(x, y);
  const portEl = el?.closest(".port") as HTMLElement | null;

  if (portEl) {
    const portId = findPortIdByElement(portEl);
    const port = portId ? state.ports.get(portId) : null;
    if (port && port !== state.hoveredPort) {
      state.hoveredPort = port;
      showHoverOverlay(port);
    }
  } else if (state.hoveredPort) {
    state.hoveredPort = null;
    clearHoverOverlay();
  }
}

async function showHoverOverlay(port: Port) {
  // Refresh element data
  try {
    port.element = await allio.refresh(port.element.id);
  } catch {
    // Use cached data if refresh fails
  }

  const { element } = port;
  const bounds = element.bounds;
  if (!bounds || !dom.hoverOverlay || !dom.infoPanel) return;

  // Get window container to inherit its clip-path
  const axWindow = allio.windows.get(port.windowId);
  const container = dom.windowContainers.get(port.windowId);
  if (!axWindow || !container) return;

  // Move overlay into window container so it inherits the container's clip-path
  container.appendChild(dom.hoverOverlay);

  // Position with window-relative coordinates
  Object.assign(dom.hoverOverlay.style, {
    left: `${bounds.x - axWindow.bounds.x}px`,
    top: `${bounds.y - axWindow.bounds.y}px`,
    width: `${bounds.w}px`,
    height: `${bounds.h}px`,
    display: "block",
  });

  // Build info content
  dom.infoPanel.innerHTML = buildInfoPanelHtml(element, port.isTransform);

  // Position info panel below or above bounds
  const panelHeight = 100;
  const belowY = bounds.y + bounds.h + 8;
  const aboveY = bounds.y - panelHeight - 8;

  Object.assign(dom.infoPanel.style, {
    left: `${bounds.x}px`,
    top:
      belowY + panelHeight < window.innerHeight
        ? `${belowY}px`
        : `${Math.max(8, aboveY)}px`,
    display: "block",
  });

  // Draw wiring line
  drawWiringLine(port, bounds);
}

function buildInfoPanelHtml(element: AX.Element, isTransform: boolean): string {
  const lines: string[] = [];

  lines.push(
    `<div style="color: var(--port-output); font-weight: 600; margin-bottom: 4px;">${element.role} <span style="opacity: 0.5">(${element.platform_role})</span></div>`
  );

  if (element.label) {
    lines.push(
      `<div><span style="opacity: 0.6;">Label:</span> ${escapeHtml(
        element.label
      )}</div>`
    );
  }

  if (element.value) {
    const val = element.value.value;
    const displayVal = typeof val === "string" ? `"${val}"` : String(val);
    lines.push(
      `<div><span style="opacity: 0.6;">Value:</span> <span style="color: var(--port-input);">${escapeHtml(
        displayVal
      )}</span></div>`
    );
  }

  // Show cached input for transforms
  if (isTransform) {
    const cachedInput = transformInputCache.get(element.id);
    if (cachedInput !== undefined) {
      const displayInput =
        typeof cachedInput === "string"
          ? `"${cachedInput}"`
          : String(cachedInput);
      lines.push(
        `<div><span style="opacity: 0.6;">Last input:</span> <span style="color: var(--port-transform);">${escapeHtml(
          displayInput
        )}</span></div>`
      );
    }
  }

  if (element.description) {
    lines.push(
      `<div style="opacity: 0.7; font-style: italic; margin-top: 2px;">${escapeHtml(
        element.description
      )}</div>`
    );
  }

  if (element.disabled) {
    lines.push(`<div style="color: #ff6b6b; margin-top: 2px;">Disabled</div>`);
  }

  if (element.actions.length > 0) {
    lines.push(
      `<div style="opacity: 0.5; margin-top: 4px; font-size: 10px;">Actions: ${element.actions.join(
        ", "
      )}</div>`
    );
  }

  return lines.join("");
}

function drawWiringLine(
  port: Port,
  bounds: { x: number; y: number; w: number; h: number }
) {
  if (!dom.wiringSvg || !dom.wiringPath) return;

  const portPos = portPositions.get(port.id);
  if (!portPos) return;

  const elemX = port.type === "input" ? bounds.x : bounds.x + bounds.w;
  const elemY = Math.max(bounds.y, Math.min(bounds.y + bounds.h, portPos.y));

  const distance = Math.abs(portPos.x - elemX);
  if (distance < 20) {
    dom.wiringSvg.style.display = "none";
    return;
  }

  const curve = Math.min(distance / 2, 50);
  const d =
    port.type === "input"
      ? `M ${elemX} ${elemY} C ${elemX - curve} ${elemY} ${portPos.x + curve} ${
          portPos.y
        } ${portPos.x} ${portPos.y}`
      : `M ${elemX} ${elemY} C ${elemX + curve} ${elemY} ${portPos.x - curve} ${
          portPos.y
        } ${portPos.x} ${portPos.y}`;

  dom.wiringPath.setAttribute("d", d);
  dom.wiringPath.setAttribute(
    "class",
    port.type === "input" ? "wiring-in" : "wiring-out"
  );
  dom.wiringPath.setAttribute(
    "stroke",
    port.type === "input" ? "var(--port-input)" : "var(--port-output)"
  );
  dom.wiringSvg.style.display = "block";
}

function clearHoverOverlay() {
  if (dom.hoverOverlay) dom.hoverOverlay.style.display = "none";
  if (dom.infoPanel) dom.infoPanel.style.display = "none";
  if (dom.wiringSvg) dom.wiringSvg.style.display = "none";
}

// --- Value Propagation ---

function handleElementUpdate(element: AX.Element) {
  for (const port of state.ports.values()) {
    if (port.element.id === element.id) {
      port.element = element;

      // If this is a transform element and its value changed, re-evaluate with cached input
      if (port.isTransform && port.type === "input") {
        const cachedInput = transformInputCache.get(element.id);
        if (cachedInput !== undefined) {
          reEvaluateTransform(port, cachedInput);
        }
      }

      // Normal output propagation
      if (port.type === "output" && !port.isTransform) {
        for (const conn of state.connections) {
          if (conn.sourceId === port.id) {
            propagateValue(conn);
          }
        }
      }
    }
  }
}

async function reEvaluateTransform(inputPort: Port, inputValue: unknown) {
  try {
    const freshElement = await allio.refresh(inputPort.element.id);
    const functionCode = freshElement.value?.value;

    if (typeof functionCode !== "string") return;

    const transformFn = parseTransformFunction(functionCode);
    const normalizedInput =
      typeof inputValue === "bigint" ? Number(inputValue) : inputValue;
    const outputValue = transformFn(normalizedInput);

    // Find output port and propagate downstream
    const outputPort = [...state.ports.values()].find(
      (p) => p.element.id === inputPort.element.id && p.type === "output"
    );
    if (!outputPort) return;

    for (const conn of state.connections) {
      if (conn.sourceId === outputPort.id) {
        const downstreamTarget = state.ports.get(conn.targetId);
        if (!downstreamTarget) continue;

        if (downstreamTarget.isTransform) {
          await propagateThroughTransform(downstreamTarget, outputValue);
        } else {
          await writeValueToElement(downstreamTarget.element, outputValue);
        }
      }
    }
  } catch (err) {
    console.error("Transform re-evaluation error:", err);
  }
}

async function propagateValue(conn: Connection) {
  const sourcePort = state.ports.get(conn.sourceId);
  const targetPort = state.ports.get(conn.targetId);
  if (!sourcePort || !targetPort) return;

  const value = sourcePort.element.value;
  if (!value) return;

  if (targetPort.isTransform) {
    await propagateThroughTransform(targetPort, value.value);
  } else {
    await writeValueToElement(targetPort.element, value.value);
  }
}

async function propagateThroughTransform(
  targetPort: Port,
  inputValue: unknown
) {
  // Cache the input value for re-evaluation when transform text changes
  transformInputCache.set(targetPort.element.id, inputValue);

  try {
    const freshElement = await allio.refresh(targetPort.element.id);
    const functionCode = freshElement.value?.value;

    if (typeof functionCode !== "string") {
      console.warn("Transform element has no text value");
      return;
    }

    const transformFn = parseTransformFunction(functionCode);
    const normalizedInput =
      typeof inputValue === "bigint" ? Number(inputValue) : inputValue;
    const outputValue = transformFn(normalizedInput);

    // Find output port for this transform element
    const outputPort = [...state.ports.values()].find(
      (p) => p.element.id === targetPort.element.id && p.type === "output"
    );
    if (!outputPort) return;

    // Propagate to downstream connections
    for (const conn of state.connections) {
      if (conn.sourceId === outputPort.id) {
        const downstreamTarget = state.ports.get(conn.targetId);
        if (!downstreamTarget) continue;

        if (downstreamTarget.isTransform) {
          await propagateThroughTransform(downstreamTarget, outputValue);
        } else {
          await writeValueToElement(downstreamTarget.element, outputValue);
        }
      }
    }
  } catch (err) {
    console.error("Transform error:", err);
  }
}

function parseTransformFunction(code: string): (val: unknown) => unknown {
  let cleanCode = code.trim();

  if (cleanCode.startsWith("export default")) {
    cleanCode = cleanCode.replace(/^export\s+default\s+/, "");
  }

  if (cleanCode.startsWith("function") || cleanCode.includes("=>")) {
    return new Function(`return (${cleanCode})`)() as (val: unknown) => unknown;
  }

  return new Function("val", cleanCode) as (val: unknown) => unknown;
}

async function writeValueToElement(element: AX.Element, value: unknown) {
  try {
    const primitive = typeof value === "bigint" ? Number(value) : value;
    await allio.writeValue(element, primitive as string | number | boolean);
  } catch (err) {
    console.error("Failed to propagate:", err);
  }
}

// --- Utilities ---

function getWindowsSorted(): AX.Window[] {
  return allio.zOrder
    .map((id) => allio.windows.get(id))
    .filter((w): w is AX.Window => !!w);
}

function getWindowAt(x: number, y: number): AX.Window | null {
  for (const w of getWindowsSorted()) {
    const b = w.bounds;
    if (x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) {
      return w;
    }
  }
  return null;
}

function findPortIdByElement(el: HTMLElement): string | undefined {
  for (const [id, portEl] of dom.portElements) {
    if (portEl === el) return id;
  }
  return undefined;
}

function escapeHtml(str: string): string {
  return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function showFeedback(x: number, y: number) {
  const el = document.createElement("div");
  el.className = "feedback";
  el.style.left = `${x - 15}px`;
  el.style.top = `${y - 15}px`;
  dom.container.appendChild(el);
  setTimeout(() => el.remove(), 400);
}

// --- Bootstrap ---

document.addEventListener("DOMContentLoaded", init);
