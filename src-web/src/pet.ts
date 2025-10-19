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

// ============================================================================
// Navmesh Types
// ============================================================================

type EdgeType = "walk" | "climb" | "fall";

interface NavNode {
  id: string;
  pos: Vec2;
  type: "platform_left" | "platform_right" | "platform_center";
  windowId: string;
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

    return navmesh;
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
      this.#gizmos.line(fromNode.pos, toNode.pos, {
        stroke: color,
        strokeWidth: 3,
      });

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

  #updateUI(): void {
    document.getElementById("petState")!.textContent = "🗺️ Navmesh ready";
    document.getElementById("nodeCount")!.textContent =
      this.#navmesh.nodes.size.toString();
    document.getElementById("edgeCount")!.textContent =
      this.#navmesh.edges.size.toString();
    document.getElementById("windowCount")!.textContent =
      this.#windows.length.toString();
  }
}

// Initialize
new PetApp();
