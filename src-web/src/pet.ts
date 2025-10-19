import { AXIO, AXNode } from "./axio.js";
import { Gizmos } from "./gizmos.js";

// ============================================================================
// Geometry Types
// ============================================================================

interface Vec2 {
  x: number;
  y: number;
}

interface WindowGeometry {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

// Character size constants
const CHARACTER_RADIUS = 32; // Half of character width (64px total)
const MIN_PLATFORM_WIDTH = 1; // Minimum walkable space (1 pixel)

// Jump parameters (arcade-style, mutable for UI controls)
let MAX_JUMP_DISTANCE = 200; // Maximum horizontal jump distance in pixels
let JUMP_ARC_HEIGHT = 70; // How high the jump arc peaks above the start point

// Step parameters
const MAX_STEP_GAP = CHARACTER_RADIUS * 3; // Max horizontal gap for stepping
const MAX_STEP_HEIGHT = CHARACTER_RADIUS; // Max height difference for stepping

// Drop parameters
const MAX_DROP_GAP = CHARACTER_RADIUS * 2; // Max horizontal gap for dropping
const MAX_DROP_HEIGHT = CHARACTER_RADIUS * 8; // Max vertical distance to drop down

// ============================================================================
// Navmesh Types
// ============================================================================

type EdgeType = "walk" | "climb" | "fall" | "jump" | "step" | "drop";

interface NavNode {
  id: string;
  pos: Vec2;
  type: "platform_left" | "platform_right" | "platform_center";
  windowId: string;
  componentId?: number; // Which connected component (via walk/step) this node belongs to
}

interface NavEdge {
  id: string;
  from: string; // node id
  to: string; // node id
  type: EdgeType;
  cost: number;
}

class Navmesh {
  nodes: Map<string, NavNode> = new Map();
  edges: Map<string, NavEdge> = new Map();

  clear(): void {
    this.nodes.clear();
    this.edges.clear();
  }

  addNode(node: NavNode): void {
    this.nodes.set(node.id, node);
  }

  addEdge(edge: NavEdge): void {
    this.edges.set(edge.id, edge);
  }

  getNode(id: string): NavNode | undefined {
    return this.nodes.get(id);
  }

  getEdgesFromNode(nodeId: string): NavEdge[] {
    return Array.from(this.edges.values()).filter(
      (e) => e.from === nodeId || e.to === nodeId
    );
  }
}

// ============================================================================
// Navmesh Builder
// ============================================================================

class NavmeshBuilder {
  #windows: WindowGeometry[] = [];

  build(windows: WindowGeometry[]): Navmesh {
    this.#windows = windows;
    const navmesh = new Navmesh();

    // For each window, analyze it as a platform
    windows.forEach((win) => {
      const platformY = win.y - CHARACTER_RADIUS;

      // Find obstacles that sit on top of this platform
      const obstacles = this.#findObstaclesOnPlatform(win, windows);

      // Split the platform into walkable segments (gaps between obstacles)
      const segments = this.#splitPlatformIntoSegments(
        win.x + CHARACTER_RADIUS,
        win.x + win.width - CHARACTER_RADIUS,
        obstacles
      );

      // Create nodes for each walkable segment
      segments.forEach((segment, segIdx) => {
        if (segment.width < MIN_PLATFORM_WIDTH) {
          return; // Skip segments too narrow
        }

        // Try to find valid positions by pushing inward if needed
        const leftPos = this.#findValidPosition(
          { x: segment.left, y: platformY },
          "right",
          segment.right - MIN_PLATFORM_WIDTH / 2,
          win.id
        );

        const rightPos = this.#findValidPosition(
          { x: segment.right, y: platformY },
          "left",
          segment.left + MIN_PLATFORM_WIDTH / 2,
          win.id
        );

        if (!leftPos || !rightPos) {
          return;
        }

        if (rightPos.x - leftPos.x < MIN_PLATFORM_WIDTH) {
          return;
        }

        // Create nodes for this segment
        const leftNode: NavNode = {
          id: `${win.id}_seg${segIdx}_left`,
          pos: leftPos,
          type: "platform_left",
          windowId: win.id,
        };
        const rightNode: NavNode = {
          id: `${win.id}_seg${segIdx}_right`,
          pos: rightPos,
          type: "platform_right",
          windowId: win.id,
        };

        navmesh.addNode(leftNode);
        navmesh.addNode(rightNode);

        // Walk edge within this segment (doesn't go through obstacles)
        const walkEdge: NavEdge = {
          id: `${win.id}_seg${segIdx}_walk`,
          from: leftNode.id,
          to: rightNode.id,
          type: "walk",
          cost: rightPos.x - leftPos.x,
        };
        navmesh.addEdge(walkEdge);
      });
    });

    // Add step edges between nearby nodes (has precedence over jump)
    this.#addStepEdges(navmesh);

    // Add drop edges for dropping down from platforms
    this.#addDropEdges(navmesh);

    // Assign connected component IDs to nodes (via walk/step/drop edges)
    this.#assignComponentIds(navmesh);

    // Add jump edges only between nodes in different components
    this.#addJumpEdges(navmesh);

    return navmesh;
  }

  /**
   * Add step edges between nearby nodes at similar heights
   */
  #addStepEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    const stepPairs = new Set<string>();
    let stepCount = 0;

    for (const fromNode of nodes) {
      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue; // Skip same window

        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y;
        const horizontalDist = Math.abs(dx);
        const heightDiff = Math.abs(dy);

        // Check if within step range
        if (horizontalDist > MAX_STEP_GAP) continue;
        if (heightDiff > MAX_STEP_HEIGHT) continue;

        // Create bidirectional step edge (only once per pair)
        const pairId = [fromNode.id, toNode.id].sort().join(":");
        if (stepPairs.has(pairId)) continue;

        stepPairs.add(pairId);

        // Add bidirectional step edges
        const straightDistance = Math.hypot(dx, dy);
        navmesh.addEdge({
          id: `step_${fromNode.id}_to_${toNode.id}`,
          from: fromNode.id,
          to: toNode.id,
          type: "step",
          cost: straightDistance,
        });
        navmesh.addEdge({
          id: `step_${toNode.id}_to_${fromNode.id}`,
          from: toNode.id,
          to: fromNode.id,
          type: "step",
          cost: straightDistance,
        });
        stepCount += 2;
      }
    }

    console.log(
      `[NavDemo] Added ${stepCount} step edges (${stepPairs.size} pairs)`
    );
  }

  /**
   * Add drop edges for dropping down from platforms
   * Unidirectional: you can drop down but not up
   */
  #addDropEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let dropCount = 0;

    for (const fromNode of nodes) {
      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue; // Skip same window

        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y; // Positive = toNode is below fromNode
        const horizontalDist = Math.abs(dx);

        // Must be dropping DOWN (toNode.y > fromNode.y, so dy > 0)
        if (dy <= 0) continue;

        // Check if within drop range
        if (horizontalDist > MAX_DROP_GAP) continue;
        if (dy > MAX_DROP_HEIGHT) continue;

        // Add unidirectional drop edge
        const straightDistance = Math.hypot(dx, dy);
        navmesh.addEdge({
          id: `drop_${fromNode.id}_to_${toNode.id}`,
          from: fromNode.id,
          to: toNode.id,
          type: "drop",
          cost: straightDistance * 0.8, // Drops are slightly preferred (faster than walking around)
        });
        dropCount++;
      }
    }

    console.log(`[NavDemo] Added ${dropCount} drop edges`);
  }

  /**
   * Assign connected component IDs to all nodes based on walk/step connectivity
   * Nodes that can reach each other via walk/step edges get the same ID
   */
  #assignComponentIds(navmesh: Navmesh): void {
    let nextComponentId = 0;
    const nodes = Array.from(navmesh.nodes.values());

    for (const node of nodes) {
      // Skip if already assigned
      if (node.componentId !== undefined) continue;

      // Flood fill from this node
      const componentId = nextComponentId++;
      const queue = [node.id];
      const visited = new Set<string>();

      while (queue.length > 0) {
        const nodeId = queue.shift()!;
        if (visited.has(nodeId)) continue;
        visited.add(nodeId);

        const currentNode = navmesh.getNode(nodeId);
        if (!currentNode) continue;

        currentNode.componentId = componentId;

        // Add neighbors via walk/step/drop edges
        for (const edge of navmesh.edges.values()) {
          if (edge.from !== nodeId) continue;
          if (
            edge.type !== "walk" &&
            edge.type !== "step" &&
            edge.type !== "drop"
          )
            continue;
          queue.push(edge.to);
        }
      }
    }

    console.log(`[NavDemo] Found ${nextComponentId} connected components`);
  }

  /**
   * Add jump edges between nodes that can be jumped between
   * Only adds jumps between nodes in different connected components
   */
  #addJumpEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let jumpCount = 0;
    let totalTests = 0;
    let rejectedByDistance = 0;
    let rejectedByLine = 0;
    let rejectedByPhysics = 0;
    let rejectedByParabola = 0;
    let rejectedByComponent = 0;

    for (const fromNode of nodes) {
      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue; // Skip same window

        totalTests++;

        // Skip if in same connected component (already reachable via walk/step)
        if (fromNode.componentId === toNode.componentId) {
          rejectedByComponent++;
          continue;
        }
        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y;
        const horizontalDist = Math.abs(dx);

        // 1. Quick distance check - too far to jump horizontally
        if (horizontalDist > MAX_JUMP_DISTANCE) {
          rejectedByDistance++;
          continue;
        }

        // 2. Can't jump too high up (but can jump down any distance)
        const maxJumpUp = JUMP_ARC_HEIGHT * 0.5; // Can jump half the arc height upward
        if (dy < -maxJumpUp) {
          rejectedByPhysics++;
          continue;
        }

        // 3. Quick line check - does straight line intersect any windows?
        if (this.#lineIntersectsWindows(fromNode.pos, toNode.pos)) {
          rejectedByLine++;
          continue;
        }

        // 4. Verify the parabolic arc is clear
        if (!this.#isJumpArcClear(fromNode.pos, toNode.pos)) {
          rejectedByParabola++;
          continue;
        }

        // Add jump edge
        const straightDistance = Math.hypot(dx, dy);
        const jumpCost = straightDistance * 1.5; // Distance-based cost with penalty
        navmesh.addEdge({
          id: `jump_${fromNode.id}_to_${toNode.id}`,
          from: fromNode.id,
          to: toNode.id,
          type: "jump",
          cost: jumpCost,
        });
        jumpCount++;
      }
    }

    console.log(`[NavDemo] Jump edges: ${jumpCount}/${totalTests} tests`);
    console.log(`  - Rejected by component: ${rejectedByComponent}`);
    console.log(`  - Rejected by distance: ${rejectedByDistance}`);
    console.log(`  - Rejected by line: ${rejectedByLine}`);
    console.log(`  - Rejected by physics: ${rejectedByPhysics}`);
    console.log(`  - Rejected by parabola: ${rejectedByParabola}`);
  }

  /**
   * Quick check if a straight line intersects any window
   */
  #lineIntersectsWindows(start: Vec2, end: Vec2): boolean {
    const samples = 10;
    for (let i = 0; i <= samples; i++) {
      const t = i / samples;
      const x = start.x + (end.x - start.x) * t;
      const y = start.y + (end.y - start.y) * t;

      // Check if this point is inside any window
      for (const win of this.#windows) {
        if (
          x > win.x &&
          x < win.x + win.width &&
          y > win.y &&
          y < win.y + win.height
        ) {
          return true;
        }
      }
    }
    return false;
  }

  /**
   * Check if a simple parabolic jump arc is clear of obstacles
   * Uses a fixed arc height for arcade-style jumping
   */
  #isJumpArcClear(start: Vec2, end: Vec2): boolean {
    const samples = 20;

    for (let i = 0; i <= samples; i++) {
      const t = i / samples;

      // Linear interpolation for x
      const x = start.x + (end.x - start.x) * t;

      // Parabolic arc for y: goes up to arcHeight then down
      // Peak is at the highest point between start and end
      const highestY = Math.min(start.y, end.y) - JUMP_ARC_HEIGHT;
      const parabola = -4 * (t - 0.5) * (t - 0.5) + 1; // 0 to 1 to 0
      const y =
        start.y + (end.y - start.y) * t + (highestY - start.y) * parabola;

      // Check if this position collides with any window
      for (const win of this.#windows) {
        const closestX = Math.max(win.x, Math.min(x, win.x + win.width));
        const closestY = Math.max(win.y, Math.min(y, win.y + win.height));

        const distX = x - closestX;
        const distY = y - closestY;
        const distSq = distX * distX + distY * distY;

        if (distSq < CHARACTER_RADIUS * CHARACTER_RADIUS) {
          return false; // Path blocked
        }
      }
    }

    return true;
  }

  /**
   * Find obstacles on this platform's walkable surface
   * An obstacle is any window that intersects the "character zone" above the platform
   */
  #findObstaclesOnPlatform(
    platform: WindowGeometry,
    allWindows: WindowGeometry[]
  ): Array<{ left: number; right: number }> {
    const obstacles: Array<{ left: number; right: number }> = [];

    // Character zone: space where character would be when walking on this platform
    const characterZoneTop = platform.y - CHARACTER_RADIUS * 2; // Top of character circle
    const characterZoneBottom = platform.y; // Bottom at platform surface

    for (const win of allWindows) {
      if (win.id === platform.id) continue;

      // Check if this window intersects the character zone vertically
      const winTop = win.y;
      const winBottom = win.y + win.height;

      const verticalOverlap =
        winBottom > characterZoneTop && winTop < characterZoneBottom;

      if (verticalOverlap) {
        // Check horizontal overlap with platform
        const overlapLeft = Math.max(win.x, platform.x);
        const overlapRight = Math.min(
          win.x + win.width,
          platform.x + platform.width
        );

        if (overlapRight > overlapLeft) {
          // This window blocks part of the platform
          obstacles.push({ left: win.x, right: win.x + win.width });
        }
      }
    }

    return obstacles;
  }

  /**
   * Split a platform range into walkable segments by cutting around obstacles
   */
  #splitPlatformIntoSegments(
    platformLeft: number,
    platformRight: number,
    obstacles: Array<{ left: number; right: number }>
  ): Array<{ left: number; right: number; width: number }> {
    if (obstacles.length === 0) {
      // No obstacles, entire platform is one segment
      return [
        {
          left: platformLeft,
          right: platformRight,
          width: platformRight - platformLeft,
        },
      ];
    }

    // Sort obstacles by left edge
    const sortedObstacles = [...obstacles].sort((a, b) => a.left - b.left);

    const segments: Array<{ left: number; right: number; width: number }> = [];
    let currentLeft = platformLeft;

    for (const obstacle of sortedObstacles) {
      // Add character radius buffer around obstacle
      const obstacleLeft = obstacle.left - CHARACTER_RADIUS;
      const obstacleRight = obstacle.right + CHARACTER_RADIUS;

      // If there's space before this obstacle, create a segment
      if (currentLeft < obstacleLeft) {
        const segLeft = currentLeft;
        const segRight = Math.min(obstacleLeft, platformRight);
        if (segRight > segLeft) {
          segments.push({
            left: segLeft,
            right: segRight,
            width: segRight - segLeft,
          });
        }
      }

      // Move past this obstacle
      currentLeft = Math.max(currentLeft, obstacleRight);
    }

    // Add final segment after last obstacle
    if (currentLeft < platformRight) {
      segments.push({
        left: currentLeft,
        right: platformRight,
        width: platformRight - currentLeft,
      });
    }

    return segments;
  }

  /**
   * Find a valid position by pushing inward if overlapping
   */
  #findValidPosition(
    startPos: Vec2,
    pushDirection: "left" | "right",
    limit: number,
    excludeWindowId: string
  ): Vec2 | null {
    const step = 1; // Pixel increment for pushing
    const maxIterations = 500; // Safety limit
    let currentPos = { ...startPos };

    for (let i = 0; i < maxIterations; i++) {
      // Check if current position is valid
      if (!this.#isPositionInsideAnyWindow(currentPos, excludeWindowId)) {
        return currentPos;
      }

      // Push inward
      if (pushDirection === "right") {
        currentPos.x += step;
        if (currentPos.x >= limit) {
          return null; // Exceeded limit
        }
      } else {
        currentPos.x -= step;
        if (currentPos.x <= limit) {
          return null; // Exceeded limit
        }
      }
    }

    return null; // Failed to find valid position
  }

  /**
   * Check if a character circle at this position intersects ANY solid window geometry
   * Circles CAN overlap with other circles, but CANNOT intersect window rectangles
   */
  #isPositionInsideAnyWindow(pos: Vec2, excludeWindowId: string): boolean {
    for (const win of this.#windows) {
      if (win.id === excludeWindowId) continue;

      // Circle-rectangle intersection test
      // Find the closest point on the rectangle to the circle center
      const closestX = Math.max(win.x, Math.min(pos.x, win.x + win.width));
      const closestY = Math.max(win.y, Math.min(pos.y, win.y + win.height));

      // Calculate distance from circle center to this closest point
      const distanceX = pos.x - closestX;
      const distanceY = pos.y - closestY;
      const distanceSquared = distanceX * distanceX + distanceY * distanceY;

      // If distance is less than radius, circle intersects rectangle
      if (distanceSquared < CHARACTER_RADIUS * CHARACTER_RADIUS) {
        return true;
      }
    }

    return false;
  }
}

// ============================================================================
// Main Application
// ============================================================================

class PetApp {
  #axio: AXIO;
  #windows: WindowGeometry[] = [];
  #navmesh = new Navmesh();
  #navmeshBuilder = new NavmeshBuilder();
  #overlayPid: number | null = null;
  #gizmos: Gizmos;

  constructor() {
    this.#axio = new AXIO();
    const debugLayer = document.getElementById("debug-layer") as any;
    this.#gizmos = new Gizmos(debugLayer);

    this.#setupAXIO();
    this.#setupPhysicsControls();
  }

  async #setupAXIO(): Promise<void> {
    try {
      await this.#axio.connect();
      console.log("[NavDemo] Connected to AXIO");

      this.#axio.onOverlayPid((pid: number) => {
        this.#overlayPid = pid;
        console.log(`[NavDemo] Overlay PID: ${pid}`);
      });

      this.#axio.onWindowUpdate((axWindows: AXNode[]) => {
        // Convert AXNode windows to our geometry format
        this.#windows = axWindows
          .filter((w) => w.pid !== this.#overlayPid && w.bounds)
          .map((w) => ({
            id: w.id, // Use actual OS window ID for stability
            x: w.bounds!.position.x,
            y: w.bounds!.position.y,
            width: w.bounds!.size.width,
            height: w.bounds!.size.height,
          }));

        // Rebuild navmesh whenever windows change
        this.#rebuildNavmesh();
        this.#render();
        this.#updateUI();
      });

      await this.#axio.setClickthrough(true);
    } catch (error) {
      console.error("[NavDemo] Failed to connect:", error);
    }
  }

  #rebuildNavmesh(): void {
    console.log(
      `[NavDemo] Rebuilding navmesh for ${this.#windows.length} windows`
    );
    this.#navmesh = this.#navmeshBuilder.build(this.#windows);
    console.log(
      `[NavDemo] Built navmesh: ${this.#navmesh.nodes.size} nodes, ${
        this.#navmesh.edges.size
      } edges`
    );
  }

  #render(): void {
    console.log("[NavDemo] Rendering...");
    this.#gizmos.clear();
    this.#renderWindowGeometry();
    this.#renderNavmesh();
  }

  #renderWindowGeometry(): void {
    for (const win of this.#windows) {
      this.#gizmos.rect(win.x, win.y, win.width, win.height, {
        stroke: "rgba(255, 255, 255, 0.3)",
      });
      this.#gizmos.text(
        win.id,
        { x: win.x + 10, y: win.y + 20 },
        {
          fill: "rgba(255, 255, 255, 0.5)",
        }
      );
    }
  }

  #renderNavmesh(): void {
    const edgeColors = {
      walk: "rgba(0, 255, 0, 0.8)",
      climb: "rgba(100, 150, 255, 0.8)",
      fall: "rgba(255, 100, 100, 0.8)",
      jump: "rgba(255, 200, 0, 0.8)",
      step: "rgba(150, 255, 150, 0.8)",
      drop: "rgba(255, 150, 255, 0.8)", // Purple/magenta for drop edges
    };

    const nodeColors = {
      platform_left: "rgba(255, 200, 0, 0.9)",
      platform_right: "rgba(0, 200, 255, 0.9)",
      platform_center: "rgba(0, 255, 0, 0.9)",
    };

    // Draw character radius circles
    for (const node of this.#navmesh.nodes.values()) {
      this.#gizmos.circle(node.pos, CHARACTER_RADIUS, {
        fill: "rgba(100, 255, 100, 0.1)",
        stroke: "rgba(100, 255, 100, 0.3)",
        strokeWidth: 1,
        strokeDasharray: "4,4",
      });
    }

    // Draw edges
    for (const edge of this.#navmesh.edges.values()) {
      const fromNode = this.#navmesh.getNode(edge.from);
      const toNode = this.#navmesh.getNode(edge.to);
      if (!fromNode || !toNode) continue;

      const color = edgeColors[edge.type];

      // Jump edges get parabolic arcs (directional)
      if (edge.type === "jump") {
        this.#drawJumpArc(fromNode.pos, toNode.pos, color);
      } else if (edge.type === "step") {
        // Step edges are dashed lines (bidirectional)
        this.#gizmos.line(fromNode.pos, toNode.pos, {
          stroke: color,
          strokeWidth: 2,
          strokeDasharray: "5,5",
        });
      } else if (edge.type === "drop") {
        // Drop edges are dotted lines with arrow (unidirectional)
        this.#gizmos.line(fromNode.pos, toNode.pos, {
          stroke: color,
          strokeWidth: 2,
          strokeDasharray: "2,4",
        });
        // Draw arrow at end
        const angle = Math.atan2(
          toNode.pos.y - fromNode.pos.y,
          toNode.pos.x - fromNode.pos.x
        );
        const arrowSize = 8;
        const arrowAngle = Math.PI / 6;
        const p1 = {
          x: toNode.pos.x - arrowSize * Math.cos(angle - arrowAngle),
          y: toNode.pos.y - arrowSize * Math.sin(angle - arrowAngle),
        };
        const p2 = {
          x: toNode.pos.x - arrowSize * Math.cos(angle + arrowAngle),
          y: toNode.pos.y - arrowSize * Math.sin(angle + arrowAngle),
        };
        const polygon = document.createElementNS(
          "http://www.w3.org/2000/svg",
          "polygon"
        );
        polygon.setAttribute(
          "points",
          `${toNode.pos.x},${toNode.pos.y} ${p1.x},${p1.y} ${p2.x},${p2.y}`
        );
        polygon.setAttribute("fill", color);
        this.#gizmos.getSvg().appendChild(polygon);
      } else {
        // Walk edges are solid lines (bidirectional)
        this.#gizmos.line(fromNode.pos, toNode.pos, {
          stroke: color,
          strokeWidth: 3,
        });
      }

      // Edge label
      const midX = (fromNode.pos.x + toNode.pos.x) / 2;
      const midY = (fromNode.pos.y + toNode.pos.y) / 2;
      this.#gizmos.text(
        `${edge.type} (${Math.round(edge.cost)})`,
        { x: midX, y: midY - 5 },
        { fill: color, fontSize: 10, textAnchor: "middle" }
      );
    }

    // Draw nodes
    for (const node of this.#navmesh.nodes.values()) {
      const color = nodeColors[node.type];
      this.#gizmos.circle(node.pos, 6, {
        fill: color,
        stroke: "rgba(0, 0, 0, 0.5)",
        strokeWidth: 2,
      });

      // Node label
      this.#gizmos.text(
        node.id,
        { x: node.pos.x, y: node.pos.y - CHARACTER_RADIUS - 5 },
        { fill: color, fontSize: 10, textAnchor: "middle" }
      );
    }
  }

  #drawJumpArc(start: Vec2, end: Vec2, color: string): void {
    const samples = 30;

    // Build parabolic arc path
    let pathData = `M ${start.x} ${start.y}`;

    for (let i = 1; i <= samples; i++) {
      const t = i / samples;
      const x = start.x + (end.x - start.x) * t;

      // Parabolic arc that peaks at JUMP_ARC_HEIGHT above the higher platform
      const highestY = Math.min(start.y, end.y) - JUMP_ARC_HEIGHT;
      const parabola = -4 * (t - 0.5) * (t - 0.5) + 1;
      const y =
        start.y + (end.y - start.y) * t + (highestY - start.y) * parabola;

      pathData += ` L ${x} ${y}`;
    }

    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", pathData);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", color);
    path.setAttribute("stroke-width", "2");
    this.#gizmos.getSvg().appendChild(path);

    // Draw arrow at end
    const t1 = 1.0;
    const t0 = 0.95;

    const x1 = start.x + (end.x - start.x) * t1;
    const highestY = Math.min(start.y, end.y) - JUMP_ARC_HEIGHT;
    const parabola1 = -4 * (t1 - 0.5) * (t1 - 0.5) + 1;
    const y1 =
      start.y + (end.y - start.y) * t1 + (highestY - start.y) * parabola1;

    const x0 = start.x + (end.x - start.x) * t0;
    const parabola0 = -4 * (t0 - 0.5) * (t0 - 0.5) + 1;
    const y0 =
      start.y + (end.y - start.y) * t0 + (highestY - start.y) * parabola0;

    const angle = Math.atan2(y1 - y0, x1 - x0);
    const arrowSize = 8;
    const arrowAngle = Math.PI / 6;

    const p1 = {
      x: x1 - arrowSize * Math.cos(angle - arrowAngle),
      y: y1 - arrowSize * Math.sin(angle - arrowAngle),
    };
    const p2 = {
      x: x1 - arrowSize * Math.cos(angle + arrowAngle),
      y: y1 - arrowSize * Math.sin(angle + arrowAngle),
    };

    const polygon = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "polygon"
    );
    polygon.setAttribute(
      "points",
      `${x1},${y1} ${p1.x},${p1.y} ${p2.x},${p2.y}`
    );
    polygon.setAttribute("fill", color);
    this.#gizmos.getSvg().appendChild(polygon);
  }

  #updateUI(): void {
    // Count edges by type
    const edgeCounts = {
      walk: 0,
      jump: 0,
      climb: 0,
      fall: 0,
      step: 0,
      drop: 0,
    };
    for (const edge of this.#navmesh.edges.values()) {
      edgeCounts[edge.type]++;
    }

    document.getElementById("nodeCount")!.textContent =
      this.#navmesh.nodes.size.toString();
    document.getElementById("windowCount")!.textContent =
      this.#windows.length.toString();
    document.getElementById("walkCount")!.textContent =
      edgeCounts.walk.toString();
    document.getElementById("stepCount")!.textContent =
      edgeCounts.step.toString();
    document.getElementById("dropCount")!.textContent =
      edgeCounts.drop.toString();
    document.getElementById("jumpCount")!.textContent =
      edgeCounts.jump.toString();
    document.getElementById("jumpDistValue")!.textContent =
      MAX_JUMP_DISTANCE.toString();
    document.getElementById("arcHeightValue")!.textContent =
      JUMP_ARC_HEIGHT.toString();
  }

  #setupPhysicsControls(): void {
    const distSlider = document.getElementById(
      "jumpDistSlider"
    ) as HTMLInputElement;
    const heightSlider = document.getElementById(
      "arcHeightSlider"
    ) as HTMLInputElement;

    if (distSlider) {
      distSlider.value = MAX_JUMP_DISTANCE.toString();
      distSlider.addEventListener("input", (e) => {
        MAX_JUMP_DISTANCE = parseInt((e.target as HTMLInputElement).value);
        this.#rebuildNavmesh();
        this.#render();
        this.#updateUI();
      });
    }

    if (heightSlider) {
      heightSlider.value = JUMP_ARC_HEIGHT.toString();
      heightSlider.addEventListener("input", (e) => {
        JUMP_ARC_HEIGHT = parseInt((e.target as HTMLInputElement).value);
        this.#rebuildNavmesh();
        this.#render();
        this.#updateUI();
      });
    }
  }
}

// Initialize
new PetApp();
