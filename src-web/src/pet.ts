import { AXIO, AXNode } from "./axio.js";
import { Gizmos } from "./gizmos.js";
import { findPath, Heuristics } from "./astar.js";

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

type EdgeType =
  | "walk"
  | "climb"
  | "fall"
  | "jump"
  | "step"
  | "drop"
  | "hang"
  | "attach";

interface NavNode {
  id: string;
  pos: Vec2;
  type:
    | "platform_left"
    | "platform_right"
    | "platform_center"
    | "hang_left"
    | "hang_right"
    | "landing"; // Intermediate nodes created for jump/drop landing points
  windowId: string;
  componentId?: number; // Which connected component (via walk/step/drop/hang) this node belongs to
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

  /**
   * Get neighbors for A* pathfinding
   */
  getNeighbors(nodeId: string): Array<{ node: NavNode; cost: number }> {
    const neighbors: Array<{ node: NavNode; cost: number }> = [];
    for (const edge of this.edges.values()) {
      if (edge.from === nodeId) {
        const node = this.nodes.get(edge.to);
        if (node) {
          neighbors.push({ node, cost: edge.cost });
        }
      }
    }
    return neighbors;
  }
}

// ============================================================================
// Navmesh Builder
// ============================================================================

class NavmeshBuilder {
  #windows: WindowGeometry[] = [];

  // Helper: Check if node is a platform node
  #isPlatformNode(node: NavNode): boolean {
    return (
      node.type === "platform_left" ||
      node.type === "platform_right" ||
      node.type === "platform_center"
    );
  }

  // Helper: Check if node is a hang node
  #isHangNode(node: NavNode): boolean {
    return node.type === "hang_left" || node.type === "hang_right";
  }

  // Helper: Add bidirectional edge between two nodes
  #addBidirectionalEdge(
    navmesh: Navmesh,
    from: string,
    to: string,
    type: EdgeType,
    cost: number,
    idPrefix: string
  ): void {
    navmesh.addEdge({
      id: `${idPrefix}_${from}_to_${to}`,
      from,
      to,
      type,
      cost,
    });
    navmesh.addEdge({
      id: `${idPrefix}_${to}_to_${from}`,
      from: to,
      to: from,
      type,
      cost,
    });
  }

  build(windows: WindowGeometry[]): Navmesh {
    this.#windows = windows;
    const navmesh = new Navmesh();

    // Phase 1: Create base nodes from window geometry
    this.#createPlatformNodes(navmesh, windows);
    this.#createHangNodes(navmesh, windows);

    // Phase 2: Add primary locomotion edges (movement along surfaces)
    // Walk and hang edges are already created with nodes above

    // Phase 3: Add step edges (small gaps between platforms)
    this.#addStepEdges(navmesh);

    // Phase 4: Add drop edges (falling between platform nodes)
    this.#addDropEdges(navmesh);

    // Phase 5: Add transitions between platform and hang systems
    this.#addPlatformHangTransitions(navmesh);
    this.#addAttachEdges(navmesh);

    // Phase 6: Assign connected components (all reachable via walk/step/drop/hang/climb/attach)
    this.#assignComponentIds(navmesh);

    // Phase 7: Create landing nodes for edges that can be landed on
    // (requires componentIds to be assigned first)
    this.#createLandingNodes(navmesh);

    // Phase 8: Add jump edges between disconnected components
    this.#addJumpEdges(navmesh);

    return navmesh;
  }

  /**
   * Phase 1a: Create platform nodes and walk edges from window tops
   */
  #createPlatformNodes(navmesh: Navmesh, windows: WindowGeometry[]): void {
    this.#createSurfaceNodes(
      navmesh,
      windows,
      "platform",
      (win) => win.y - CHARACTER_RADIUS,
      (win, windows) => this.#findObstaclesOnPlatform(win, windows)
    );
  }

  /**
   * Phase 1b: Create hang nodes and hang edges from window undersides
   */
  #createHangNodes(navmesh: Navmesh, windows: WindowGeometry[]): void {
    this.#createSurfaceNodes(
      navmesh,
      windows,
      "hang",
      (win) => win.y + win.height + CHARACTER_RADIUS,
      (win, windows) => this.#findObstaclesOnHangSurface(win, windows)
    );
  }

  /**
   * Unified method to create nodes on a surface (platform or hang)
   */
  #createSurfaceNodes(
    navmesh: Navmesh,
    windows: WindowGeometry[],
    surfaceType: "platform" | "hang",
    getY: (win: WindowGeometry) => number,
    getObstacles: (
      win: WindowGeometry,
      windows: WindowGeometry[]
    ) => Array<{ left: number; right: number }>
  ): void {
    const leftType = surfaceType === "platform" ? "platform_left" : "hang_left";
    const rightType =
      surfaceType === "platform" ? "platform_right" : "hang_right";
    const edgeType = surfaceType === "platform" ? "walk" : "hang";
    const prefix = surfaceType === "platform" ? "seg" : "hang_seg";

    windows.forEach((win) => {
      const y = getY(win);
      const obstacles = getObstacles(win, windows);
      const segments = this.#splitPlatformIntoSegments(
        win.x + CHARACTER_RADIUS,
        win.x + win.width - CHARACTER_RADIUS,
        obstacles
      );

      segments.forEach((segment, segIdx) => {
        if (segment.width < MIN_PLATFORM_WIDTH) return;

        const leftPos = this.#findValidPosition(
          { x: segment.left, y },
          "right",
          segment.right - MIN_PLATFORM_WIDTH / 2,
          win.id
        );
        const rightPos = this.#findValidPosition(
          { x: segment.right, y },
          "left",
          segment.left + MIN_PLATFORM_WIDTH / 2,
          win.id
        );

        if (
          !leftPos ||
          !rightPos ||
          rightPos.x - leftPos.x < MIN_PLATFORM_WIDTH
        )
          return;

        const leftNode: NavNode = {
          id: `${win.id}_${prefix}${segIdx}_left`,
          pos: leftPos,
          type: leftType,
          windowId: win.id,
        };
        const rightNode: NavNode = {
          id: `${win.id}_${prefix}${segIdx}_right`,
          pos: rightPos,
          type: rightType,
          windowId: win.id,
        };

        navmesh.addNode(leftNode);
        navmesh.addNode(rightNode);
        navmesh.addEdge({
          id: `${win.id}_${prefix}${segIdx}_${edgeType}`,
          from: leftNode.id,
          to: rightNode.id,
          type: edgeType,
          cost: rightPos.x - leftPos.x,
        });
      });
    });
  }

  /**
   * Find obstacles on the underside of a window (windows hanging from above)
   */
  #findObstaclesOnHangSurface(
    baseWin: WindowGeometry,
    allWindows: WindowGeometry[]
  ): Array<{ left: number; right: number }> {
    const hangY = baseWin.y + baseWin.height + CHARACTER_RADIUS;
    const obstacles: Array<{ left: number; right: number }> = [];

    for (const win of allWindows) {
      if (win.id === baseWin.id) continue;

      // Check if this window hangs down and blocks the hang path
      const windowBottom = win.y + win.height;
      const isHangingInPath =
        windowBottom > baseWin.y + baseWin.height &&
        windowBottom < hangY + CHARACTER_RADIUS * 2;

      if (!isHangingInPath) continue;

      // Check horizontal overlap
      const horizontalOverlap =
        win.x < baseWin.x + baseWin.width && win.x + win.width > baseWin.x;

      if (horizontalOverlap) {
        obstacles.push({
          left: Math.max(win.x, baseWin.x),
          right: Math.min(win.x + win.width, baseWin.x + baseWin.width),
        });
      }
    }

    return obstacles;
  }

  /**
   * Phase 3: Add step edges between nearby platform nodes at similar heights
   */
  #addStepEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    const stepPairs = new Set<string>();
    let pairCount = 0;

    for (const fromNode of nodes) {
      if (!this.#isPlatformNode(fromNode)) continue;

      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue;
        if (!this.#isPlatformNode(toNode)) continue;

        // Check distance constraints
        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y;
        if (Math.abs(dx) > MAX_STEP_GAP) continue;
        if (Math.abs(dy) > MAX_STEP_HEIGHT) continue;

        // Create bidirectional edge (once per pair)
        const pairId = [fromNode.id, toNode.id].sort().join(":");
        if (stepPairs.has(pairId)) continue;
        stepPairs.add(pairId);

        this.#addBidirectionalEdge(
          navmesh,
          fromNode.id,
          toNode.id,
          "step",
          Math.hypot(dx, dy),
          "step"
        );
        pairCount++;
      }
    }

    console.log(`[NavDemo] Added ${pairCount} step edge pairs`);
  }

  /**
   * Phase 4: Add drop edges for dropping down from platforms
   */
  #addDropEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    const stepPairs = this.#getStepPairs(navmesh);
    let dropCount = 0;

    for (const fromNode of nodes) {
      if (!this.#isPlatformNode(fromNode)) continue;

      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue;
        if (!this.#isPlatformNode(toNode)) continue;

        // Skip if step edge exists (precedence)
        const pairId = [fromNode.id, toNode.id].sort().join(":");
        if (stepPairs.has(pairId)) continue;

        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y;

        // Must drop down
        if (dy <= 0) continue;
        if (Math.abs(dx) > MAX_DROP_GAP) continue;
        if (dy > MAX_DROP_HEIGHT) continue;

        navmesh.addEdge({
          id: `drop_${fromNode.id}_to_${toNode.id}`,
          from: fromNode.id,
          to: toNode.id,
          type: "drop",
          cost: Math.hypot(dx, dy) * 0.8,
        });
        dropCount++;
      }
    }

    console.log(`[NavDemo] Added ${dropCount} drop edges`);
  }

  // Helper: Get set of node pairs connected by step edges
  #getStepPairs(navmesh: Navmesh): Set<string> {
    const stepPairs = new Set<string>();
    for (const edge of navmesh.edges.values()) {
      if (edge.type === "step") {
        stepPairs.add([edge.from, edge.to].sort().join(":"));
      }
    }
    return stepPairs;
  }

  /**
   * Add transition edges between platform and hang nodes
   * These allow transitioning from walking to hanging and vice versa
   */
  #addPlatformHangTransitions(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let transitionCount = 0;

    for (const platformNode of nodes) {
      // Only consider platform edge nodes
      if (
        platformNode.type !== "platform_left" &&
        platformNode.type !== "platform_right"
      )
        continue;

      for (const hangNode of nodes) {
        // Only consider hang edge nodes
        if (hangNode.type !== "hang_left" && hangNode.type !== "hang_right")
          continue;

        // Skip same window transitions (can't grab underneath your own platform)
        if (platformNode.windowId === hangNode.windowId) continue;

        const dx = hangNode.pos.x - platformNode.pos.x;
        const dy = hangNode.pos.y - platformNode.pos.y;
        const distance = Math.hypot(dx, dy);

        // Must be close enough and hang node should be below platform node
        if (distance > CHARACTER_RADIUS * 3) continue;
        if (dy <= 0) continue; // Hang must be below platform

        // Add bidirectional transition
        // Platform -> Hang (drop to grab)
        navmesh.addEdge({
          id: `transition_${platformNode.id}_to_${hangNode.id}`,
          from: platformNode.id,
          to: hangNode.id,
          type: "drop",
          cost: distance * 1.2, // Slight penalty for transition
        });

        // Hang -> Platform (climb up)
        navmesh.addEdge({
          id: `transition_${hangNode.id}_to_${platformNode.id}`,
          from: hangNode.id,
          to: platformNode.id,
          type: "climb",
          cost: distance * 1.5, // Higher penalty for climbing up
        });

        transitionCount += 2;
      }
    }

    console.log(`[NavDemo] Added ${transitionCount} platform-hang transitions`);
  }

  /**
   * Add attach edges to grab onto hanging positions from platform edges
   * This allows starting to hang from the edge of a platform
   */
  #addAttachEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let attachCount = 0;

    for (const platformNode of nodes) {
      // Only consider platform edge nodes
      if (
        platformNode.type !== "platform_left" &&
        platformNode.type !== "platform_right"
      )
        continue;

      for (const hangNode of nodes) {
        // Only consider hang edge nodes
        if (hangNode.type !== "hang_left" && hangNode.type !== "hang_right")
          continue;

        // Skip same window attachments
        if (platformNode.windowId === hangNode.windowId) continue;

        const dx = hangNode.pos.x - platformNode.pos.x;
        const dy = hangNode.pos.y - platformNode.pos.y;
        const distance = Math.hypot(dx, dy);

        // Must be close enough and hang node should be below/at similar level
        if (distance > CHARACTER_RADIUS * 2) continue;

        // Add unidirectional attach edge (platform -> hang)
        navmesh.addEdge({
          id: `attach_${platformNode.id}_to_${hangNode.id}`,
          from: platformNode.id,
          to: hangNode.id,
          type: "attach",
          cost: distance * 1.1, // Small penalty for attaching
        });
        attachCount++;
      }
    }

    console.log(`[NavDemo] Added ${attachCount} attach edges`);
  }

  /**
   * Phase 6: Assign connected component IDs to all nodes
   * Nodes reachable via walk/step/drop/hang/climb/attach get the same ID
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

        // Add neighbors via walk/step/drop/hang/climb/attach edges
        for (const edge of navmesh.edges.values()) {
          if (edge.from !== nodeId) continue;
          if (
            edge.type !== "walk" &&
            edge.type !== "step" &&
            edge.type !== "drop" &&
            edge.type !== "hang" &&
            edge.type !== "climb" &&
            edge.type !== "attach"
          )
            continue;
          queue.push(edge.to);
        }
      }
    }

    console.log(`[NavDemo] Found ${nextComponentId} connected components`);
  }

  /**
   * Phase 7: Create landing nodes where edges can be landed on
   * This unifies all "landing on edges" logic in one place
   * Requires componentIds to be assigned first (Phase 6)
   */
  #createLandingNodes(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let landingNodeCount = 0;

    // Get all walk/step edges that can be landed on
    const walkStepEdges: Array<{ from: NavNode; to: NavNode; edge: NavEdge }> =
      [];
    for (const edge of navmesh.edges.values()) {
      if (edge.type !== "walk" && edge.type !== "step") continue;
      const fromNode = navmesh.getNode(edge.from);
      const toNode = navmesh.getNode(edge.to);
      if (!fromNode || !toNode) continue;
      walkStepEdges.push({ from: fromNode, to: toNode, edge });
    }

    // Get existing step pairs for precedence checking
    const stepPairs = new Set<string>();
    for (const edge of navmesh.edges.values()) {
      if (edge.type === "step") {
        const pairId = [edge.from, edge.to].sort().join(":");
        stepPairs.add(pairId);
      }
    }

    // Helper to create/get a landing node on an edge
    const createLandingNode = (
      landingPoint: Vec2,
      walkEdge: { from: NavNode; to: NavNode; edge: NavEdge }
    ): NavNode => {
      const landingNodeId = `landing_${walkEdge.from.windowId}_${Math.round(
        landingPoint.x
      )}_${Math.round(landingPoint.y)}`;

      let landingNode = navmesh.getNode(landingNodeId);
      if (!landingNode) {
        landingNode = {
          id: landingNodeId,
          pos: landingPoint,
          type: "landing",
          windowId: walkEdge.from.windowId,
          componentId: walkEdge.from.componentId,
        };
        navmesh.addNode(landingNode);

        // Connect to edge endpoints
        const edgeStart = walkEdge.from.pos;
        const edgeEnd = walkEdge.to.pos;
        const distToStart = Math.abs(landingPoint.x - edgeStart.x);
        const distToEnd = Math.abs(landingPoint.x - edgeEnd.x);

        navmesh.addEdge({
          id: `${landingNodeId}_to_${walkEdge.from.id}`,
          from: landingNodeId,
          to: walkEdge.from.id,
          type: walkEdge.edge.type,
          cost: distToStart,
        });
        navmesh.addEdge({
          id: `${walkEdge.from.id}_to_${landingNodeId}`,
          from: walkEdge.from.id,
          to: landingNodeId,
          type: walkEdge.edge.type,
          cost: distToStart,
        });
        navmesh.addEdge({
          id: `${landingNodeId}_to_${walkEdge.to.id}`,
          from: landingNodeId,
          to: walkEdge.to.id,
          type: walkEdge.edge.type,
          cost: distToEnd,
        });
        navmesh.addEdge({
          id: `${walkEdge.to.id}_to_${landingNodeId}`,
          from: walkEdge.to.id,
          to: landingNodeId,
          type: walkEdge.edge.type,
          cost: distToEnd,
        });

        landingNodeCount++;
      }
      return landingNode;
    };

    // Case 1: Drops from hang nodes (vertical raycast down)
    for (const fromNode of nodes) {
      if (fromNode.type !== "hang_left" && fromNode.type !== "hang_right")
        continue;

      for (const walkEdge of walkStepEdges) {
        if (fromNode.componentId === walkEdge.from.componentId) continue;

        const edgeStart = walkEdge.from.pos;
        const edgeEnd = walkEdge.to.pos;
        const edgeY = edgeStart.y;

        if (edgeY <= fromNode.pos.y) continue;
        if (fromNode.pos.x < Math.min(edgeStart.x, edgeEnd.x)) continue;
        if (fromNode.pos.x > Math.max(edgeStart.x, edgeEnd.x)) continue;

        const landingPoint = { x: fromNode.pos.x, y: edgeY };
        const dy = landingPoint.y - fromNode.pos.y;

        if (dy > MAX_DROP_HEIGHT) continue;
        if (this.#lineIntersectsWindows(fromNode.pos, landingPoint)) continue;

        const landingNode = createLandingNode(landingPoint, walkEdge);
        const pairId = [fromNode.id, landingNode.id].sort().join(":");
        if (stepPairs.has(pairId)) continue;

        navmesh.addEdge({
          id: `drop_${fromNode.id}_to_${landingNode.id}`,
          from: fromNode.id,
          to: landingNode.id,
          type: "drop",
          cost: dy * 0.8,
        });
      }
    }

    // Case 2: Jumps/drops from platform nodes (projected landing)
    for (const fromNode of nodes) {
      if (fromNode.type === "landing") continue;
      if (fromNode.type === "hang_left" || fromNode.type === "hang_right")
        continue;

      for (const walkEdge of walkStepEdges) {
        if (fromNode.componentId === walkEdge.from.componentId) continue;

        const edgeStart = walkEdge.from.pos;
        const edgeEnd = walkEdge.to.pos;
        const edgeVec = {
          x: edgeEnd.x - edgeStart.x,
          y: edgeEnd.y - edgeStart.y,
        };
        const edgeLen = Math.hypot(edgeVec.x, edgeVec.y);
        if (edgeLen < 0.01) continue;

        // Project fromNode onto edge
        const toEdgeVec = {
          x: fromNode.pos.x - edgeStart.x,
          y: fromNode.pos.y - edgeStart.y,
        };
        const t = Math.max(
          0,
          Math.min(
            1,
            (toEdgeVec.x * edgeVec.x + toEdgeVec.y * edgeVec.y) /
              (edgeLen * edgeLen)
          )
        );

        if (t < 0.1 || t > 0.9) continue; // Too close to endpoints

        const landingPoint = {
          x: edgeStart.x + t * edgeVec.x,
          y: edgeStart.y + t * edgeVec.y,
        };
        const dx = landingPoint.x - fromNode.pos.x;
        const dy = landingPoint.y - fromNode.pos.y;
        const horizontalDist = Math.abs(dx);

        if (horizontalDist > MAX_JUMP_DISTANCE) continue;
        const maxJumpUp = JUMP_ARC_HEIGHT * 0.5;
        if (dy < -maxJumpUp) continue;
        if (this.#lineIntersectsWindows(fromNode.pos, landingPoint)) continue;
        if (!this.#isJumpArcClear(fromNode.pos, landingPoint)) continue;

        const landingNode = createLandingNode(landingPoint, walkEdge);
        const straightDistance = Math.hypot(dx, dy);
        navmesh.addEdge({
          id: `jump_${fromNode.id}_to_${landingNode.id}`,
          from: fromNode.id,
          to: landingNode.id,
          type: "jump",
          cost: straightDistance * 1.5,
        });
      }
    }

    console.log(`[NavDemo] Created ${landingNodeCount} landing nodes`);
  }

  /**
   * Phase 8: Add jump edges between disconnected components (node-to-node only)
   */
  #addJumpEdges(navmesh: Navmesh): void {
    const nodes = Array.from(navmesh.nodes.values());
    let jumpCount = 0;
    let totalTests = 0;
    const rejected = {
      component: 0,
      distance: 0,
      physics: 0,
      line: 0,
      parabola: 0,
    };

    for (const fromNode of nodes) {
      // Only platform nodes can initiate jumps
      if (!this.#isPlatformNode(fromNode)) continue;

      for (const toNode of nodes) {
        if (fromNode.id === toNode.id) continue;
        if (fromNode.windowId === toNode.windowId) continue;
        // Can't jump to hang nodes
        if (this.#isHangNode(toNode)) continue;

        totalTests++;

        // Skip if already connected
        if (fromNode.componentId === toNode.componentId) {
          rejected.component++;
          continue;
        }

        const dx = toNode.pos.x - fromNode.pos.x;
        const dy = toNode.pos.y - fromNode.pos.y;

        // Check jump constraints
        if (Math.abs(dx) > MAX_JUMP_DISTANCE) {
          rejected.distance++;
          continue;
        }
        if (dy < -JUMP_ARC_HEIGHT * 0.5) {
          rejected.physics++;
          continue;
        }
        if (this.#lineIntersectsWindows(fromNode.pos, toNode.pos)) {
          rejected.line++;
          continue;
        }
        if (!this.#isJumpArcClear(fromNode.pos, toNode.pos)) {
          rejected.parabola++;
          continue;
        }

        navmesh.addEdge({
          id: `jump_${fromNode.id}_to_${toNode.id}`,
          from: fromNode.id,
          to: toNode.id,
          type: "jump",
          cost: Math.hypot(dx, dy) * 1.5,
        });
        jumpCount++;
      }
    }

    console.log(`[NavDemo] Jump edges: ${jumpCount}/${totalTests} tests`);
    console.log(`  - Rejected:`, rejected);
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
// Physics World Manager
// ============================================================================

class PhysicsWorld {
  #RAPIER: any;
  #world: any;
  #windowColliders: Map<string, any> = new Map();

  constructor(RAPIER: any) {
    this.#RAPIER = RAPIER;
    const gravity = { x: 0.0, y: 500.0 }; // Positive Y is down in our coordinate system
    this.#world = new RAPIER.World(gravity);
  }

  getWorld(): any {
    return this.#world;
  }

  getRAPIER(): any {
    return this.#RAPIER;
  }

  step(deltaTime: number): void {
    // Rapier expects deltaTime in seconds
    this.#world.timestep = Math.min(deltaTime / 1000, 0.1);
    this.#world.step();
  }

  updateWindows(windows: WindowGeometry[]): void {
    // Remove old window colliders
    for (const collider of this.#windowColliders.values()) {
      this.#world.removeCollider(collider, false);
    }
    this.#windowColliders.clear();

    // Create new window colliders (static platforms)
    for (const win of windows) {
      const colliderDesc = this.#RAPIER.ColliderDesc.cuboid(
        win.width / 2,
        win.height / 2
      ).setTranslation(win.x + win.width / 2, win.y + win.height / 2);

      const collider = this.#world.createCollider(colliderDesc);
      this.#windowColliders.set(win.id, collider);
    }
  }

  createCharacterRigidBody(pos: Vec2): any {
    const rigidBodyDesc = this.#RAPIER.RigidBodyDesc.dynamic()
      .setTranslation(pos.x, pos.y)
      .setLinearDamping(2.0) // Air resistance
      .setCanSleep(false); // Always active for responsive controls

    const rigidBody = this.#world.createRigidBody(rigidBodyDesc);

    // Create character collider (circle)
    const colliderDesc = this.#RAPIER.ColliderDesc.ball(CHARACTER_RADIUS)
      .setRestitution(0.0) // No bounciness
      .setFriction(0.5);

    this.#world.createCollider(colliderDesc, rigidBody);

    return rigidBody;
  }
}

// ============================================================================
// Character
// ============================================================================

type CharacterState = "idle" | "walking" | "jumping" | "falling" | "hanging";

class Character {
  pos: Vec2;
  velocity: Vec2 = { x: 0, y: 0 };
  state: CharacterState = "idle";

  #path: string[] = []; // Current path (node IDs)
  #pathIndex: number = 0; // Current target node in path
  #destinationNode: string | null = null;

  #navmesh: Navmesh;
  #physicsWorld: PhysicsWorld;
  #rigidBody: any;

  // Movement constants
  #walkSpeed = 200; // Pixels per second
  #maxSpeed = 250;

  // Timers
  #idleTimer = 0;
  #idleDelay = 2000; // ms before picking new destination

  constructor(
    navmesh: Navmesh,
    _windows: WindowGeometry[],
    physicsWorld: PhysicsWorld
  ) {
    this.#navmesh = navmesh;
    this.#physicsWorld = physicsWorld;

    // Start at a random platform node
    const spawnPos = this.#getRandomSpawnPosition();
    this.pos = spawnPos;

    // Create physics rigid body
    this.#rigidBody = physicsWorld.createCharacterRigidBody(spawnPos);
  }

  #getRandomSpawnPosition(): Vec2 {
    const platformNodes = Array.from(this.#navmesh.nodes.values()).filter(
      (n) =>
        n.type === "platform_left" ||
        n.type === "platform_right" ||
        n.type === "platform_center"
    );

    if (platformNodes.length === 0) {
      return { x: window.innerWidth / 2, y: window.innerHeight / 2 };
    }

    const node =
      platformNodes[Math.floor(Math.random() * platformNodes.length)];
    return { ...node.pos };
  }

  update(deltaTime: number): void {
    // Sync position from physics
    const translation = this.#rigidBody.translation();
    this.pos = { x: translation.x, y: translation.y };

    const linvel = this.#rigidBody.linvel();
    this.velocity = { x: linvel.x, y: linvel.y };

    // Update state based on physics
    this.#updateState();

    // Update behavior
    switch (this.state) {
      case "idle":
        this.#updateIdle(deltaTime);
        break;
      case "walking":
        this.#updateWalking(deltaTime);
        break;
      case "jumping":
      case "falling":
        this.#updateAirborne(deltaTime);
        break;
    }
  }

  #updateState(): void {
    const onGround = this.#isOnGround();

    if (onGround) {
      if (this.state === "falling" || this.state === "jumping") {
        this.state = "idle";
      }
    } else {
      if (this.state === "walking" || this.state === "idle") {
        this.state = "falling";
      }
    }
  }

  #isOnGround(): boolean {
    // Check if character is touching ground using Rapier's collision detection
    const world = this.#physicsWorld.getWorld();
    const RAPIER = this.#physicsWorld.getRAPIER();

    // Cast a small ray downward to detect ground
    const rayOrigin = { x: this.pos.x, y: this.pos.y };
    const rayDir = { x: 0, y: 1 };
    const maxToi = CHARACTER_RADIUS + 5; // Slightly beyond character radius

    const ray = new RAPIER.Ray(rayOrigin, rayDir);
    const hit = world.castRay(ray, maxToi, false);

    return hit !== null;
  }

  #updateIdle(deltaTime: number): void {
    // Stop horizontal movement
    this.#rigidBody.setLinvel({ x: 0, y: this.velocity.y }, true);

    this.#idleTimer += deltaTime;
    if (this.#idleTimer >= this.#idleDelay) {
      this.#idleTimer = 0;
      this.#pickNewDestination();
    }
  }

  #updateWalking(_deltaTime: number): void {
    if (this.#path.length === 0 || this.#pathIndex >= this.#path.length) {
      // Reached destination
      this.state = "idle";
      this.#rigidBody.setLinvel({ x: 0, y: this.velocity.y }, true);
      this.#path = [];
      this.#pathIndex = 0;
      return;
    }

    const targetNodeId = this.#path[this.#pathIndex];
    const targetNode = this.#navmesh.getNode(targetNodeId);

    if (!targetNode) {
      console.warn(
        `[Character] Target node ${targetNodeId} not found, replanning...`
      );
      this.#pickNewDestination();
      return;
    }

    // Move towards target node
    const dx = targetNode.pos.x - this.pos.x;
    const distToTarget = Math.abs(dx);

    if (distToTarget < 15) {
      // Reached this waypoint, move to next
      this.#pathIndex++;
      if (this.#pathIndex >= this.#path.length) {
        this.state = "idle";
        this.#rigidBody.setLinvel({ x: 0, y: this.velocity.y }, true);
      }
    } else {
      // Apply force towards target
      const direction = dx > 0 ? 1 : -1;
      const targetVelX = direction * this.#walkSpeed;

      // Clamp speed
      const clampedVelX = Math.max(
        -this.#maxSpeed,
        Math.min(this.#maxSpeed, targetVelX)
      );

      this.#rigidBody.setLinvel({ x: clampedVelX, y: this.velocity.y }, true);
    }
  }

  #updateAirborne(_deltaTime: number): void {
    // Maintain some horizontal control in air
    // Physics handles gravity automatically
  }

  #pickNewDestination(): void {
    // Find nearest node to current position
    const startNode = this.#findNearestNode();
    if (!startNode) {
      console.warn("[Character] No valid start node found");
      return;
    }

    // Pick random destination
    const platformNodes = Array.from(this.#navmesh.nodes.values()).filter(
      (n) =>
        (n.type === "platform_left" ||
          n.type === "platform_right" ||
          n.type === "platform_center") &&
        n.id !== startNode.id
    );

    if (platformNodes.length === 0) {
      console.warn("[Character] No destination nodes available");
      return;
    }

    const destNode =
      platformNodes[Math.floor(Math.random() * platformNodes.length)];

    console.log(
      `[Character] Planning path from ${startNode.id} to ${destNode.id}`
    );

    this.#path = findPath(this.#navmesh, startNode.id, destNode.id, {
      heuristic: Heuristics.euclidean,
      closest: true,
    });

    if (this.#path.length > 0) {
      console.log(`[Character] Found path with ${this.#path.length} nodes`);
      this.#pathIndex = 0;
      this.#destinationNode = destNode.id;
      this.state = "walking";
    } else {
      console.warn("[Character] No path found");
    }
  }

  #findNearestNode(): NavNode | null {
    const nodes = Array.from(this.#navmesh.nodes.values());
    if (nodes.length === 0) return null;

    let nearest: NavNode | null = null;
    let minDist = Infinity;

    for (const node of nodes) {
      const dist = Math.hypot(node.pos.x - this.pos.x, node.pos.y - this.pos.y);
      if (dist < minDist) {
        minDist = dist;
        nearest = node;
      }
    }

    return nearest;
  }

  replaceNavmesh(
    navmesh: Navmesh,
    _windows: WindowGeometry[],
    physicsWorld: PhysicsWorld
  ): void {
    this.#navmesh = navmesh;
    this.#physicsWorld = physicsWorld;

    // Physics world has been updated with new window colliders

    // If we're currently following a path, replan
    if (this.state === "walking" && this.#destinationNode) {
      const startNode = this.#findNearestNode();
      const destNode = navmesh.getNode(this.#destinationNode);

      if (startNode && destNode) {
        console.log("[Character] Replanning due to navmesh change");
        this.#path = findPath(navmesh, startNode.id, destNode.id, {
          heuristic: Heuristics.euclidean,
          closest: true,
        });
        this.#pathIndex = 0;

        if (this.#path.length === 0) {
          console.warn("[Character] Replan failed, picking new destination");
          this.#pickNewDestination();
        }
      } else {
        console.warn("[Character] Destination lost, picking new destination");
        this.#pickNewDestination();
      }
    }
  }

  getPath(): string[] {
    return this.#path;
  }

  render(gizmos: Gizmos): void {
    // Draw simple character: cube with legs
    const size = CHARACTER_RADIUS;

    // Body (cube)
    gizmos.rect(this.pos.x - size / 2, this.pos.y - size, size, size, {
      fill: "rgba(100, 150, 255, 0.9)",
      stroke: "rgba(0, 0, 0, 0.8)",
      strokeWidth: 2,
    });

    // Legs (simple lines)
    const legWidth = 3;
    const legHeight = size / 2;
    gizmos.line(
      { x: this.pos.x - size / 4, y: this.pos.y },
      { x: this.pos.x - size / 4, y: this.pos.y + legHeight },
      { stroke: "rgba(0, 0, 0, 0.8)", strokeWidth: legWidth }
    );
    gizmos.line(
      { x: this.pos.x + size / 4, y: this.pos.y },
      { x: this.pos.x + size / 4, y: this.pos.y + legHeight },
      { stroke: "rgba(0, 0, 0, 0.8)", strokeWidth: legWidth }
    );

    // State indicator
    gizmos.text(
      this.state,
      { x: this.pos.x, y: this.pos.y - size - 10 },
      { fill: "white", fontSize: 12, textAnchor: "middle" }
    );
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
  #character: Character | null = null;
  #physicsWorld: PhysicsWorld | null = null;
  #lastTime = performance.now();

  constructor() {
    this.#axio = new AXIO();
    const debugLayer = document.getElementById("debug-layer") as any;
    this.#gizmos = new Gizmos(debugLayer);

    this.#initPhysics();
    this.#setupAXIO();
    this.#setupPhysicsControls();
    this.#startGameLoop();
  }

  async #initPhysics(): Promise<void> {
    try {
      // Load Rapier asynchronously
      const RAPIER = await import("@dimforge/rapier2d");
      this.#physicsWorld = new PhysicsWorld(RAPIER);
      console.log("[NavDemo] Rapier physics initialized");
    } catch (error) {
      console.error("[NavDemo] Failed to initialize Rapier:", error);
    }
  }

  #startGameLoop(): void {
    const loop = (currentTime: number) => {
      const deltaTime = currentTime - this.#lastTime;
      this.#lastTime = currentTime;

      // Step physics simulation
      if (this.#physicsWorld) {
        this.#physicsWorld.step(deltaTime);
      }

      // Update character
      if (this.#character) {
        this.#character.update(deltaTime);
      }

      // Render (only if we have a character to show)
      if (this.#character) {
        this.#render();
      }

      requestAnimationFrame(loop);
    };

    requestAnimationFrame(loop);
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
        this.#updateUI();
        // Note: Rendering is handled by game loop
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

    // Update physics world with new window colliders
    if (this.#physicsWorld) {
      this.#physicsWorld.updateWindows(this.#windows);
    }

    // Create or update character
    if (!this.#character && this.#physicsWorld) {
      this.#character = new Character(
        this.#navmesh,
        this.#windows,
        this.#physicsWorld
      );
      console.log("[NavDemo] Character spawned");
    } else if (this.#character && this.#physicsWorld) {
      this.#character.replaceNavmesh(
        this.#navmesh,
        this.#windows,
        this.#physicsWorld
      );
    }
  }

  #render(): void {
    this.#gizmos.clear();
    this.#renderWindowGeometry();
    this.#renderNavmesh();

    // Render character on top
    if (this.#character) {
      this.#character.render(this.#gizmos);
    }
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
      drop: "rgba(255, 150, 255, 0.8)",
      hang: "rgba(0, 255, 255, 0.8)",
      attach: "rgba(255, 100, 200, 0.8)", // Pink for attach edges
    };

    const nodeColors = {
      platform_left: "rgba(255, 200, 0, 0.9)",
      platform_right: "rgba(0, 200, 255, 0.9)",
      platform_center: "rgba(0, 255, 0, 0.9)",
      hang_left: "rgba(0, 255, 255, 0.9)",
      hang_right: "rgba(0, 200, 200, 0.9)",
      landing: "rgba(200, 200, 200, 0.7)", // Gray for landing nodes
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
      } else if (
        edge.type === "drop" ||
        edge.type === "climb" ||
        edge.type === "attach"
      ) {
        // Drop/climb/attach edges are dotted lines with arrow (unidirectional)
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
      } else if (edge.type === "hang") {
        // Hang edges are wavy lines (bidirectional) - like walk but different style
        this.#gizmos.line(fromNode.pos, toNode.pos, {
          stroke: color,
          strokeWidth: 3,
          strokeDasharray: "8,4",
        });
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

    // Draw character's path
    if (this.#character) {
      const characterPath = this.#character.getPath();
      if (characterPath.length > 1) {
        const pathNodes = characterPath
          .map((id) => this.#navmesh.getNode(id))
          .filter((n): n is NavNode => n !== undefined);

        // Draw path segments
        for (let i = 0; i < pathNodes.length - 1; i++) {
          const from = pathNodes[i];
          const to = pathNodes[i + 1];
          this.#gizmos.line(from.pos, to.pos, {
            stroke: "rgba(255, 100, 255, 0.6)",
            strokeWidth: 4,
          });
        }

        // Highlight destination
        if (pathNodes.length > 0) {
          const end = pathNodes[pathNodes.length - 1];
          this.#gizmos.circle(end.pos, CHARACTER_RADIUS * 1.2, {
            fill: "rgba(255, 0, 0, 0.4)",
            stroke: "rgba(255, 0, 0, 0.8)",
            strokeWidth: 2,
          });
        }
      }
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
      hang: 0,
      attach: 0,
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
    document.getElementById("hangCount")!.textContent =
      edgeCounts.hang.toString();
    document.getElementById("attachCount")!.textContent =
      edgeCounts.attach.toString();
    document.getElementById("climbCount")!.textContent =
      edgeCounts.climb.toString();
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
        this.#updateUI();
      });
    }

    if (heightSlider) {
      heightSlider.value = JUMP_ARC_HEIGHT.toString();
      heightSlider.addEventListener("input", (e) => {
        JUMP_ARC_HEIGHT = parseInt((e.target as HTMLInputElement).value);
        this.#rebuildNavmesh();
        this.#updateUI();
      });
    }
  }
}

// Initialize
new PetApp();
