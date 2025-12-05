/**
 * Occlusion Manager - CSS-driven window occlusion for overlay UIs
 *
 * Creates shared SVG clipPath definitions based on window z-order.
 * Elements just reference their layer's clipPath for hardware-accelerated clipping.
 *
 * Usage:
 *   const occlusion = new OcclusionManager(axio);
 *   container.style.clipPath = occlusion.getClipPath(windowId);
 */

import type { AXIO, AXWindow, WindowId } from "./index";

type Rect = { x: number; y: number; w: number; h: number };

export class OcclusionManager {
  private axio: AXIO;
  private svgDefs: SVGDefsElement;
  private svg: SVGSVGElement;
  private clipPaths = new Map<string, SVGClipPathElement>();

  constructor(axio: AXIO) {
    this.axio = axio;

    // Create hidden SVG for clipPath definitions
    this.svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    this.svg.setAttribute("width", "0");
    this.svg.setAttribute("height", "0");
    this.svg.style.position = "absolute";
    this.svg.style.pointerEvents = "none";

    this.svgDefs = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "defs"
    );
    this.svg.appendChild(this.svgDefs);
    document.body.appendChild(this.svg);

    // Listen to window events to update clipPaths
    this.axio.on("sync:init", () => this.update());
    this.axio.on("window:added", () => this.update());
    this.axio.on("window:changed", () => this.update());
    this.axio.on("window:removed", () => this.update());
    this.update();
  }

  /** Get the CSS clip-path value for a window */
  getClipPath(windowId: WindowId): string {
    const clipPath = this.clipPaths.get(windowId);
    if (!clipPath) return "";
    return `url(#${clipPath.id})`;
  }

  /** Get z-index for a window (higher = more in front) */
  getZIndex(windowId: WindowId): number {
    const index = this.axio.depthOrder.indexOf(windowId);
    if (index === -1) return 0;
    return 1000 - index;
  }

  /** Update all clipPath definitions based on current window state */
  private update() {
    const windows = this.getWindows();
    const currentIds = new Set(windows.map((w) => w.id));

    // Remove clipPaths for closed windows
    for (const [id, clipPath] of this.clipPaths) {
      if (!currentIds.has(id)) {
        clipPath.remove();
        this.clipPaths.delete(id);
      }
    }

    // Update/create clipPaths for each window
    for (let i = 0; i < windows.length; i++) {
      const window = windows[i];
      const windowsInFront = windows.slice(0, i);
      this.updateClipPath(window, windowsInFront);
    }
  }

  /** Get windows sorted by z-order (frontmost first) */
  private getWindows(): AXWindow[] {
    return this.axio.depthOrder
      .map((id) => this.axio.windows.get(id))
      .filter((w): w is AXWindow => !!w);
  }

  /** Update or create clipPath for a specific window */
  private updateClipPath(window: AXWindow, windowsInFront: AXWindow[]) {
    let clipPath = this.clipPaths.get(window.id);

    if (!clipPath) {
      clipPath = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "clipPath"
      );
      clipPath.id = `occlusion-${window.id}`;
      this.svgDefs.appendChild(clipPath);
      this.clipPaths.set(window.id, clipPath);
    }

    // Clear existing content
    clipPath.innerHTML = "";

    // If frontmost window, no clipping needed - use a huge rect
    if (windowsInFront.length === 0) {
      const rect = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "rect"
      );
      rect.setAttribute("x", "-10000");
      rect.setAttribute("y", "-10000");
      rect.setAttribute("width", "20000");
      rect.setAttribute("height", "20000");
      clipPath.appendChild(rect);
      return;
    }

    // Build path with holes for windows in front
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");

    // Convert to window-relative coordinates
    const rects = windowsInFront.map((fw) => ({
      x: fw.bounds.x - window.bounds.x,
      y: fw.bounds.y - window.bounds.y,
      w: fw.bounds.w,
      h: fw.bounds.h,
    }));

    // Compute union to handle overlapping windows
    const unionRects =
      rects.length === 1 ? rects : this.computeRectUnion(rects);

    // Build path string
    const pathParts: string[] = [];

    // Outer boundary (clockwise) - huge rect
    pathParts.push("M -5000 -5000 L 5000 -5000 L 5000 5000 L -5000 5000 Z");

    // Each union rect as counter-clockwise hole
    for (const rect of unionRects) {
      const { x, y, w, h } = rect;
      pathParts.push(
        `M ${x} ${y} L ${x} ${y + h} L ${x + w} ${y + h} L ${x + w} ${y} Z`
      );
    }

    path.setAttribute("d", pathParts.join(" "));
    path.setAttribute("clip-rule", "evenodd");
    clipPath.appendChild(path);
  }

  /** Compute union of axis-aligned rectangles, returning non-overlapping rects */
  private computeRectUnion(rects: Rect[]): Rect[] {
    if (rects.length <= 1) return rects;

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

    // Create grid and mark covered cells
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

    // Extract non-overlapping rectangles using greedy approach
    const result: Rect[] = [];

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

  /** Clean up resources */
  destroy() {
    this.svg.remove();
    this.clipPaths.clear();
  }
}
