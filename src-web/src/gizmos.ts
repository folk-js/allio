/**
 * Simple SVG gizmo utility for debug visualization
 */

interface Vec2 {
  x: number;
  y: number;
}

interface CircleOptions {
  fill?: string;
  stroke?: string;
  strokeWidth?: number;
  strokeDasharray?: string;
}

interface LineOptions {
  stroke?: string;
  strokeWidth?: number;
  strokeDasharray?: string;
}

interface RectOptions {
  fill?: string;
  stroke?: string;
  strokeWidth?: number;
}

interface TextOptions {
  fill?: string;
  fontSize?: number;
  fontFamily?: string;
  textAnchor?: "start" | "middle" | "end";
}

export class Gizmos {
  #svg: SVGElement;

  constructor(svg: SVGElement) {
    this.#svg = svg;
    this.#setupViewBox();
  }

  getSvg(): SVGElement {
    return this.#svg;
  }

  #setupViewBox(): void {
    this.#svg.setAttribute(
      "viewBox",
      `0 0 ${window.innerWidth} ${window.innerHeight}`
    );

    // Update viewBox on window resize
    window.addEventListener("resize", () => {
      this.#svg.setAttribute(
        "viewBox",
        `0 0 ${window.innerWidth} ${window.innerHeight}`
      );
    });
  }

  clear(): void {
    this.#svg.innerHTML = "";
  }

  circle(
    center: Vec2,
    radius: number,
    {
      fill = "red",
      stroke = "none",
      strokeWidth = 1,
      strokeDasharray,
    }: CircleOptions = {}
  ): void {
    const circle = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "circle"
    );
    circle.setAttribute("cx", center.x.toString());
    circle.setAttribute("cy", center.y.toString());
    circle.setAttribute("r", radius.toString());
    circle.setAttribute("fill", fill);
    circle.setAttribute("stroke", stroke);
    circle.setAttribute("stroke-width", strokeWidth.toString());
    if (strokeDasharray) {
      circle.setAttribute("stroke-dasharray", strokeDasharray);
    }
    this.#svg.appendChild(circle);
  }

  line(
    start: Vec2,
    end: Vec2,
    { stroke = "blue", strokeWidth = 2, strokeDasharray }: LineOptions = {}
  ): void {
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("x1", start.x.toString());
    line.setAttribute("y1", start.y.toString());
    line.setAttribute("x2", end.x.toString());
    line.setAttribute("y2", end.y.toString());
    line.setAttribute("stroke", stroke);
    line.setAttribute("stroke-width", strokeWidth.toString());
    if (strokeDasharray) {
      line.setAttribute("stroke-dasharray", strokeDasharray);
    }
    this.#svg.appendChild(line);
  }

  rect(
    x: number,
    y: number,
    width: number,
    height: number,
    { fill = "none", stroke = "white", strokeWidth = 2 }: RectOptions = {}
  ): void {
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.setAttribute("x", x.toString());
    rect.setAttribute("y", y.toString());
    rect.setAttribute("width", width.toString());
    rect.setAttribute("height", height.toString());
    rect.setAttribute("fill", fill);
    rect.setAttribute("stroke", stroke);
    rect.setAttribute("stroke-width", strokeWidth.toString());
    this.#svg.appendChild(rect);
  }

  text(
    content: string,
    pos: Vec2,
    {
      fill = "white",
      fontSize = 12,
      fontFamily = "monospace",
      textAnchor = "start",
    }: TextOptions = {}
  ): void {
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.setAttribute("x", pos.x.toString());
    text.setAttribute("y", pos.y.toString());
    text.setAttribute("fill", fill);
    text.setAttribute("font-size", fontSize.toString());
    text.setAttribute("font-family", fontFamily);
    text.setAttribute("text-anchor", textAnchor);
    text.textContent = content;
    this.#svg.appendChild(text);
  }

  /**
   * Draw a parabolic arc with arrow
   * Used for jump trajectories
   */
  parabola(
    start: Vec2,
    end: Vec2,
    duration: number,
    gravity: number,
    jumpVelocity: number,
    { stroke = "blue", strokeWidth = 2 }: LineOptions = {}
  ): void {
    const dx = end.x - start.x;
    const vx = dx / duration;
    const samples = 30;

    // Build path
    let pathData = `M ${start.x} ${start.y}`;
    for (let i = 1; i <= samples; i++) {
      const t = (i / samples) * duration;
      const x = start.x + vx * t;
      const y = start.y + jumpVelocity * t - 0.5 * gravity * t * t;
      pathData += ` L ${x} ${y}`;
    }

    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", pathData);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", stroke);
    path.setAttribute("stroke-width", strokeWidth.toString());
    this.#svg.appendChild(path);

    // Draw arrow head at the end
    const lastT = duration;
    const secondLastT = duration * 0.95;

    const lastX = start.x + vx * lastT;
    const lastY =
      start.y + jumpVelocity * lastT - 0.5 * gravity * lastT * lastT;

    const secondLastX = start.x + vx * secondLastT;
    const secondLastY =
      start.y +
      jumpVelocity * secondLastT -
      0.5 * gravity * secondLastT * secondLastT;

    const angle = Math.atan2(lastY - secondLastY, lastX - secondLastX);
    const arrowSize = 8;
    const arrowAngle = Math.PI / 6;

    const p1 = {
      x: lastX - arrowSize * Math.cos(angle - arrowAngle),
      y: lastY - arrowSize * Math.sin(angle - arrowAngle),
    };
    const p2 = {
      x: lastX - arrowSize * Math.cos(angle + arrowAngle),
      y: lastY - arrowSize * Math.sin(angle + arrowAngle),
    };

    const polygon = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "polygon"
    );
    polygon.setAttribute(
      "points",
      `${lastX},${lastY} ${p1.x},${p1.y} ${p2.x},${p2.y}`
    );
    polygon.setAttribute("fill", stroke);
    this.#svg.appendChild(polygon);
  }
}
