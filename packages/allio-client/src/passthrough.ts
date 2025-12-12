/**
 * AllioPassthrough - Declarative pointer event handling for overlay UIs
 *
 * Automatically manages passthrough state based on mode and DOM attributes.
 *
 * Modes:
 * - `auto`: Uses `ax-io` attribute to determine passthrough
 * - `opaque`: Always capture events (overlay receives them)
 * - `transparent`: Always pass through to underlying apps
 * - `outside`: Pass through when OUTSIDE window geometry, capture when inside (for drawing in empty space)
 * - `inside`: Pass through when INSIDE window geometry, capture when outside
 *
 * DOM Attribute (for auto mode):
 * - `ax-io="opaque"`: Element captures pointer events
 * - `ax-io="transparent"`: Element passes pointer events through (can create "holes" in opaque regions)
 *
 * Usage:
 *   const passthrough = new AllioPassthrough(allio);
 *
 *   // Change mode
 *   passthrough.mode = "opaque";     // Always capture
 *   passthrough.mode = "transparent"; // Always pass through
 *   passthrough.mode = "outside";     // Pass through outside windows (sand demo)
 *   passthrough.mode = "auto";        // Back to DOM-based (default)
 *
 *   // In HTML (for auto mode):
 *   <div ax-io="opaque">Captures clicks</div>
 *   <button ax-io="opaque">Interactive</button>
 *   <div ax-io="opaque">
 *     <span ax-io="transparent">This part passes through</span>
 *   </div>
 */

import type { Allio } from "./index";

export type PassthroughMode =
  | "auto"
  | "opaque"
  | "transparent"
  | "outside"
  | "inside";

export class AllioPassthrough {
  private allio: Allio;
  private _mode: PassthroughMode = "auto";
  private lastState: boolean | null = null;
  private cleanupMouseListener: (() => void) | null = null;

  constructor(
    allio: Allio,
    options: {
      /** Initial mode (default: "auto") */
      mode?: PassthroughMode;
    } = {}
  ) {
    this.allio = allio;
    this._mode = options.mode ?? "auto";

    this.setupMouseListener();
  }

  /** Get current mode */
  get mode(): PassthroughMode {
    return this._mode;
  }

  /** Set mode - immediately updates passthrough state */
  set mode(value: PassthroughMode) {
    this._mode = value;
    // Clear last state to force re-evaluation
    this.lastState = null;
  }

  private setupMouseListener() {
    const handler = ({ x, y }: { x: number; y: number }) => {
      this.handleMouseMove(x, y);
    };

    this.allio.on("mouse:position", handler);
    this.cleanupMouseListener = () => {
      this.allio.off("mouse:position", handler);
    };
  }

  private handleMouseMove(x: number, y: number) {
    const shouldPassthrough = this.computePassthrough(x, y);

    // Only send if state changed (Rust side also dedupes, but this saves a message)
    if (this.lastState !== shouldPassthrough) {
      this.lastState = shouldPassthrough;
      // Delay 1 frame to allow mouse exit and other events to fire as Rust will set this
      // window to 'non key' when passthrough is disabled and this interrupts ... something.
      if (shouldPassthrough) {
        requestAnimationFrame(() => this.allio.setPassthrough(true));
      } else {
        this.allio.setPassthrough(false);
      }
    }
  }

  /**
   * Compute whether pointer events should pass through at the given coordinates.
   */
  private computePassthrough(x: number, y: number): boolean {
    switch (this._mode) {
      case "opaque":
        return false; // Never pass through

      case "transparent":
        return true; // Always pass through

      case "outside":
        // Pass through when OUTSIDE any window (for drawing in empty space)
        return this.isInsideAnyWindow(x, y);

      case "inside":
        // Pass through when INSIDE any window
        return !this.isInsideAnyWindow(x, y);

      case "auto":
      default:
        return this.computeAutoPassthrough(x, y);
    }
  }

  /**
   * Check if point is inside any tracked window's geometry.
   */
  private isInsideAnyWindow(x: number, y: number): boolean {
    for (const win of this.allio.windows.values()) {
      const b = win.bounds;
      if (x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) {
        return true;
      }
    }
    return false;
  }

  /**
   * Compute passthrough for auto mode using ax-io attribute.
   * Single .closest() lookup for efficiency.
   */
  private computeAutoPassthrough(x: number, y: number): boolean {
    // Get all elements at the point (top to bottom in stacking order)
    const elements = document.elementsFromPoint(x, y);

    for (const element of elements) {
      // Find closest ancestor with ax-io attribute
      const allioElement = element.closest("[ax-io]");
      if (allioElement) {
        const value = allioElement.getAttribute("ax-io");
        if (value === "opaque") return false; // Capture
        if (value === "transparent") return true; // Pass through
      }
    }

    // No attribute found - default to transparent
    return true;
  }

  /** Clean up resources */
  destroy(): void {
    this.cleanupMouseListener?.();
    this.cleanupMouseListener = null;
  }
}
