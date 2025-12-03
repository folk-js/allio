import { AXIO, Window } from "@axio/client";

interface Rectangle {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Point {
  x: number;
  y: number;
}

class WindowOverlay {
  private windowContainer: HTMLElement;
  private axio: AXIO;
  private windowElements: Map<string, HTMLElement> = new Map();
  private svgElement!: SVGSVGElement;
  private borderGroups: Map<string, SVGPathElement> = new Map();

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.axio = new AXIO();
    this.setupSVG();
    this.setupWebSocketListener();
  }

  private setupSVG() {
    // Create SVG element for unified borders
    this.svgElement = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "svg"
    );
    this.svgElement.style.position = "absolute";
    this.svgElement.style.top = "0";
    this.svgElement.style.left = "0";
    this.svgElement.style.width = "100vw";
    this.svgElement.style.height = "100vh";
    this.svgElement.style.pointerEvents = "none";
    this.svgElement.style.zIndex = "1000";

    this.windowContainer.appendChild(this.svgElement);
  }

  private async setupWebSocketListener() {
    try {
      // Set up window update handler
      this.axio.onWindowUpdate((windows) => {
        this.updateWindowRectangles(windows);
      });

      // Connect to websocket
      await this.axio.connect();

      console.log("📡 AXIO connected");
    } catch (error) {
      console.error("❌ Failed to connect AXIO:", error);
    }
  }

  private updateWindowRectangles(windows: Window[]) {
    // Filter out very small windows
    const visibleWindows = windows.filter((w) => w.w >= 50 && w.h >= 50);

    // Keep track of current window IDs
    const currentWindowIds = new Set(visibleWindows.map((w) => w.id));

    // Remove labels for windows that no longer exist
    for (const [windowId, element] of this.windowElements) {
      if (!currentWindowIds.has(windowId)) {
        element.remove();
        this.windowElements.delete(windowId);
      }
    }

    // Update or create labels for each window
    visibleWindows.forEach((window) => {
      this.updateWindowLabel(window);
    });

    // Group overlapping windows and create unified borders
    this.updateUnifiedBorders(visibleWindows);
  }

  private updateWindowLabel(window: Window) {
    let labelElement = this.windowElements.get(window.id);

    // Create new label element if it doesn't exist
    if (!labelElement) {
      labelElement = document.createElement("div");
      labelElement.className = "window-label";
      this.windowContainer.appendChild(labelElement);
      this.windowElements.set(window.id, labelElement);
    }

    // Get client status
    const hasClient = !!(window as any).client_id;
    const clientStatus = hasClient ? "🔗" : "○";

    // Update label content
    labelElement.textContent = `${clientStatus} ${
      window.title || "Untitled"
    } (${window.id})`;

    // Update CSS classes
    labelElement.className = "window-label";

    if (window.focused) {
      labelElement.classList.add("focused");
    }

    if (hasClient) {
      labelElement.classList.add("has-client");
    }

    // Position the label
    labelElement.style.position = "absolute";
    labelElement.style.left = `${window.x}px`;
    labelElement.style.top = `${window.y - 24}px`;

    // Ensure label is visible by default (visibility will be adjusted later if needed)
    labelElement.style.display = "block";
  }

  private updateUnifiedBorders(windows: Window[]) {
    // Clear existing border groups
    this.borderGroups.forEach((path) => path.remove());
    this.borderGroups.clear();

    if (windows.length === 0) {
      return;
    }

    // Group overlapping windows
    const overlappingGroups = this.groupOverlappingWindows(windows);

    // Create unified borders for each group and update label visibility
    overlappingGroups.forEach((group, index) => {
      const rectangles = group.map((w) => ({
        x: w.x,
        y: w.y,
        w: w.w,
        h: w.h,
      }));
      const unionPolygon = this.computeRectangleUnion(rectangles);
      const borderStyle = this.getBorderStyleForGroup(group);

      if (unionPolygon.length > 0) {
        const pathElement = this.createBorderPath(unionPolygon, borderStyle);
        this.svgElement.appendChild(pathElement);
        this.borderGroups.set(`group-${index}`, pathElement);

        // Update label visibility based on polygon containment (only for multi-window groups)
        if (group.length > 1) {
          this.updateLabelVisibility(group, unionPolygon);
        } else {
          // Single window groups: always show label
          const labelElement = this.windowElements.get(group[0].id);
          if (labelElement) {
            labelElement.style.display = "block";
          }
        }
      }
    });
  }

  private updateLabelVisibility(windows: Window[], polygon: Point[]) {
    for (const window of windows) {
      const labelElement = this.windowElements.get(window.id);
      if (!labelElement) continue;

      // Check if the label position is inside the polygon
      const labelX = window.x;
      const labelY = window.y - 24; // Label is positioned above the window
      const isInside = this.isPointInPolygon({ x: labelX, y: labelY }, polygon);

      // Show label if: it's outside the polygon OR the window is focused
      const shouldShow = !isInside || window.focused;

      labelElement.style.display = shouldShow ? "block" : "none";
    }
  }

  private isPointInPolygon(point: Point, polygon: Point[]): boolean {
    if (polygon.length < 3) return false;

    let inside = false;
    for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
      const xi = polygon[i].x,
        yi = polygon[i].y;
      const xj = polygon[j].x,
        yj = polygon[j].y;

      if (
        yi > point.y !== yj > point.y &&
        point.x < ((xj - xi) * (point.y - yi)) / (yj - yi) + xi
      ) {
        inside = !inside;
      }
    }

    return inside;
  }

  private groupOverlappingWindows(windows: Window[]): Window[][] {
    const groups: Window[][] = [];
    const visited = new Set<string>();

    for (const window of windows) {
      if (visited.has(window.id)) continue;

      const group = [window];
      visited.add(window.id);

      // Find all windows that overlap with any window in the current group
      let changed = true;
      while (changed) {
        changed = false;
        for (const otherWindow of windows) {
          if (visited.has(otherWindow.id)) continue;

          // Check if this window overlaps with any window in the current group
          for (const groupWindow of group) {
            if (this.doWindowsOverlap(groupWindow, otherWindow)) {
              group.push(otherWindow);
              visited.add(otherWindow.id);
              changed = true;
              break;
            }
          }
        }
      }

      groups.push(group);
    }

    return groups;
  }

  private doWindowsOverlap(a: Window, b: Window): boolean {
    return !(
      a.x + a.w <= b.x ||
      b.x + b.w <= a.x ||
      a.y + a.h <= b.y ||
      b.y + b.h <= a.y
    );
  }

  private computeRectangleUnion(rectangles: Rectangle[]): Point[] {
    if (rectangles.length === 0) return [];
    if (rectangles.length === 1) {
      const r = rectangles[0];
      return [
        { x: r.x, y: r.y },
        { x: r.x + r.w, y: r.y },
        { x: r.x + r.w, y: r.y + r.h },
        { x: r.x, y: r.y + r.h },
      ];
    }

    // Use a simplified approach for axis-aligned rectangles
    return this.computeRectilinearUnion(rectangles);
  }

  private computeRectilinearUnion(rectangles: Rectangle[]): Point[] {
    // Get all unique x and y coordinates
    const xCoords = new Set<number>();
    const yCoords = new Set<number>();

    rectangles.forEach((rect) => {
      xCoords.add(rect.x);
      xCoords.add(rect.x + rect.w);
      yCoords.add(rect.y);
      yCoords.add(rect.y + rect.h);
    });

    const sortedX = Array.from(xCoords).sort((a, b) => a - b);
    const sortedY = Array.from(yCoords).sort((a, b) => a - b);

    // Create a grid and mark covered cells
    const grid: boolean[][] = [];
    for (let i = 0; i < sortedY.length - 1; i++) {
      grid[i] = [];
      for (let j = 0; j < sortedX.length - 1; j++) {
        // Check if this grid cell is covered by any rectangle
        const cellLeft = sortedX[j];
        const cellRight = sortedX[j + 1];
        const cellTop = sortedY[i];
        const cellBottom = sortedY[i + 1];

        grid[i][j] = rectangles.some(
          (rect) =>
            rect.x <= cellLeft &&
            rect.x + rect.w >= cellRight &&
            rect.y <= cellTop &&
            rect.y + rect.h >= cellBottom
        );
      }
    }

    // Find the outline by collecting boundary edges
    const horizontalEdges: {
      x1: number;
      x2: number;
      y: number;
      direction: "top" | "bottom";
    }[] = [];
    const verticalEdges: {
      y1: number;
      y2: number;
      x: number;
      direction: "left" | "right";
    }[] = [];

    for (let i = 0; i < grid.length; i++) {
      for (let j = 0; j < grid[i].length; j++) {
        if (grid[i][j]) {
          const left = sortedX[j];
          const right = sortedX[j + 1];
          const top = sortedY[i];
          const bottom = sortedY[i + 1];

          // Top edge (if cell above is empty or out of bounds)
          if (i === 0 || !grid[i - 1][j]) {
            horizontalEdges.push({
              x1: left,
              x2: right,
              y: top,
              direction: "top",
            });
          }

          // Bottom edge (if cell below is empty or out of bounds)
          if (i === grid.length - 1 || !grid[i + 1][j]) {
            horizontalEdges.push({
              x1: left,
              x2: right,
              y: bottom,
              direction: "bottom",
            });
          }

          // Left edge (if cell to the left is empty or out of bounds)
          if (j === 0 || !grid[i][j - 1]) {
            verticalEdges.push({
              y1: top,
              y2: bottom,
              x: left,
              direction: "left",
            });
          }

          // Right edge (if cell to the right is empty or out of bounds)
          if (j === grid[i].length - 1 || !grid[i][j + 1]) {
            verticalEdges.push({
              y1: top,
              y2: bottom,
              x: right,
              direction: "right",
            });
          }
        }
      }
    }

    // Trace the outline by connecting the edges
    return this.tracePolygonOutline(horizontalEdges, verticalEdges);
  }

  private tracePolygonOutline(
    horizontalEdges: {
      x1: number;
      x2: number;
      y: number;
      direction: "top" | "bottom";
    }[],
    verticalEdges: {
      y1: number;
      y2: number;
      x: number;
      direction: "left" | "right";
    }[]
  ): Point[] {
    if (horizontalEdges.length === 0 && verticalEdges.length === 0) return [];

    // Collect all corner points where edges meet
    const corners = new Set<string>();

    // Add endpoints of all edges
    horizontalEdges.forEach((edge) => {
      corners.add(`${edge.x1},${edge.y}`);
      corners.add(`${edge.x2},${edge.y}`);
    });

    verticalEdges.forEach((edge) => {
      corners.add(`${edge.x},${edge.y1}`);
      corners.add(`${edge.x},${edge.y2}`);
    });

    // Convert corner strings back to points
    const cornerPoints: Point[] = Array.from(corners).map((corner) => {
      const [x, y] = corner.split(",").map(Number);
      return { x, y };
    });

    if (cornerPoints.length === 0) return [];

    // Start from the leftmost-topmost point
    cornerPoints.sort((a, b) => (a.x === b.x ? a.y - b.y : a.x - b.x));

    const outline: Point[] = [];
    const visited = new Set<string>();
    let current = cornerPoints[0];

    while (true) {
      const currentKey = `${current.x},${current.y}`;
      if (visited.has(currentKey) && outline.length > 0) break;

      visited.add(currentKey);
      outline.push({ x: current.x, y: current.y });

      // Find the next point by following edges
      let next: Point | null = null;

      // Try to follow horizontal edges first (prefer clockwise traversal)
      for (const edge of horizontalEdges) {
        if (edge.direction === "top" && edge.y === current.y) {
          if (edge.x1 === current.x && !visited.has(`${edge.x2},${edge.y}`)) {
            next = { x: edge.x2, y: edge.y };
            break;
          }
          if (edge.x2 === current.x && !visited.has(`${edge.x1},${edge.y}`)) {
            next = { x: edge.x1, y: edge.y };
            break;
          }
        }
      }

      // If no horizontal edge found, try vertical edges
      if (!next) {
        for (const edge of verticalEdges) {
          if (edge.direction === "right" && edge.x === current.x) {
            if (edge.y1 === current.y && !visited.has(`${edge.x},${edge.y2}`)) {
              next = { x: edge.x, y: edge.y2 };
              break;
            }
            if (edge.y2 === current.y && !visited.has(`${edge.x},${edge.y1}`)) {
              next = { x: edge.x, y: edge.y1 };
              break;
            }
          }
        }
      }

      // If still no next point, try any adjacent unvisited corner
      if (!next) {
        for (const corner of cornerPoints) {
          const cornerKey = `${corner.x},${corner.y}`;
          if (
            !visited.has(cornerKey) &&
            this.areAdjacent(current, corner, horizontalEdges, verticalEdges)
          ) {
            next = corner;
            break;
          }
        }
      }

      if (!next) break;
      current = next;
    }

    return outline;
  }

  private areAdjacent(
    p1: Point,
    p2: Point,
    horizontalEdges: {
      x1: number;
      x2: number;
      y: number;
      direction: "top" | "bottom";
    }[],
    verticalEdges: {
      y1: number;
      y2: number;
      x: number;
      direction: "left" | "right";
    }[]
  ): boolean {
    // Check if there's a horizontal edge connecting these points
    if (p1.y === p2.y) {
      const minX = Math.min(p1.x, p2.x);
      const maxX = Math.max(p1.x, p2.x);
      return horizontalEdges.some(
        (edge) => edge.y === p1.y && edge.x1 === minX && edge.x2 === maxX
      );
    }

    // Check if there's a vertical edge connecting these points
    if (p1.x === p2.x) {
      const minY = Math.min(p1.y, p2.y);
      const maxY = Math.max(p1.y, p2.y);
      return verticalEdges.some(
        (edge) => edge.x === p1.x && edge.y1 === minY && edge.y2 === maxY
      );
    }

    return false;
  }

  private getBorderStyleForGroup(windows: Window[]): string {
    // Priority: focused > has-client > default
    if (windows.some((w) => w.focused)) {
      return "focused";
    }
    if (windows.some((w) => !!(w as any).client_id)) {
      return "has-client";
    }
    return "default";
  }

  private createBorderPath(polygon: Point[], style: string): SVGPathElement {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");

    // Build path data from polygon points
    let pathData = "";
    if (polygon.length > 0) {
      pathData = `M ${polygon[0].x} ${polygon[0].y}`;
      for (let i = 1; i < polygon.length; i++) {
        pathData += ` L ${polygon[i].x} ${polygon[i].y}`;
      }
      pathData += " Z";
    }

    path.setAttribute("d", pathData);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke-width", "2");

    // Set style based on group
    switch (style) {
      case "focused":
        path.setAttribute("stroke", "#4caf50");
        path.style.filter = "drop-shadow(0 0 12px rgba(76, 175, 80, 0.3))";
        break;
      case "has-client":
        path.setAttribute("stroke", "#2196f3");
        break;
      default:
        path.setAttribute("stroke", "rgba(255, 255, 255, 0.6)");
        break;
    }

    return path;
  }
}

// Initialize the overlay when the page loads
document.addEventListener("DOMContentLoaded", () => {
  new WindowOverlay();
  console.log("🪟 Window overlay initialized");
});
