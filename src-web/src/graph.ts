/**
 * AX Graph - Force-directed graph visualization of accessibility tree
 *
 * Tests the lazy linking and cleanup logic by visualizing:
 * - Element registration via click
 * - Parent/child relationships
 * - Orphan → linked transitions
 * - Element removal on view changes
 */

import { AXIO, AXElement, ElementId, AxioPassthrough } from "@axio/client";
import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCollide,
  Simulation,
  SimulationNodeDatum,
  SimulationLinkDatum,
} from "d3-force";

// ============================================================================
// Types
// ============================================================================

interface GraphNode extends SimulationNodeDatum {
  id: ElementId;
  element: AXElement;
}

interface GraphLink extends SimulationLinkDatum<GraphNode> {
  source: GraphNode;
  target: GraphNode;
  type: "parent-child" | "pending";
}

// ============================================================================
// Graph State
// ============================================================================

class AXGraph {
  private axio: AXIO;
  // @ts-expect-error passthrough must stay in scope to keep mouse listener active
  private passthrough: AxioPassthrough;
  private svg: SVGSVGElement;
  private container: HTMLElement;

  private nodes: Map<ElementId, GraphNode> = new Map();
  private links: GraphLink[] = [];
  private simulation: Simulation<GraphNode, GraphLink>;

  private width = 0;
  private height = 0;

  // Hover overlay state
  private hoveredNode: GraphNode | null = null;
  private boundsOverlay: HTMLElement | null = null;
  private wiringLine: SVGLineElement | null = null;
  private elementInfoPanel: HTMLElement | null = null;
  private elementDetailsEl: HTMLElement | null = null;

  constructor() {
    this.container = document.getElementById("graph-container")!;
    this.svg = document.getElementById("graph") as unknown as SVGSVGElement;
    this.elementInfoPanel = document.getElementById("element-info");
    this.elementDetailsEl = document.getElementById("element-details");
    this.axio = new AXIO();
    // Declarative passthrough: elements with ax-io="opaque" capture, rest passes through
    this.passthrough = new AxioPassthrough(this.axio);

    this.createHoverOverlay();
    this.updateDimensions();
    this.simulation = this.createSimulation();

    this.init();
  }

  private createHoverOverlay() {
    // Bounds overlay - shows element rectangle on screen
    this.boundsOverlay = document.createElement("div");
    this.boundsOverlay.style.cssText = `
      position: fixed;
      pointer-events: none;
      border: 2px solid #4fc3f7;
      border-radius: 4px;
      background: rgba(79, 195, 247, 0.1);
      box-sizing: border-box;
      display: none;
      z-index: 1000;
    `;
    document.body.appendChild(this.boundsOverlay);

    // Wiring line - connects bounds to node (rendered in SVG)
    this.wiringLine = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "line"
    );
    this.wiringLine.setAttribute("stroke", "#4fc3f7");
    this.wiringLine.setAttribute("stroke-width", "2");
    this.wiringLine.setAttribute("stroke-dasharray", "6,4");
    this.wiringLine.setAttribute("opacity", "0.7");
    this.wiringLine.style.display = "none";
    this.wiringLine.style.pointerEvents = "none";
  }

  private updateDimensions() {
    const rect = this.container.getBoundingClientRect();
    this.width = rect.width;
    this.height = rect.height;
  }

  private createSimulation(): Simulation<GraphNode, GraphLink> {
    const padding = 20;
    return forceSimulation<GraphNode>()
      .force(
        "link",
        forceLink<GraphNode, GraphLink>()
          .id((d) => String(d.id))
          .distance(60)
          .strength(0.5)
      )
      .force("charge", forceManyBody().strength(-150))
      .force("collide", forceCollide<GraphNode>().radius(25))
      .on("tick", () => {
        // Constrain nodes to screen bounds
        for (const node of this.nodes.values()) {
          if (node.fx === null || node.fx === undefined) {
            node.x = Math.max(
              padding,
              Math.min(this.width - padding, node.x ?? 0)
            );
          }
          if (node.fy === null || node.fy === undefined) {
            node.y = Math.max(
              padding,
              Math.min(this.height - padding, node.y ?? 0)
            );
          }
        }
        this.render();
      });
  }

  private async init() {
    // Connect to AXIO
    await this.axio.connect();

    // Listen for element events
    this.axio.on("element:added", (data) => {
      this.onElementAdded(data.element);
    });

    this.axio.on("element:changed", (data) => {
      this.onElementChanged(data.element);
    });

    this.axio.on("element:removed", (data) => {
      this.onElementRemoved(data.element_id);
    });

    // Click on SVG background to register element at that point
    this.svg.addEventListener("click", async (e) => {
      // Only handle clicks directly on SVG (not on nodes)
      if (e.target !== this.svg) return;

      try {
        const element = await this.axio.elementAt(e.clientX, e.clientY);
        const { isNew } = this.addNode(element);
        if (isNew) this.toast(`+ ${element.role}`);
      } catch (err) {
        this.toast(`elementAt: ${err}`, true);
      }
    });

    // Handle window resize
    window.addEventListener("resize", () => {
      this.updateDimensions();
    });

    // Initial render
    this.render();
    this.updateStats();
  }

  // ============================================================================
  // Event Handlers
  // ============================================================================

  /** Add a node to the graph. Returns { node, isNew } */
  private addNode(element: AXElement): { node: GraphNode; isNew: boolean } {
    // Update existing node
    if (this.nodes.has(element.id)) {
      const node = this.nodes.get(element.id)!;
      node.element = element;
      return { node, isNew: false };
    }

    // Create new node
    const node: GraphNode = {
      id: element.id,
      element,
      x: this.width / 2 + (Math.random() - 0.5) * 100,
      y: this.height / 2 + (Math.random() - 0.5) * 100,
    };

    this.nodes.set(element.id, node);
    this.markDirty();

    // Create link to parent if exists
    this.updateLinks();

    // Restart simulation
    this.restartSimulation();
    this.updateStats();

    return { node, isNew: true };
  }

  private onElementAdded(element: AXElement) {
    const { isNew } = this.addNode(element);
    if (isNew) {
      this.toast(`+ ${element.role} [${this.parentState(element)}]`);
    }
  }

  private onElementChanged(element: AXElement) {
    const node = this.nodes.get(element.id);
    if (!node) return;

    const oldParentState = this.parentState(node.element);
    node.element = element;

    // Re-check links (parent may have changed)
    this.updateLinks();

    // Update node appearance (class might have changed)
    const nodeEl = this.nodeElements.get(element.id);
    if (nodeEl) {
      nodeEl.setAttribute("class", `node ${this.getNodeClass(node)}`);
    }

    this.updateStats();

    // Update element info if this is the hovered node
    if (this.hoveredNode?.id === element.id) {
      this.updateElementInfo(node);
    }

    // Show if parent state changed (orphan → linked, etc)
    const newParentState = this.parentState(element);
    if (oldParentState !== newParentState) {
      this.toast(`Δ ${element.role}: ${oldParentState} → ${newParentState}`);
    }
  }

  private onElementRemoved(elementId: ElementId) {
    const node = this.nodes.get(elementId);
    if (!node) return;

    const role = node.element.role;
    this.nodes.delete(elementId);

    // Remove links involving this node
    this.links = this.links.filter(
      (l) => l.source.id !== elementId && l.target.id !== elementId
    );

    this.markDirty();
    this.restartSimulation();
    this.updateStats();

    this.toast(`− ${role}`);
  }

  // ============================================================================
  // Link Management
  // ============================================================================

  private updateLinks() {
    const newLinks: GraphLink[] = [];

    for (const node of this.nodes.values()) {
      const parentId = node.element.parent_id;
      if (parentId !== null) {
        const parentNode = this.nodes.get(parentId);
        if (parentNode) {
          // Check if link already exists
          const exists = this.links.some(
            (l) => l.source.id === parentId && l.target.id === node.id
          );
          if (!exists) {
            newLinks.push({
              source: parentNode,
              target: node,
              type: "parent-child",
            });
          }
        }
      }
    }

    // Add new links and mark for rebuild
    if (newLinks.length > 0) {
      this.links.push(...newLinks);
      this.markDirty();
    }

    // Update simulation
    const linkForce = this.simulation.force("link") as ReturnType<
      typeof forceLink
    >;
    if (linkForce) {
      linkForce.links(this.links);
    }
  }

  private restartSimulation() {
    this.simulation.nodes(Array.from(this.nodes.values()));

    const linkForce = this.simulation.force("link") as ReturnType<
      typeof forceLink
    >;
    if (linkForce) {
      linkForce.links(this.links);
    }

    this.simulation.alpha(0.5).restart();
  }

  // ============================================================================
  // Node Interaction
  // ============================================================================

  /** Fetch children + parent for node */
  private async expandNode(node: GraphNode) {
    try {
      let newCount = 0;
      let existingCount = 0;

      // Fetch children
      const children = await this.axio.children(node.id);
      for (const child of children) {
        const { isNew } = this.addNode(child);
        if (isNew) newCount++;
        else existingCount++;
      }

      // Fetch parent (if not root)
      if (!node.element.is_root) {
        const parent = await this.axio.parent(node.id);
        if (parent) {
          const { isNew } = this.addNode(parent);
          if (isNew) newCount++;
          else existingCount++;
        }
      }

      // Honest toast: show what actually happened
      const parts = [];
      if (newCount > 0) parts.push(`+${newCount}`);
      if (existingCount > 0) parts.push(`=${existingCount}`);
      this.toast(parts.length > 0 ? parts.join(" ") : "∅");
    } catch (err) {
      this.toast(`expand: ${err}`, true);
    }

    this.updateLinks();
    this.restartSimulation();
  }

  // ============================================================================
  // Rendering
  // ============================================================================

  // SVG element references for efficient updates
  private linkGroup: SVGGElement | null = null;
  private nodeGroup: SVGGElement | null = null;
  private nodeElements: Map<ElementId, SVGGElement> = new Map();
  private linkElements: SVGLineElement[] = [];
  private needsRebuild = true;

  /** Mark that SVG elements need to be rebuilt (nodes/links changed) */
  private markDirty() {
    this.needsRebuild = true;
  }

  /** Called on every simulation tick - only updates positions */
  private render() {
    // Hide all overlays if in passthrough mode
    if (this.axio.passthrough) {
      this.hideBoundsOverlay();
      this.hideElementInfo();
    }

    // Rebuild SVG elements only when structure changed
    if (this.needsRebuild) {
      this.rebuildSVG();
      this.needsRebuild = false;
    }

    // Fast path: just update positions
    this.updatePositions();
  }

  /** Rebuild SVG elements (called only when nodes/links change) */
  private rebuildSVG() {
    // Clear SVG
    this.svg.innerHTML = "";
    this.nodeElements.clear();
    this.linkElements = [];

    // Create defs for arrow markers
    const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
    defs.innerHTML = `
      <marker id="arrow" viewBox="0 -5 10 10" refX="20" refY="0" markerWidth="6" markerHeight="6" orient="auto">
        <path d="M0,-5L10,0L0,5" fill="rgba(78, 205, 196, 0.4)" />
      </marker>
    `;
    this.svg.appendChild(defs);

    // Create link group
    this.linkGroup = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "g"
    );
    this.linkGroup.setAttribute("class", "links");

    for (const link of this.links) {
      const line = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "line"
      );
      line.setAttribute("class", `link ${link.type}`);
      line.setAttribute("marker-end", "url(#arrow)");
      this.linkElements.push(line);
      this.linkGroup.appendChild(line);
    }
    this.svg.appendChild(this.linkGroup);

    // Add wiring line BEFORE nodes (renders behind)
    if (this.wiringLine) {
      this.svg.appendChild(this.wiringLine);
    }

    // Create node group
    this.nodeGroup = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "g"
    );
    this.nodeGroup.setAttribute("class", "nodes");

    for (const node of this.nodes.values()) {
      const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
      const nodeClass = this.getNodeClass(node);
      g.setAttribute("class", `node ${nodeClass}`);
      g.setAttribute("data-id", String(node.id));
      g.setAttribute("ax-io", "opaque"); // Capture clicks, don't pass through
      g.style.pointerEvents = "all";
      g.style.cursor = "pointer";

      // Circle
      const circle = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "circle"
      );
      circle.setAttribute("r", "12");
      circle.style.pointerEvents = "all";
      g.appendChild(circle);

      // Label
      const text = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "text"
      );
      text.setAttribute("dy", "24");
      text.textContent = this.getNodeLabel(node);
      g.appendChild(text);

      // Hover to show bounds overlay
      g.addEventListener("mouseenter", () => this.onNodeHoverEnter(node));
      g.addEventListener("mouseleave", () => this.onNodeHoverLeave());

      // Drag behavior (also handles click detection)
      this.addDragBehavior(g, node);

      this.nodeElements.set(node.id, g);
      this.nodeGroup.appendChild(g);
    }

    this.svg.appendChild(this.nodeGroup);
  }

  /** Fast position update - called on every tick */
  private updatePositions() {
    // Update link positions
    this.links.forEach((link, i) => {
      const line = this.linkElements[i];
      if (line) {
        line.setAttribute("x1", String(link.source.x ?? 0));
        line.setAttribute("y1", String(link.source.y ?? 0));
        line.setAttribute("x2", String(link.target.x ?? 0));
        line.setAttribute("y2", String(link.target.y ?? 0));
      }
    });

    // Update node positions
    for (const node of this.nodes.values()) {
      const g = this.nodeElements.get(node.id);
      if (g) {
        g.setAttribute(
          "transform",
          `translate(${node.x ?? 0}, ${node.y ?? 0})`
        );
      }
    }

    // Update wiring line if visible
    this.updateWiringLine();
  }

  private getNodeClass(node: GraphNode): string {
    return this.parentState(node.element);
  }

  /** Get parent state as a string for display */
  private parentState(el: AXElement): "root" | "linked" | "orphan" {
    if (el.is_root) return "root";
    if (el.parent_id !== null) return "linked";
    return "orphan";
  }

  private getNodeLabel(node: GraphNode): string {
    // Show full role - don't hide data
    return node.element.role.toLowerCase();
  }

  private addDragBehavior(g: SVGGElement, node: GraphNode) {
    let dragging = false;
    let hasMoved = false;
    let startMouseX = 0;
    let startMouseY = 0;
    let startNodeX = 0;
    let startNodeY = 0;

    g.addEventListener("mousedown", (e) => {
      dragging = true;
      hasMoved = false;
      startMouseX = e.clientX;
      startMouseY = e.clientY;
      startNodeX = node.x ?? 0;
      startNodeY = node.y ?? 0;
      e.preventDefault();
      e.stopPropagation();
    });

    const onMouseMove = (e: MouseEvent) => {
      if (!dragging) return;

      // Detect if we've moved more than a few pixels (actual drag vs click)
      const dx = e.clientX - startMouseX;
      const dy = e.clientY - startMouseY;
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) {
        if (!hasMoved) {
          hasMoved = true;
          this.simulation.alphaTarget(0.3).restart();
        }
      }

      if (hasMoved) {
        node.fx = startNodeX + dx;
        node.fy = startNodeY + dy;
      }
    };

    const onMouseUp = () => {
      if (!dragging) return;
      dragging = false;

      if (hasMoved) {
        // Was a drag - release fixed position
        node.fx = null;
        node.fy = null;
        this.simulation.alphaTarget(0);
      } else {
        // Was a click - expand node
        console.log("node click", node.element.role, node.id);
        this.expandNode(node);
      }
    };

    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }

  // ============================================================================
  // Hover Overlay - Shows element bounds on screen
  // ============================================================================

  private async onNodeHoverEnter(node: GraphNode) {
    this.hoveredNode = node;

    // Always show element info in panel
    this.updateElementInfo(node);

    // Refresh element data to get latest bounds
    try {
      const fresh = await this.axio.refresh(node.element.id);
      node.element = fresh;
    } catch {
      // Element might be gone, use cached data
    }

    // Only show if still hovering this node
    if (this.hoveredNode === node) {
      this.updateElementInfo(node);
      // Only show bounds overlay if not in passthrough mode
      if (!this.axio.passthrough) {
        this.showBoundsOverlay(node);
      }
    }
  }

  private onNodeHoverLeave() {
    this.hoveredNode = null;
    this.hideBoundsOverlay();
    this.hideElementInfo();
  }

  private showBoundsOverlay(node: GraphNode) {
    const bounds = node.element.bounds;
    if (!bounds || !this.boundsOverlay || !this.wiringLine) return;

    // Show bounds rectangle
    const { x, y, w, h } = bounds;
    this.boundsOverlay.style.left = `${x}px`;
    this.boundsOverlay.style.top = `${y}px`;
    this.boundsOverlay.style.width = `${w}px`;
    this.boundsOverlay.style.height = `${h}px`;
    this.boundsOverlay.style.display = "block";

    // Draw wiring line from bounds center to node
    this.updateWiringLine();
    this.wiringLine.style.display = "block";
  }

  private hideBoundsOverlay() {
    if (this.boundsOverlay) this.boundsOverlay.style.display = "none";
    if (this.wiringLine) this.wiringLine.style.display = "none";
  }

  private updateWiringLine() {
    // Update wiring line position during simulation ticks
    if (!this.hoveredNode || !this.wiringLine) return;
    const node = this.hoveredNode;
    const bounds = node.element.bounds;
    if (!bounds) return;

    const containerRect = this.container.getBoundingClientRect();
    const boundsCenter = {
      x: bounds.x + bounds.w / 2,
      y: bounds.y + bounds.h / 2,
    };

    this.wiringLine.setAttribute(
      "x1",
      String(boundsCenter.x - containerRect.left)
    );
    this.wiringLine.setAttribute(
      "y1",
      String(boundsCenter.y - containerRect.top)
    );
    this.wiringLine.setAttribute("x2", String(node.x ?? 0));
    this.wiringLine.setAttribute("y2", String(node.y ?? 0));
  }

  // ============================================================================
  // Element Info Panel
  // ============================================================================

  private updateElementInfo(node: GraphNode) {
    if (!this.elementInfoPanel || !this.elementDetailsEl) return;

    const el = node.element;
    const row = (label: string, value: string) =>
      `<div class="detail-row"><span class="detail-label">${label}</span><span class="detail-value">${value}</span></div>`;

    const rows: string[] = [];

    rows.push(row("id", String(el.id)));
    rows.push(row("role", el.role));
    rows.push(row("platform", el.platform_role));
    rows.push(
      row(
        "parent",
        el.parent_id !== null
          ? `linked (${el.parent_id})`
          : el.is_root
          ? "root"
          : "orphan"
      )
    );
    if (el.children !== null)
      rows.push(
        row("children", el.children.length > 0 ? el.children.join(", ") : "[]")
      );
    if (el.label) rows.push(row("label", this.escapeHtml(el.label)));
    if (el.value)
      rows.push(
        row(
          "value",
          `${el.value.type}: ${this.escapeHtml(String(el.value.value))}`
        )
      );
    if (el.description) rows.push(row("desc", this.escapeHtml(el.description)));
    if (el.placeholder)
      rows.push(row("placeholder", this.escapeHtml(el.placeholder)));
    if (el.url) rows.push(row("url", this.escapeHtml(el.url)));
    if (el.focused !== null) rows.push(row("focused", String(el.focused)));
    if (el.disabled) rows.push(row("disabled", "true"));
    if (el.selected !== null) rows.push(row("selected", String(el.selected)));
    if (el.expanded !== null) rows.push(row("expanded", String(el.expanded)));
    if (el.row_index !== null)
      rows.push(row("row_index", String(el.row_index)));
    if (el.column_index !== null)
      rows.push(row("column_index", String(el.column_index)));
    if (el.row_count !== null)
      rows.push(row("row_count", String(el.row_count)));
    if (el.column_count !== null)
      rows.push(row("column_count", String(el.column_count)));
    if (el.actions.length > 0) rows.push(row("actions", el.actions.join(", ")));

    this.elementDetailsEl.innerHTML = rows.join("");
    this.elementInfoPanel.style.display = "block";
  }

  private hideElementInfo() {
    if (this.elementInfoPanel) this.elementInfoPanel.style.display = "none";
  }

  private escapeHtml(str: string): string {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // ============================================================================
  // Stats & UI
  // ============================================================================

  private updateStats() {
    const nodes = this.nodes.size;
    const roots = Array.from(this.nodes.values()).filter(
      (n) => n.element.is_root
    ).length;
    const linked = Array.from(this.nodes.values()).filter(
      (n) => !n.element.is_root && n.element.parent_id !== null
    ).length;
    const orphans = Array.from(this.nodes.values()).filter(
      (n) => !n.element.is_root && n.element.parent_id === null
    ).length;

    document.getElementById("stat-nodes")!.textContent = String(nodes);
    document.getElementById("stat-roots")!.textContent = String(roots);
    document.getElementById("stat-linked")!.textContent = String(linked);
    document.getElementById("stat-orphans")!.textContent = String(orphans);
    document.getElementById("stat-edges")!.textContent = String(
      this.links.length
    );
  }

  private toast(message: string, isError = false) {
    const toast = document.getElementById("toast")!;
    toast.textContent = message;
    toast.className = isError ? "show error" : "show";

    setTimeout(() => {
      toast.className = "";
    }, 2000);
  }
}

// ============================================================================
// Initialize
// ============================================================================

new AXGraph();
