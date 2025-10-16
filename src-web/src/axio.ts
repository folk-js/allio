/**
 * AXIO - Accessibility I/O Layer
 *
 * A principled, minimal accessibility tree interface based on a subset of ARIA.
 * This layer provides a clean abstraction over platform accessibility APIs.
 *
 * Design principles:
 * - Types mirror Rust backend exactly
 * - Based on ARIA role subset
 * - Immutable data structures
 * - No UI concerns (overlay rendering is separate)
 */

// ============================================================================
// Core Value Types
// ============================================================================

/**
 * Typed values from accessibility attributes
 * Matches Rust AXValue enum exactly
 */
export type AXValue =
  | { type: "String"; value: string }
  | { type: "Integer"; value: number }
  | { type: "Float"; value: number }
  | { type: "Boolean"; value: boolean };

/**
 * 2D position in screen coordinates
 */
export interface Position {
  x: number;
  y: number;
}

/**
 * 2D size dimensions
 */
export interface Size {
  width: number;
  height: number;
}

/**
 * Geometric bounds (position + size)
 */
export interface Bounds {
  position: Position;
  size: Size;
}

// ============================================================================
// ARIA Role Subset
// ============================================================================

/**
 * ARIA role subset covering common UI elements
 * Extensible for platform-specific roles via "unknown"
 */
export type AXRole =
  // Document structure
  | "application"
  | "document"
  | "window"
  | "group"
  // Interactive elements
  | "button"
  | "checkbox"
  | "radio"
  | "toggle"
  | "textbox"
  | "searchbox"
  | "slider"
  | "menu"
  | "menuitem"
  | "menubar"
  | "link"
  | "tab"
  | "tablist"
  // Static content
  | "text"
  | "heading"
  | "image"
  | "list"
  | "listitem"
  | "table"
  | "row"
  | "cell"
  // Other
  | "progressbar"
  | "scrollbar"
  | "unknown";

// ============================================================================
// Node Structure
// ============================================================================

/**
 * Core accessibility node
 *
 * Represents a single element in the accessibility tree with:
 * - Identity (id, role)
 * - Content (title, value, description)
 * - State (focused, enabled)
 * - Geometry (position, size)
 * - Tree structure (children)
 */
export interface AXNode {
  // Identity
  readonly id: string;
  readonly role: AXRole;
  readonly subrole?: string; // Platform-specific subtype

  // Content
  readonly title?: string;
  readonly value?: AXValue;
  readonly description?: string;
  readonly placeholder?: string;

  // State
  readonly focused: boolean;
  readonly enabled: boolean;
  readonly selected?: boolean;

  // Geometry (optional, not all nodes have screen position)
  readonly bounds?: Bounds;

  // Tree structure
  readonly children: ReadonlyArray<AXNode>;
}

/**
 * Root of an accessibility tree (represents an application window)
 */
export interface AXRoot extends AXNode {
  readonly role: "window" | "application";
  readonly processId: number;
  readonly windowId?: number;
}

// ============================================================================
// AXIO Client Class
// ============================================================================

/**
 * Main AXIO client
 *
 * Responsibilities:
 * - WebSocket connection to backend
 * - Deserialize and normalize accessibility data
 * - Provide clean API for querying nodes
 * - Event notifications for tree updates
 */
export class AXIO {
  private ws: WebSocket | null = null;
  private listeners: Map<string, Set<(data: any) => void>> = new Map();
  private reconnectTimer: number | null = null;
  private readonly reconnectDelay = 1000;

  constructor(private readonly wsUrl: string = "ws://localhost:3030") {}

  /**
   * Connect to the AXIO backend
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.wsUrl);

        this.ws.onopen = () => {
          console.log("[AXIO] Connected to backend");
          if (this.reconnectTimer !== null) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
          }
          resolve();
        };

        this.ws.onmessage = (event) => {
          this.handleMessage(event.data);
        };

        this.ws.onerror = (error) => {
          console.error("[AXIO] WebSocket error:", error);
          reject(error);
        };

        this.ws.onclose = () => {
          console.log("[AXIO] Disconnected from backend");
          this.scheduleReconnect();
        };
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Disconnect from backend
   */
  disconnect(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  /**
   * Subscribe to events from the backend
   */
  on(event: string, callback: (data: any) => void): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback);
  }

  /**
   * Unsubscribe from events
   */
  off(event: string, callback: (data: any) => void): void {
    const listeners = this.listeners.get(event);
    if (listeners) {
      listeners.delete(callback);
    }
  }

  /**
   * Send a message to the backend
   */
  send(message: any): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    } else {
      console.warn("[AXIO] Cannot send message: not connected");
    }
  }

  // ============================================================================
  // Private Methods
  // ============================================================================

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data);
      const event = message.event || message.type;

      if (event) {
        const listeners = this.listeners.get(event);
        if (listeners) {
          listeners.forEach((callback) => callback(message.data || message));
        }
      }
    } catch (error) {
      console.error("[AXIO] Failed to parse message:", error);
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer === null) {
      console.log(`[AXIO] Reconnecting in ${this.reconnectDelay}ms...`);
      this.reconnectTimer = window.setTimeout(() => {
        this.reconnectTimer = null;
        this.connect().catch((error) => {
          console.error("[AXIO] Reconnection failed:", error);
        });
      }, this.reconnectDelay);
    }
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Convert AXValue to string for display
 */
export function axValueToString(value: AXValue): string {
  switch (value.type) {
    case "String":
      return value.value;
    case "Integer":
    case "Float":
      return value.value.toString();
    case "Boolean":
      return value.value ? "true" : "false";
  }
}
