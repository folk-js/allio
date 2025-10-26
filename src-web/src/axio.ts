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

import {
  type ServerMessage,
  type ClientMessage,
  type ElementUpdate,
} from "./protocol.ts";

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
  click?(): Promise<void>;
  getChildren?(maxDepth?: number, maxChildren?: number): Promise<AXNode[]>;
}

// ElementUpdate is imported from protocol.ts

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

  // Tree cache - holds trees per window for stable state across focus changes
  private treeCache: Map<string, AXNode> = new Map();

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
   * Cleans up cache for closed windows
   */
  onWindowUpdate(callback: (windows: Window[]) => void): void {
    if (!this.listeners.has("window_update")) {
      this.listeners.set("window_update", new Set());
    }
    this.listeners.get("window_update")!.add((data: any) => {
      if (data.windows) {
        const windows = data.windows as Window[];

        // Track which windows were removed
        const oldWindowIds = new Set(this.windows.map((w) => w.id));
        const newWindowIds = new Set(windows.map((w: Window) => w.id));

        // Clean up cache for closed windows
        for (const oldId of oldWindowIds) {
          if (!newWindowIds.has(oldId)) {
            this.treeCache.delete(oldId);
            console.log(`🗑️ Removed cached tree for closed window ${oldId}`);
          }
        }

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
   * Generic method to send a request and wait for a response
   * Reduces boilerplate by centralizing Promise/listener/timeout logic
   */
  private async sendRequest<T>(
    msgType: string,
    data: Record<string, any>,
    timeout: number = 5000
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const responseType = `${msgType}_response`;

      const handler = (responseData: any) => {
        // Remove this specific handler
        const listeners = this.listeners.get(responseType);
        if (listeners) {
          listeners.delete(handler);
        }

        if (responseData.success) {
          resolve(responseData as T);
        } else {
          reject(new Error(responseData.error || "Request failed"));
        }
      };

      // Add temporary handler
      if (!this.listeners.has(responseType)) {
        this.listeners.set(responseType, new Set());
      }
      this.listeners.get(responseType)!.add(handler);

      // Send request
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        let message: ClientMessage;

        if (msgType === "write_to_element") {
          message = {
            type: "write_to_element",
            element_id: data.element_id,
            text: data.text,
          };
        } else if (msgType === "click_element") {
          message = {
            type: "click_element",
            element_id: data.element_id,
          };
        } else {
          reject(new Error(`Unknown message type: ${msgType}`));
          return;
        }

        this.ws.send(JSON.stringify(message));
      } else {
        reject(new Error("WebSocket not connected"));
        return;
      }

      // Timeout
      setTimeout(() => {
        const listeners = this.listeners.get(responseType);
        if (listeners) {
          listeners.delete(handler);
        }
        reject(new Error(`Timeout waiting for ${msgType} response`));
      }, timeout);
    });
  }

  /**
   * Get the root accessibility node for a window by window ID
   * Returns the cached root node (pushed by backend when window is focused)
   * If not cached, waits for the backend to push it (with timeout)
   * The root node has getChildren() method to fetch children on demand
   */
  async getRootNode(windowId: string, timeout: number = 5000): Promise<AXNode> {
    // Check cache first
    const cached = this.treeCache.get(windowId);
    if (cached) {
      console.log(`📦 Returning cached root for window ${windowId}`);
      return Promise.resolve(cached);
    }

    // Not cached - wait for backend to push it
    console.log(`⏳ Waiting for root node for window ${windowId}`);
    return new Promise((resolve, reject) => {
      const checkInterval = setInterval(() => {
        const root = this.treeCache.get(windowId);
        if (root) {
          clearInterval(checkInterval);
          resolve(root);
        }
      }, 100); // Check every 100ms

      // Timeout
      setTimeout(() => {
        clearInterval(checkInterval);
        reject(
          new Error(`Timeout waiting for root node for window ${windowId}`)
        );
      }, timeout);
    });
  }

  /**
   * @deprecated Use getRootNode() instead. The backend now pushes root nodes automatically.
   */
  async getTreeByWindowId(
    windowId: string,
    maxDepth?: number,
    maxChildrenPerLevel?: number
  ): Promise<AXNode> {
    console.warn(
      "[AXIO] getTreeByWindowId is deprecated, use getRootNode() instead"
    );
    return this.getRootNode(windowId);
  }

  /**
   * Attach operation methods to a node and its children recursively
   */
  private attachNodeMethods(node: AXNode): AXNode {
    // Attach setValue method
    (node as any).setValue = async (text: string) => {
      return this.writeByElementId(node.id, text);
    };

    // Attach click method
    (node as any).click = async () => {
      return this.clickElement(node.id);
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
        const message: ClientMessage = {
          type: "get_children",
          element_id: elementId,
          max_depth: maxDepth,
          max_children_per_level: maxChildren,
        };
        this.ws.send(JSON.stringify(message));
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
    await this.sendRequest("write_to_element", { element_id: elementId, text });
  }

  /**
   * Click/press an element by element ID
   */
  async clickElement(elementId: string): Promise<void> {
    await this.sendRequest("click_element", { element_id: elementId });
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
        const message: ClientMessage = {
          type: "set_clickthrough",
          enabled,
        };
        this.ws.send(JSON.stringify(message));
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
        const message: ClientMessage = {
          type: "watch_node",
          element_id: elementId,
          node_id: nodeId,
        };
        this.ws.send(JSON.stringify(message));
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
        const message: ClientMessage = {
          type: "unwatch_node",
          element_id: elementId,
        };
        this.ws.send(JSON.stringify(message));
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
   * Register callback for element updates (pushed from backend via AXObserver)
   * Called when a watched element's value, title, or other properties change
   * Also updates cached trees to keep them fresh
   */
  onElementUpdate(callback: (update: ElementUpdate) => void): void {
    if (!this.listeners.has("element_update")) {
      this.listeners.set("element_update", new Set());
    }
    this.listeners.get("element_update")!.add((data: any) => {
      if (data.update) {
        const update = data.update as ElementUpdate;

        // Apply update to cached trees
        this.applyCachedTreeUpdate(update);

        // Notify listeners
        callback(update);
      }
    });
  }

  /**
   * Apply element update to cached trees
   * Keeps trees fresh without refetching
   */
  private applyCachedTreeUpdate(update: ElementUpdate): void {
    // Search all cached trees for the element
    for (const tree of this.treeCache.values()) {
      const node = this.findNodeInTree(tree, update.element_id);
      if (node) {
        // Found the node - apply update
        switch (update.update_type) {
          case "ValueChanged":
            (node as any).value = update.value;
            break;
          case "TitleChanged":
            (node as any).title = update.title;
            break;
          case "ElementDestroyed":
            // Note: For destruction, we'd need to remove from parent's children
            // This is complex, so for now we just mark it
            // Frontend can handle this by removing from DOM
            console.log(
              `⚠️ Element ${update.element_id} destroyed in cached tree`
            );
            break;
        }
        break; // Found and updated, stop searching
      }
    }
  }

  /**
   * Recursively search tree for node with given ID
   */
  private findNodeInTree(node: AXNode, elementId: string): AXNode | null {
    if (node.id === elementId) {
      return node;
    }

    for (const child of node.children) {
      const found = this.findNodeInTree(child, elementId);
      if (found) return found;
    }

    return null;
  }

  /**
   * Clear cached tree for a specific window
   * Next request will fetch fresh tree from backend
   */
  clearTreeCache(windowId: string): void {
    this.treeCache.delete(windowId);
    console.log(`🗑️ Cleared tree cache for window ${windowId}`);
  }

  /**
   * Clear all cached trees
   * Useful for debugging or forced refresh
   */
  clearAllTreeCache(): void {
    this.treeCache.clear();
    console.log(`🗑️ Cleared all tree cache`);
  }

  // ============================================================================
  // Private Methods
  // ============================================================================

  private handleMessage(data: string): void {
    try {
      const message: ServerMessage = JSON.parse(data);

      // Type-safe message handling with discriminated union
      switch (message.type) {
        case "window_update":
          // Update internal window state
          this.windows = message.windows;
          this.focused = message.windows.find((w) => w.focused) || null;

          // Notify listeners
          const windowListeners = this.listeners.get("window_update");
          if (windowListeners) {
            windowListeners.forEach((callback) => callback(message));
          }
          break;

        case "window_root_update":
          // Attach methods to the root node
          const rootWithMethods = this.attachNodeMethods(message.root);
          // Cache the root node
          this.treeCache.set(message.window_id, rootWithMethods);
          console.log(
            `[AXIO] 📦 Cached root node for window ${message.window_id}`
          );

          // Notify listeners
          const rootListeners = this.listeners.get("window_root_update");
          if (rootListeners) {
            rootListeners.forEach((callback) => callback(message));
          }
          break;

        case "mouse_position":
          const mouseListeners = this.listeners.get("mouse_position");
          if (mouseListeners) {
            mouseListeners.forEach((callback) => callback(message));
          }
          break;

        case "element_update":
          const elementListeners = this.listeners.get("element_update");
          if (elementListeners) {
            elementListeners.forEach((callback) => callback(message));
          }
          // Apply update to cached tree
          this.applyCachedTreeUpdate(message.update);
          break;

        // Response messages
        case "get_children_response":
        case "write_to_element_response":
        case "click_element_response":
        case "set_clickthrough_response":
        case "watch_node_response":
        case "unwatch_node_response":
          // These are handled by request/response pattern in private methods
          const responseListeners = this.listeners.get(message.type);
          if (responseListeners) {
            responseListeners.forEach((callback) => callback(message));
          }
          break;

        default:
          console.warn("[AXIO] Unhandled message type:", message);
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
