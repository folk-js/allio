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
// Window Structure
// ============================================================================

/**
 * Window information
 *
 * Represents a system window with its metadata and geometry.
 * Windows are the entry point to accessibility trees via their process_id.
 */
export interface Window {
  readonly id: string; // System window ID
  readonly title: string; // Window title
  readonly app_name: string; // Application name
  readonly x: number; // X position
  readonly y: number; // Y position
  readonly w: number; // Width
  readonly h: number; // Height
  readonly focused: boolean; // Is this window focused?
  readonly process_id: number; // PID for accessing accessibility tree
}

// ============================================================================
// Node Structure
// ============================================================================

/**
 * Core accessibility node
 *
 * Each node has a unique ID (UUID from ElementRegistry) for direct access.
 * Forms a tree structure via the children field and parent_id.
 */
export interface AXNode {
  // Identity - UUID from ElementRegistry (for direct lookup)
  readonly id: string; // UUID from ElementRegistry
  readonly parent_id?: string; // UUID of parent element (None for root)

  // Role information
  readonly role: AXRole;
  readonly subrole?: string; // Platform-specific subtype (or native name for unknown roles)

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
  readonly children_count: number; // Total number of children (whether loaded or not)
  readonly children: ReadonlyArray<AXNode>; // Loaded children (may be empty even if children_count > 0)

  // Operations (set by AXIO when creating nodes)
  setValue?(text: string): Promise<void>;
  getChildren?(maxDepth?: number, maxChildren?: number): Promise<AXNode[]>;
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

  // Window state (always up-to-date)
  public windows: readonly Window[] = [];
  public focused: Window | null = null;

  constructor(private readonly wsUrl: string = "ws://localhost:3030/ws") {}

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
   * Register callback for window updates
   * Also updates axio.windows and axio.focused automatically
   */
  onWindowUpdate(callback: (windows: Window[]) => void): void {
    if (!this.listeners.has("window_update")) {
      this.listeners.set("window_update", new Set());
    }
    this.listeners.get("window_update")!.add((data: any) => {
      if (data.windows) {
        const windows = data.windows as Window[];

        // Update internal state
        this.windows = windows;
        this.focused = windows.find((w: Window) => w.focused) || null;

        // Notify listeners
        callback(windows);
      }
    });
  }

  /**
   * Register callback for global mouse position updates
   * Mouse position is tracked system-wide, even when window is not focused
   */
  onMousePosition(callback: (x: number, y: number) => void): void {
    if (!this.listeners.has("mouse_position")) {
      this.listeners.set("mouse_position", new Set());
    }
    this.listeners.get("mouse_position")!.add((data: any) => {
      if (data.x !== undefined && data.y !== undefined) {
        callback(data.x, data.y);
      }
    });
  }

  /**
   * Register callback for tree changes (pushed from backend when focus changes)
   */
  onTreeChanged(callback: (pid: number, tree: AXNode) => void): void {
    if (!this.listeners.has("tree_changed")) {
      this.listeners.set("tree_changed", new Set());
    }
    this.listeners.get("tree_changed")!.add((data: any) => {
      if (data.pid !== undefined && data.tree !== undefined) {
        // Attach methods to the tree
        const tree = this.attachNodeMethods(data.tree);
        callback(data.pid, tree);
      }
    });
  }

  // ============================================================================
  // AXIO Protocol Methods
  // ============================================================================

  /**
   * Get accessibility tree for a window by window ID
   * Uses cached window element as root - faster and more accurate
   * Returns hierarchical tree structure with children
   * All nodes have setValue() and getChildren() methods attached for easy operations
   */
  async getTreeByWindowId(
    windowId: string,
    maxDepth: number = 50,
    maxChildrenPerLevel: number = 2000
  ): Promise<AXNode> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        // Remove this specific handler
        const listeners = this.listeners.get("accessibility_tree_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success && data.tree) {
          // Attach methods to all nodes in the tree
          const tree = this.attachNodeMethods(data.tree);
          resolve(tree);
        } else {
          reject(new Error(data.error || "Failed to get tree"));
        }
      };

      // Add temporary handler
      if (!this.listeners.has("accessibility_tree_response")) {
        this.listeners.set("accessibility_tree_response", new Set());
      }
      this.listeners.get("accessibility_tree_response")!.add(handler);

      // Send request
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "get_accessibility_tree",
            window_id: windowId,
            max_depth: maxDepth,
            max_children_per_level: maxChildrenPerLevel,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      // Timeout after 10s
      setTimeout(() => {
        const listeners = this.listeners.get("accessibility_tree_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for tree response"));
      }, 10000);
    });
  }

  /**
   * Attach operation methods to a node and its children recursively
   */
  private attachNodeMethods(node: AXNode): AXNode {
    // Attach setValue method
    (node as any).setValue = async (text: string) => {
      return this.writeByElementId(node.id, text);
    };

    // Attach getChildren method
    (node as any).getChildren = async (
      maxDepth: number = 1,
      maxChildren: number = 2000
    ) => {
      const children = await this.getChildrenByElementId(
        node.id,
        maxDepth,
        maxChildren
      );
      // Attach methods to the newly loaded children
      return children.map((child) => this.attachNodeMethods(child));
    };

    // Recursively attach to children
    if (node.children && node.children.length > 0) {
      (node as any).children = node.children.map((child) =>
        this.attachNodeMethods(child)
      );
    }

    return node;
  }

  /**
   * Get children of a specific node by element ID
   * Returns immediate children with their children_count populated but not loaded
   */
  async getChildrenByElementId(
    elementId: string,
    maxDepth: number = 1,
    maxChildren: number = 2000
  ): Promise<AXNode[]> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        // Remove this specific handler
        const listeners = this.listeners.get("get_children_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success && data.children) {
          resolve(data.children);
        } else {
          reject(new Error(data.error || "Failed to get children"));
        }
      };

      // Add temporary handler
      if (!this.listeners.has("get_children_response")) {
        this.listeners.set("get_children_response", new Set());
      }
      this.listeners.get("get_children_response")!.add(handler);

      // Send request
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "get_children",
            element_id: elementId,
            max_depth: maxDepth,
            max_children_per_level: maxChildren,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      // Timeout after 10s
      setTimeout(() => {
        const listeners = this.listeners.get("get_children_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for children response"));
      }, 10000);
    });
  }

  /**
   * Write text to an element by element ID
   */
  async writeByElementId(elementId: string, text: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        // Remove this specific handler
        const listeners = this.listeners.get("accessibility_write_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success) {
          resolve();
        } else {
          reject(new Error(data.error || "Failed to write"));
        }
      };

      // Add temporary handler
      if (!this.listeners.has("accessibility_write_response")) {
        this.listeners.set("accessibility_write_response", new Set());
      }
      this.listeners.get("accessibility_write_response")!.add(handler);

      // Send request
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "write_to_element",
            element_id: elementId,
            text,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      // Timeout after 5s
      setTimeout(() => {
        const listeners = this.listeners.get("accessibility_write_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for write response"));
      }, 5000);
    });
  }

  /**
   * Set clickthrough state (window transparency to mouse events)
   */
  async setClickthrough(enabled: boolean): Promise<void> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        // Remove this specific handler
        const listeners = this.listeners.get("set_clickthrough_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success) {
          resolve();
        } else {
          reject(new Error(data.error || "Failed to set clickthrough"));
        }
      };

      // Add temporary handler
      if (!this.listeners.has("set_clickthrough_response")) {
        this.listeners.set("set_clickthrough_response", new Set());
      }
      this.listeners.get("set_clickthrough_response")!.add(handler);

      // Send request
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "set_clickthrough",
            enabled,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      // Timeout after 2s (faster timeout for UI responsiveness)
      setTimeout(() => {
        const listeners = this.listeners.get("set_clickthrough_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for clickthrough response"));
      }, 2000);
    });
  }

  /**
   * Watch a node for changes by element ID
   * When the node changes, `onNodeUpdated` callbacks will fire
   */
  async watchNodeByElementId(elementId: string, nodeId: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        const listeners = this.listeners.get("watch_node_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success) {
          resolve();
        } else {
          reject(new Error(data.error || "Failed to watch node"));
        }
      };

      if (!this.listeners.has("watch_node_response")) {
        this.listeners.set("watch_node_response", new Set());
      }
      this.listeners.get("watch_node_response")!.add(handler);

      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "watch_node",
            element_id: elementId,
            node_id: nodeId,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      setTimeout(() => {
        const listeners = this.listeners.get("watch_node_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for watch response"));
      }, 5000);
    });
  }

  /**
   * Stop watching a node by element ID
   */
  async unwatchNodeByElementId(elementId: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        const listeners = this.listeners.get("unwatch_node_response");
        if (listeners) {
          listeners.delete(handler);
        }

        if (data.success) {
          resolve();
        } else {
          reject(new Error("Failed to unwatch node"));
        }
      };

      if (!this.listeners.has("unwatch_node_response")) {
        this.listeners.set("unwatch_node_response", new Set());
      }
      this.listeners.get("unwatch_node_response")!.add(handler);

      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(
          JSON.stringify({
            msg_type: "unwatch_node",
            element_id: elementId,
          })
        );
      } else {
        reject(new Error("WebSocket not connected"));
      }

      setTimeout(() => {
        const listeners = this.listeners.get("unwatch_node_response");
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error("Timeout waiting for unwatch response"));
      }, 5000);
    });
  }

  /**
   * Register callback for node updates (pushed from backend via AXObserver)
   * Called when a watched node's value, bounds, or state changes
   */
  onNodeUpdated(callback: (update: any) => void): void {
    if (!this.listeners.has("node_updated")) {
      this.listeners.set("node_updated", new Set());
    }
    this.listeners.get("node_updated")!.add((data: any) => {
      if (data.update) {
        callback(data.update);
      }
    });
  }

  // ============================================================================
  // Private Methods
  // ============================================================================

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data);

      // Determine event type from various field names
      let event = message.event_type || message.msg_type;

      // Special case: window updates (has 'windows' array but no explicit type)
      if (!event && message.windows) {
        event = "window_update";
        // Always update internal window state, even if no listeners
        const windows = message.windows as Window[];
        this.windows = windows;
        this.focused = windows.find((w: Window) => w.focused) || null;
      }

      if (event) {
        const listeners = this.listeners.get(event);
        if (listeners) {
          listeners.forEach((callback) => callback(message.data || message));
        }
      } else {
        // Log unhandled messages for debugging
        console.warn("[AXIO] Received message without event type:", message);
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
