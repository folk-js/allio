import {
  Allio,
  AX,
  AllioOcclusion,
  AllioPassthrough,
  queryString,
  accepts,
} from "allio";

// --- Window Interface System ---

interface WindowInterface {
  windowId: AX.WindowId;
  queryStr: string; // e.g. "(tree) listitem { checkbox:completed textfield:text }"
  observedRootId?: AX.ElementId;
  outputPortId?: string;
}

/** Execute a window interface query and return results */
function executeInterfaceQuery(iface: WindowInterface): unknown[] {
  if (!iface.observedRootId) return [];

  // Get the window root element
  const rootElement = allio.getRootElement(iface.windowId);
  if (!rootElement) return [];

  try {
    const results = queryString(allio, rootElement.id, iface.queryStr);
    return results.map((r) => {
      // Return extracted fields without the element reference
      const { element: _el, ...data } = r;
      return data;
    });
  } catch (err) {
    console.error("[interface] Query error:", err);
    return [];
  }
}

/** Propagate a value from a port to all its connections */
async function propagateValueFromPort(sourcePort: Port, value: unknown) {
  for (const conn of state.connections) {
    if (conn.sourceId === sourcePort.id) {
      const targetPort = state.ports.get(conn.targetId);
      if (!targetPort) continue;

      if (targetPort.isTransform) {
        await propagateThroughTransform(targetPort, value);
      } else {
        await writeValueToElement(targetPort.element, value);
      }
    }
  }
}

type PortType = "input" | "output";

interface Port {
  id: string;
  windowId: AX.WindowId;
  element: AX.TypedElement;
  type: PortType;
  isTransform: boolean;
  // For TODO query: the tree element we're observing
  treeElementId?: AX.ElementId;
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
  sourceElement: AX.TypedElement;
  sourceWindow: AX.Window;
  targetElement: AX.TypedElement | null;
  targetWindow: AX.Window | null;
}

/** Walk up the tree from an element until we find a "tree" role */
async function findTreeAncestor(
  elementId: AX.ElementId
): Promise<AX.TypedElement | null> {
  let currentId: AX.ElementId | undefined = elementId;
  const visited = new Set<AX.ElementId>();

  while (currentId !== undefined) {
    if (visited.has(currentId)) break;
    visited.add(currentId);

    try {
      const element = await allio.getElement(currentId, "current");
      if (element.role === "tree") {
        return element;
      }
      const parent = await allio.parent(currentId);
      currentId = parent?.id;
    } catch {
      break;
    }
  }
  return null;
}

// Port drag state
interface PortDragState {
  port: Port;
  startX: number;
  startY: number;
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
  // Port drag state (for dragging from output ports)
  portDragging: null as PortDragState | null,
  // Window interfaces (queried windows)
  interfaces: new Map<AX.WindowId, WindowInterface>(),
};

// Transform input cache: elementId -> last input value
const transformInputCache = new Map<AX.ElementId, unknown>();

// DOM elements
const dom = {
  container: document.getElementById("portContainer")!,
  svg: document.getElementById("connections") as unknown as SVGSVGElement,
  menuBar: document.getElementById("menuBar")!,
  dropOverlay: document.getElementById("dropOverlay")!,
  portElements: new Map<string, HTMLElement>(),
  windowContainers: new Map<AX.WindowId, HTMLElement>(),
  queryBars: new Map<AX.WindowId, HTMLElement>(),
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

  // Listen to subtree:changed for observed trees
  allio.on("subtree:changed", ({ root_id }) => {
    // Check interfaces first
    for (const iface of state.interfaces.values()) {
      if (iface.observedRootId === root_id) {
        const results = executeInterfaceQuery(iface);
        console.log("[interface] Query results:", results);

        if (iface.outputPortId) {
          const outputPort = state.ports.get(iface.outputPortId);
          if (outputPort) {
            propagateValueFromPort(outputPort, results);
          }
        }
        return; // Interface handled it
      }
    }

    // Fallback: legacy tree ports (from drag-to-connect)
    const outputPort = [...state.ports.values()].find(
      (p) => p.treeElementId === root_id && p.type === "output"
    );
    if (outputPort) {
      const results = queryString(
        allio,
        root_id,
        "listitem { checkbox:completed textfield:text }"
      );
      const data = results.map((r) => {
        const { element: _el, ...fields } = r;
        return fields;
      });
      console.log("[subtree:changed] Legacy tree results:", data);
      propagateValueFromPort(outputPort, data);
    }
  });

  // Element value changes trigger propagation
  allio.on("element:changed", ({ element }) => {
    handleElementUpdate(element as AX.TypedElement);
  });

  // Clean up ports when elements are removed
  allio.on("element:removed", ({ element_id }) => {
    const portsToRemove = [...state.ports.values()]
      .filter((p) => p.element.id === element_id)
      .map((p) => p.id);
    portsToRemove.forEach((portId) => deletePort(portId));
  });

  // Mouse tracking for connections, hover, and drag preview
  allio.on("mouse:position", ({ x, y }) => {
    if (state.portDragging) {
      updatePortDrag(x, y);
    } else if (state.connectingFrom && dom.tempLine) {
      updateTempLine(x, y);
    }
    if (state.dragging) {
      updateDragPreview(x, y);
    }
    if (!state.dragging && !state.portDragging) {
      updatePortHover(x, y);
    }
  });

  // Menu bar toggles creation mode
  dom.menuBar.addEventListener("click", toggleCreationMode);

  // Keyboard shortcuts
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (state.portDragging) cancelPortDrag();
      else if (state.dragging) cancelDrag();
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
    ? `<span style="color:#5eead4;margin-right:4px">●</span> Click element → transform | <kbd>Shift</kbd>+click window → query | <kbd>Esc</kbd> exit`
    : `Click to enter creation mode`;
}

// --- Drag-to-Connect (Creation Mode) ---

async function onMouseDown(e: MouseEvent) {
  // Skip if clicked on port (handled by port's own mousedown)
  if ((e.target as Element)?.closest(".port")) return;

  // Skip if clicked on query bar
  if ((e.target as Element)?.closest(".query-bar")) return;

  // Creation mode actions
  if (!state.creationMode) return;
  if ((e.target as Element)?.closest("#menuBar")) return;

  try {
    // Get the element at click position to determine its window
    const element = await allio.elementAt(e.clientX, e.clientY);
    if (!element?.bounds) return;
    if (element.is_fallback) return;

    const windowId = element.window_id;

    // Shift+click: make the element's window queried
    if (e.shiftKey) {
      makeWindowQueried(windowId);
      showFeedback(e.clientX, e.clientY);
      return;
    }

    // Regular click: make clicked element a transform (if it's a textarea)
    if (element.role === "textarea") {
      createPortPair(windowId, element, true);
      allio.watch(element.id);
      showFeedback(e.clientX, e.clientY);
    }
  } catch (err) {
    console.error("Failed to handle creation mode click:", err);
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
  // Handle port drag completion
  if (state.portDragging) {
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const portEl = el?.closest(".port") as HTMLElement | null;

    if (portEl) {
      const portId = findPortIdByElement(portEl);
      const port = portId ? state.ports.get(portId) : null;
      if (port?.type === "input") {
        completePortDrag(port);
        return;
      }
    }

    // Drop on element (create port if needed)
    await dropPortOnElement(e.clientX, e.clientY);
    return;
  }

  // Handle drag-to-connect completion
  if (state.dragging) {
    const isTransform = e.shiftKey;
    await completeDrag(isTransform);
    return;
  }

  // Handle legacy port connection completion (startConnection flow)
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

  // --- TODO Query: Walk up to find tree ancestor for source ---
  const treeElement = await findTreeAncestor(sourceElement.id);
  const effectiveSourceElement = treeElement || sourceElement;
  const treeElementId = treeElement?.id;

  // Create port pairs for both elements (if they don't exist)
  const sourceExists = [...state.ports.values()].some(
    (p) =>
      p.element.id === effectiveSourceElement.id ||
      p.treeElementId === treeElementId
  );
  const targetExists = [...state.ports.values()].some(
    (p) => p.element.id === targetElement.id
  );

  if (!sourceExists) {
    createPortPair(
      sourceWindow.id,
      effectiveSourceElement,
      false,
      treeElementId
    );
    if (treeElementId) {
      // Set up observation for the entire tree structure
      await allio.observe(treeElementId, { depth: 10, wait_between_ms: 100 });
    } else {
      allio.watch(effectiveSourceElement.id);
    }
  }

  if (!targetExists) {
    createPortPair(targetWindow.id, targetElement, isTransform);
    allio.watch(targetElement.id);
  }

  // Find output port of source and input port of target
  const sourceOutputPort = [...state.ports.values()].find(
    (p) =>
      (p.element.id === effectiveSourceElement.id ||
        p.treeElementId === treeElementId) &&
      p.type === "output"
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

    // If this is a tree, do initial propagation; future updates via subtree:changed
    if (treeElementId) {
      // Use generic todo-style query for trees created via drag
      const results = queryString(
        allio,
        treeElementId,
        "listitem { checkbox:completed textfield:text }"
      );
      const data = results.map((r) => {
        const { element: _el, ...fields } = r;
        return fields;
      });
      const outputPort = state.ports.get(connection.sourceId);
      if (outputPort) {
        await propagateValueFromPort(outputPort, data);
      }
    } else {
      propagateValue(connection);
    }
  }

  showFeedback(
    targetElement.bounds!.x + targetElement.bounds!.w / 2,
    targetElement.bounds!.y + targetElement.bounds!.h / 2
  );
}

function cancelDrag() {
  state.dragging = null;
  if (dom.dragSourceOverlay) dom.dragSourceOverlay.style.display = "none";
  hideDragTarget();
  hideDragLine();
}

// --- Drag Overlay Management ---

function createDragOverlays() {
  dom.dragSourceOverlay = createOverlayElement(`
    position: absolute;
    pointer-events: none;
    border: 2px solid rgba(255, 255, 255, 0.6);
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.1);
    box-sizing: border-box;
    display: none;
    z-index: 997;
  `);

  dom.dragTargetOverlay = createOverlayElement(`
    position: absolute;
    pointer-events: none;
    border: 2px dashed rgba(255, 255, 255, 0.6);
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
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

function showDragTarget(element: AX.TypedElement) {
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
  element: AX.TypedElement,
  isTransform: boolean,
  treeElementId?: AX.ElementId
) {
  // Skip if ports already exist for this element
  const exists = [...state.ports.values()].some(
    (p) =>
      p.element.id === element.id ||
      (treeElementId && p.treeElementId === treeElementId)
  );
  if (exists) return;

  const baseId = `port-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;

  const inputPort: Port = {
    id: `${baseId}-in`,
    windowId,
    element,
    type: "input",
    isTransform,
    treeElementId,
  };

  const outputPort: Port = {
    id: `${baseId}-out`,
    windowId,
    element,
    type: "output",
    isTransform,
    treeElementId,
  };

  state.ports.set(inputPort.id, inputPort);
  state.ports.set(outputPort.id, outputPort);

  createPortElement(inputPort);
  createPortElement(outputPort);
  updatePortPositions();
}

/** Create just an input port for an element (for receiving values from connections) */
function createInputPort(
  windowId: AX.WindowId,
  element: AX.TypedElement
): Port | null {
  // Skip if input port already exists for this element
  const exists = [...state.ports.values()].some(
    (p) => p.element.id === element.id && p.type === "input"
  );
  if (exists) return null;

  const portId = `port-${Date.now()}-${Math.random()
    .toString(36)
    .slice(2, 7)}-in`;

  const inputPort: Port = {
    id: portId,
    windowId,
    element,
    type: "input",
    isTransform: false,
  };

  state.ports.set(inputPort.id, inputPort);
  createPortElement(inputPort);
  updatePortPositions();

  return inputPort;
}

function createPortElement(port: Port) {
  const el = document.createElement("div");
  el.className = `port ${port.type}${port.isTransform ? " transform" : ""}`;
  el.setAttribute("ax-io", "opaque");
  el.title = formatPortTitle(port);

  // Mousedown: start drag for output ports, handle input ports during active drag
  el.addEventListener("mousedown", (e) => {
    e.stopPropagation();
    e.preventDefault();

    if (e.shiftKey) {
      deletePort(port.id);
      return;
    }

    // Clicking input port while dragging -> complete connection
    if (port.type === "input" && state.portDragging) {
      completePortDrag(port);
      return;
    }

    // Start dragging from output port
    if (port.type === "output") {
      startPortDrag(port, e.clientX, e.clientY);
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

  // Clean up tree watch set if this was a tree port
  if (port.treeElementId) {
    const hasOtherTreePorts = [...state.ports.values()].some(
      (p) => p.treeElementId === port.treeElementId
    );
    if (!hasOtherTreePorts) {
      // Stop observing the tree when no ports reference it
      allio.unobserve(port.treeElementId).catch(() => {});
    }
  }

  redrawConnections();
}

// --- Connections ---

// Legacy connection start (kept for reference, now handled by port drag)
function _startConnection(port: Port) {
  state.connectingFrom = port;
  dom.portElements.get(port.id)?.classList.add("connecting");

  dom.tempLine = document.createElementNS("http://www.w3.org/2000/svg", "path");
  dom.tempLine.classList.add("temp-connection");

  const clipPath = occlusion.getAbsoluteClipPath(port.windowId);
  if (clipPath) dom.tempLine.style.clipPath = clipPath;

  dom.svg.appendChild(dom.tempLine);
}
void _startConnection; // Suppress unused warning

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

// --- Port Dragging ---

function startPortDrag(port: Port, x: number, y: number) {
  state.portDragging = { port, startX: x, startY: y };
  dom.portElements.get(port.id)?.classList.add("dragging");

  // Create temp line
  dom.tempLine = document.createElementNS("http://www.w3.org/2000/svg", "path");
  dom.tempLine.classList.add("temp-connection");
  dom.svg.appendChild(dom.tempLine);

  // Enable passthrough for hover detection
  passthrough.mode = "opaque";
}

async function updatePortDrag(x: number, y: number) {
  if (!state.portDragging || !dom.tempLine) return;

  // Update temp line
  const pos = portPositions.get(state.portDragging.port.id);
  if (pos) {
    dom.tempLine.setAttribute("d", makeBezierPath(pos.x, pos.y, x, y));
  }

  // Update hover overlay on target element
  try {
    const element = await allio.elementAt(x, y);
    if (element?.bounds && element.id !== state.portDragging.port.element.id) {
      showDragHoverOverlay(element);
    } else {
      hideDragHoverOverlay();
    }
  } catch {
    hideDragHoverOverlay();
  }
}

async function completePortDrag(targetPort?: Port) {
  if (!state.portDragging) return;

  const sourcePort = state.portDragging.port;

  // If target port provided, connect directly
  if (targetPort) {
    if (sourcePort.id !== targetPort.id) {
      const exists = state.connections.some(
        (c) => c.sourceId === sourcePort.id && c.targetId === targetPort.id
      );
      if (!exists) {
        const connection: Connection = {
          id: `conn-${Date.now()}`,
          sourceId: sourcePort.id,
          targetId: targetPort.id,
        };
        state.connections.push(connection);
        propagateValue(connection);
      }
    }
    cancelPortDrag();
    return;
  }

  // Try to drop onto an element under cursor (checked elsewhere)
  cancelPortDrag();
}

async function dropPortOnElement(x: number, y: number) {
  if (!state.portDragging) return;

  const sourcePort = state.portDragging.port;

  try {
    const element = await allio.elementAt(x, y);
    if (!element?.bounds) {
      cancelPortDrag();
      return;
    }

    const targetWindow = getWindowAt(x, y);
    if (!targetWindow) {
      cancelPortDrag();
      return;
    }

    // Check if element already has ports
    const existingInputPort = [...state.ports.values()].find(
      (p) => p.element.id === element.id && p.type === "input"
    );

    if (existingInputPort) {
      // Connect to existing port
      const exists = state.connections.some(
        (c) =>
          c.sourceId === sourcePort.id && c.targetId === existingInputPort.id
      );
      if (!exists) {
        const connection: Connection = {
          id: `conn-${Date.now()}`,
          sourceId: sourcePort.id,
          targetId: existingInputPort.id,
        };
        state.connections.push(connection);
        propagateValue(connection);
      }
    } else {
      // Check if element accepts value writes (for creating new port)
      // Elements with string/number/boolean values can be written to
      const canWrite =
        accepts(element, "string") ||
        accepts(element, "number") ||
        accepts(element, "boolean");

      if (canWrite) {
        // Create just an input port for this target (NOT a transform)
        const newInputPort = createInputPort(targetWindow.id, element);
        allio.watch(element.id);

        if (newInputPort) {
          const connection: Connection = {
            id: `conn-${Date.now()}`,
            sourceId: sourcePort.id,
            targetId: newInputPort.id,
          };
          state.connections.push(connection);
          propagateValue(connection);
        }
      }
    }

    showFeedback(x, y);
  } catch (err) {
    console.error("Failed to drop port:", err);
  }

  cancelPortDrag();
}

function cancelPortDrag() {
  if (state.portDragging) {
    dom.portElements
      .get(state.portDragging.port.id)
      ?.classList.remove("dragging");
  }
  dom.tempLine?.remove();
  dom.tempLine = null;
  state.portDragging = null;
  hideDragHoverOverlay();

  if (!state.creationMode) {
    passthrough.mode = "auto";
  }
  redrawConnections();
}

function showDragHoverOverlay(element: AX.TypedElement) {
  if (!dom.hoverOverlay || !element.bounds) return;

  // Find the window for this element
  const axWindow = allio.windows.get(element.window_id);
  const container = dom.windowContainers.get(element.window_id);
  if (!axWindow || !container) return;

  // Move overlay into window container so it inherits the container's clip-path
  container.appendChild(dom.hoverOverlay);

  const { x, y, w, h } = element.bounds;
  // Position with window-relative coordinates
  Object.assign(dom.hoverOverlay.style, {
    left: `${x - axWindow.bounds.x}px`,
    top: `${y - axWindow.bounds.y}px`,
    width: `${w}px`,
    height: `${h}px`,
    display: "block",
  });
}

function hideDragHoverOverlay() {
  if (dom.hoverOverlay) {
    dom.hoverOverlay.style.display = "none";
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
      // Remove query bar
      const queryBar = dom.queryBars.get(id);
      if (queryBar) {
        queryBar.remove();
        dom.queryBars.delete(id);
      }
      // Remove interface
      state.interfaces.delete(id);
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
  const isQueried = state.interfaces.has(window.id);
  const queryBarHeight = 28;
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

  // Update queried class
  container.classList.toggle("queried", isQueried);

  const { x, y, w, h } = window.bounds;
  Object.assign(container.style, {
    left: `${x}px`,
    top: `${y}px`,
    width: `${w}px`,
    height: `${h}px`,
    zIndex: String(occlusion.getZIndex(window.id)),
    clipPath: occlusion.getClipPath(window.id),
  });

  // Render query bar for queried windows
  renderQueryBar(window, queryBarHeight);
}

function renderQueryBar(window: AX.Window, height: number) {
  const isQueried = state.interfaces.has(window.id);
  let queryBar = dom.queryBars.get(window.id);

  if (isQueried) {
    if (!queryBar) {
      queryBar = document.createElement("div");
      queryBar.className = "query-bar";
      queryBar.setAttribute("ax-io", "opaque");

      const input = document.createElement("input");
      input.type = "text";
      input.placeholder = "(tree) listitem { checkbox:done textfield:text }";

      input.addEventListener("keydown", (e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          updateInterfaceQuery(window.id, input.value);
        }
      });
      input.addEventListener("blur", () => {
        updateInterfaceQuery(window.id, input.value);
      });

      queryBar.appendChild(input);
      dom.container.appendChild(queryBar);
      dom.queryBars.set(window.id, queryBar);

      // Set initial value
      const iface = state.interfaces.get(window.id);
      if (iface) input.value = iface.queryStr;
    }

    // Position query bar exactly matching window width, above window
    const { x, y, w } = window.bounds;
    queryBar.style.left = `${x}px`;
    queryBar.style.top = `${y - height}px`;
    queryBar.style.width = `${w}px`;
    queryBar.style.zIndex = String(occlusion.getZIndex(window.id) + 1);
  } else if (queryBar) {
    queryBar.remove();
    dom.queryBars.delete(window.id);
  }
}

async function updateInterfaceQuery(windowId: AX.WindowId, queryStr: string) {
  const iface = state.interfaces.get(windowId);
  if (!iface) return;

  const trimmed = queryStr.trim();
  if (trimmed === iface.queryStr) return;

  iface.queryStr = trimmed;

  // Re-find observed root
  const rootElement = allio.getRootElement(windowId);
  if (rootElement) {
    const findMatch = trimmed.match(/^\(([^)]+)\)/);
    if (findMatch) {
      const { findFirst } = await import("allio");
      const found = findFirst(allio, rootElement.id, findMatch[1].trim());
      if (found && found.id !== iface.observedRootId) {
        if (iface.observedRootId) await allio.unobserve(iface.observedRootId);
        iface.observedRootId = found.id;
        await allio.observe(found.id, { depth: 10, wait_between_ms: 100 });
      }
    }
  }

  // Execute query immediately
  const results = executeInterfaceQuery(iface);
  console.log("[interface] Query updated:", results);
  if (iface.outputPortId) {
    const port = state.ports.get(iface.outputPortId);
    if (port) propagateValueFromPort(port, results);
  }
}

/** Make a window into a queried window (shift+click in creation mode) */
async function makeWindowQueried(windowId: AX.WindowId) {
  if (state.interfaces.has(windowId)) return; // Already queried

  const window = allio.windows.get(windowId);
  if (!window) return;

  // Fetch root element from OS (not just local cache)
  const rootElement = await allio.windowRoot(windowId);
  if (!rootElement) return;

  // Create interface with default query (finds tree, then extracts from listitems)
  const defaultQuery = "(tree) listitem { checkbox:completed textfield:text }";
  const iface: WindowInterface = {
    windowId,
    queryStr: defaultQuery,
    observedRootId: rootElement.id,
  };

  // Create output port
  const portId = crypto.randomUUID();
  const port: Port = {
    id: portId,
    windowId,
    element: rootElement,
    type: "output",
    isTransform: false,
    treeElementId: rootElement.id,
  };

  state.ports.set(portId, port);
  iface.outputPortId = portId;
  createPortElement(port);

  state.interfaces.set(windowId, iface);

  // Start observing
  await allio.observe(rootElement.id, { depth: 10, wait_between_ms: 100 });
  console.log("[interface] Window queried:", windowId);

  renderAll();
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
    border: 2px solid var(--accent);
    border-radius: 4px;
    background: rgba(91, 155, 213, 0.15);
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
    port.element = await allio.getElement(port.element.id, "current");
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
    borderColor: "var(--accent)",
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

function buildInfoPanelHtml(
  element: AX.TypedElement,
  isTransform: boolean
): string {
  const lines: string[] = [];

  lines.push(
    `<div style="color: var(--accent); font-weight: 600; margin-bottom: 4px;">${element.role} <span style="opacity: 0.5">(${element.platform_role})</span></div>`
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
      `<div><span style="opacity: 0.6;">Value:</span> <span style="color: var(--accent);">${escapeHtml(
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
  dom.wiringPath.setAttribute("stroke", "var(--accent)");
  dom.wiringSvg.style.display = "block";
}

function clearHoverOverlay() {
  if (dom.hoverOverlay) dom.hoverOverlay.style.display = "none";
  if (dom.infoPanel) dom.infoPanel.style.display = "none";
  if (dom.wiringSvg) dom.wiringSvg.style.display = "none";
}

// --- Value Propagation ---

function handleElementUpdate(element: AX.TypedElement) {
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
    const freshElement = await allio.getElement(
      inputPort.element.id,
      "current"
    );
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
    const freshElement = await allio.getElement(
      targetPort.element.id,
      "current"
    );
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

/**
 * Parse transform code. The code is treated as the body of a function
 * where the incoming value is available as `from`.
 *
 * Examples:
 *   from.filter(t => t.completed)
 *
 *   const mark = t => t.completed ? "[X]" : "[ ]"
 *   return from.map(t => `- ${mark(t)}`)
 *
 * If code doesn't have a return statement, the last expression is returned.
 */
function parseTransformFunction(code: string): (from: unknown) => unknown {
  const trimmed = code.trim();

  // If no explicit return, wrap as expression
  const hasReturn = /\breturn\b/.test(trimmed);
  const body = hasReturn ? trimmed : `return (${trimmed})`;

  return new Function("from", body) as (from: unknown) => unknown;
}

/** Parse a value into a Color object (0.0-1.0 RGBA components) */
function parseColorValue(value: unknown): AX.Color | null {
  // Already a Color object with r, g, b properties
  if (
    value &&
    typeof value === "object" &&
    "r" in value &&
    "g" in value &&
    "b" in value
  ) {
    const c = value as { r: number; g: number; b: number; a?: number };
    return { r: c.r, g: c.g, b: c.b, a: c.a ?? 1.0 };
  }

  if (typeof value === "string") {
    // Parse rgba(r, g, b, a) or rgb(r, g, b) CSS format (0-255 integers)
    const rgbaMatch = value.match(
      /rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)/
    );
    if (rgbaMatch) {
      return {
        r: parseInt(rgbaMatch[1], 10) / 255,
        g: parseInt(rgbaMatch[2], 10) / 255,
        b: parseInt(rgbaMatch[3], 10) / 255,
        a: rgbaMatch[4] ? parseFloat(rgbaMatch[4]) : 1.0,
      };
    }

    // Parse #RRGGBB or #RRGGBBAA hex format
    const hexMatch = value.match(
      /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})?$/i
    );
    if (hexMatch) {
      return {
        r: parseInt(hexMatch[1], 16) / 255,
        g: parseInt(hexMatch[2], 16) / 255,
        b: parseInt(hexMatch[3], 16) / 255,
        a: hexMatch[4] ? parseInt(hexMatch[4], 16) / 255 : 1.0,
      };
    }

    // Parse #RGB short hex format
    const shortHexMatch = value.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i);
    if (shortHexMatch) {
      return {
        r: parseInt(shortHexMatch[1] + shortHexMatch[1], 16) / 255,
        g: parseInt(shortHexMatch[2] + shortHexMatch[2], 16) / 255,
        b: parseInt(shortHexMatch[3] + shortHexMatch[3], 16) / 255,
        a: 1.0,
      };
    }
  }

  return null;
}

/** Format a value as a CSS color string if it's a color, otherwise stringify */
function formatColorAsString(value: unknown): string {
  // Check if it's a Color object with r, g, b properties (0.0-1.0 range)
  if (
    value &&
    typeof value === "object" &&
    "r" in value &&
    "g" in value &&
    "b" in value
  ) {
    const c = value as { r: number; g: number; b: number; a?: number };
    const r = Math.round(c.r * 255);
    const g = Math.round(c.g * 255);
    const b = Math.round(c.b * 255);
    const a = c.a ?? 1.0;
    return a === 1.0 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${a})`;
  }

  // For arrays and plain objects, JSON stringify
  if (Array.isArray(value) || (typeof value === "object" && value !== null)) {
    return JSON.stringify(value, null, 2);
  }

  return String(value);
}

async function writeValueToElement(element: AX.TypedElement, value: unknown) {
  try {
    if (accepts(element, "string")) {
      // Format value appropriately (colors as CSS, arrays as JSON, etc.)
      const stringValue = formatColorAsString(value);
      await allio.set(element, stringValue);
    } else if (accepts(element, "number")) {
      await allio.set(element, Number(value));
    } else if (accepts(element, "boolean")) {
      await allio.set(element, Boolean(value));
    } else if (accepts(element, "color")) {
      const color = parseColorValue(value);
      if (color) await allio.set(element, color);
    }
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
