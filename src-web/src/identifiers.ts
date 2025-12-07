import { AXIO, AXWindow, WindowId } from "@axio/client";

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
  private axio = new AXIO();
  private container = document.getElementById("windowContainer")!;
  private labels = new Map<WindowId, HTMLElement>();
  private svg: SVGSVGElement;
  private borders = new Map<WindowId, SVGPathElement>();

  constructor() {
    this.svg = this.createSVG();
    this.container.appendChild(this.svg);
    this.connect();
  }

  private createSVG(): SVGSVGElement {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    Object.assign(svg.style, {
      position: "absolute",
      top: "0",
      left: "0",
      width: "100vw",
      height: "100vh",
      pointerEvents: "none",
      zIndex: "1000",
    });
    return svg;
  }

  private async connect() {
    await this.axio.connect();

    const render = () => this.render([...this.axio.windows.values()]);
    render();
    (["window:added", "window:changed", "window:removed"] as const).forEach(
      (e) => this.axio.on(e, render)
    );
  }

  private render(windows: AXWindow[]) {
    const visible = windows.filter((w) => w.bounds.w >= 50 && w.bounds.h >= 50);
    const currentIds = new Set(visible.map((w) => w.id));

    // Remove stale labels
    for (const [id, el] of this.labels) {
      if (!currentIds.has(id)) {
        el.remove();
        this.labels.delete(id);
      }
    }

    // Update labels
    visible.forEach((w) => this.updateLabel(w));

    // Update unified borders
    this.updateBorders(visible);
  }

  private updateLabel(w: AXWindow) {
    let el = this.labels.get(w.id);
    if (!el) {
      el = document.createElement("div");
      el.className = "window-label";
      this.container.appendChild(el);
      this.labels.set(w.id, el);
    }

    el.textContent = `○ ${w.title || "Untitled"} (${w.id})`;
    el.className = `window-label${w.focused ? " focused" : ""}`;
    Object.assign(el.style, {
      position: "absolute",
      left: `${w.bounds.x}px`,
      top: `${w.bounds.y - 24}px`,
      display: "block",
    });
  }

  private updateBorders(windows: AXWindow[]) {
    this.borders.forEach((p) => p.remove());
    this.borders.clear();

    if (windows.length === 0) return;

    const groups = this.groupOverlapping(windows);
    groups.forEach((group, i) => {
      const rects = group.map((w) => ({
        x: w.bounds.x,
        y: w.bounds.y,
        w: w.bounds.w,
        h: w.bounds.h,
      }));
      const polygon = this.computeUnion(rects);

      if (polygon.length > 0) {
        const path = this.createPath(
          polygon,
          group.some((w) => w.focused)
        );
        this.svg.appendChild(path);
        this.borders.set(i, path);

        // Hide labels inside polygon (except focused)
        if (group.length > 1) {
          group.forEach((w) => {
            const label = this.labels.get(w.id);
            if (label) {
              const inside = this.pointInPolygon(
                { x: w.bounds.x, y: w.bounds.y - 24 },
                polygon
              );
              label.style.display = !inside || w.focused ? "block" : "none";
            }
          });
        }
      }
    });
  }

  private groupOverlapping(windows: AXWindow[]): AXWindow[][] {
    const groups: AXWindow[][] = [];
    const visited = new Set<WindowId>();

    for (const w of windows) {
      if (visited.has(w.id)) continue;
      const group = [w];
      visited.add(w.id);

      let changed = true;
      while (changed) {
        changed = false;
        for (const other of windows) {
          if (visited.has(other.id)) continue;
          if (group.some((g) => this.overlaps(g, other))) {
            group.push(other);
            visited.add(other.id);
            changed = true;
          }
        }
      }
      groups.push(group);
    }
    return groups;
  }

  private overlaps(a: AXWindow, b: AXWindow): boolean {
    return !(
      a.bounds.x + a.bounds.w <= b.bounds.x ||
      b.bounds.x + b.bounds.w <= a.bounds.x ||
      a.bounds.y + a.bounds.h <= b.bounds.y ||
      b.bounds.y + b.bounds.h <= a.bounds.y
    );
  }

  private computeUnion(rects: Rectangle[]): Point[] {
    if (rects.length === 0) return [];
    if (rects.length === 1) {
      const r = rects[0];
      return [
        { x: r.x, y: r.y },
        { x: r.x + r.w, y: r.y },
        { x: r.x + r.w, y: r.y + r.h },
        { x: r.x, y: r.y + r.h },
      ];
    }

    // Grid-based union for rectilinear shapes
    const xs = [...new Set(rects.flatMap((r) => [r.x, r.x + r.w]))].sort(
      (a, b) => a - b
    );
    const ys = [...new Set(rects.flatMap((r) => [r.y, r.y + r.h]))].sort(
      (a, b) => a - b
    );

    const grid: boolean[][] = ys
      .slice(0, -1)
      .map((_, i) =>
        xs
          .slice(0, -1)
          .map((_, j) =>
            rects.some(
              (r) =>
                r.x <= xs[j] &&
                r.x + r.w >= xs[j + 1] &&
                r.y <= ys[i] &&
                r.y + r.h >= ys[i + 1]
            )
          )
      );

    // Collect boundary edges
    const hEdges: { x1: number; x2: number; y: number }[] = [];
    const vEdges: { y1: number; y2: number; x: number }[] = [];

    for (let i = 0; i < grid.length; i++) {
      for (let j = 0; j < grid[i].length; j++) {
        if (!grid[i][j]) continue;
        const [l, r, t, b] = [xs[j], xs[j + 1], ys[i], ys[i + 1]];
        if (i === 0 || !grid[i - 1][j]) hEdges.push({ x1: l, x2: r, y: t });
        if (i === grid.length - 1 || !grid[i + 1][j])
          hEdges.push({ x1: l, x2: r, y: b });
        if (j === 0 || !grid[i][j - 1]) vEdges.push({ y1: t, y2: b, x: l });
        if (j === grid[i].length - 1 || !grid[i][j + 1])
          vEdges.push({ y1: t, y2: b, x: r });
      }
    }

    return this.traceOutline(hEdges, vEdges);
  }

  private traceOutline(
    hEdges: { x1: number; x2: number; y: number }[],
    vEdges: { y1: number; y2: number; x: number }[]
  ): Point[] {
    const corners = new Set<string>();
    hEdges.forEach((e) => {
      corners.add(`${e.x1},${e.y}`);
      corners.add(`${e.x2},${e.y}`);
    });
    vEdges.forEach((e) => {
      corners.add(`${e.x},${e.y1}`);
      corners.add(`${e.x},${e.y2}`);
    });

    const points = [...corners].map((c) => {
      const [x, y] = c.split(",").map(Number);
      return { x, y };
    });
    if (points.length === 0) return [];

    points.sort((a, b) => (a.x === b.x ? a.y - b.y : a.x - b.x));

    const outline: Point[] = [];
    const visited = new Set<string>();
    let current = points[0];

    while (true) {
      const key = `${current.x},${current.y}`;
      if (visited.has(key) && outline.length > 0) break;
      visited.add(key);
      outline.push({ ...current });

      // Find next unvisited connected point
      let next: Point | null = null;
      for (const e of hEdges) {
        if (e.y === current.y) {
          if (e.x1 === current.x && !visited.has(`${e.x2},${e.y}`)) {
            next = { x: e.x2, y: e.y };
            break;
          }
          if (e.x2 === current.x && !visited.has(`${e.x1},${e.y}`)) {
            next = { x: e.x1, y: e.y };
            break;
          }
        }
      }
      if (!next) {
        for (const e of vEdges) {
          if (e.x === current.x) {
            if (e.y1 === current.y && !visited.has(`${e.x},${e.y2}`)) {
              next = { x: e.x, y: e.y2 };
              break;
            }
            if (e.y2 === current.y && !visited.has(`${e.x},${e.y1}`)) {
              next = { x: e.x, y: e.y1 };
              break;
            }
          }
        }
      }
      if (!next) break;
      current = next;
    }
    return outline;
  }

  private pointInPolygon(p: Point, poly: Point[]): boolean {
    let inside = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      if (
        poly[i].y > p.y !== poly[j].y > p.y &&
        p.x <
          ((poly[j].x - poly[i].x) * (p.y - poly[i].y)) /
            (poly[j].y - poly[i].y) +
            poly[i].x
      ) {
        inside = !inside;
      }
    }
    return inside;
  }

  private createPath(polygon: Point[], focused: boolean): SVGPathElement {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    const d =
      polygon.length > 0
        ? `M ${polygon[0].x} ${polygon[0].y}${polygon
            .slice(1)
            .map((p) => ` L ${p.x} ${p.y}`)
            .join("")} Z`
        : "";
    path.setAttribute("d", d);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke-width", "2");
    path.setAttribute(
      "stroke",
      focused ? "#4caf50" : "rgba(255, 255, 255, 0.6)"
    );
    if (focused)
      path.style.filter = "drop-shadow(0 0 12px rgba(76, 175, 80, 0.3))";
    return path;
  }
}

document.addEventListener("DOMContentLoaded", () => {
  new WindowOverlay();
  console.log("🪟 Window overlay initialized");
});
