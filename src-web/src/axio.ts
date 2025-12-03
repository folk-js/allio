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
  type Window,
  GetChildren,
  WriteToElement,
  ClickElement,
  SetClickthrough,
  WatchNode,
  UnwatchNode,
  GetElementAtPosition,
} from "./protocol.ts";

// Re-export protocol types for convenience
export type { Window, ElementUpdate };

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

// Window type is imported from protocol.ts
// We extend it internally with root property when needed

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
  readonly label?: string;
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
 * Type-safe RPC helper for request/response pairs
 */
type RPCConfig<Req> = {
  requestType: ClientMessage["type"];
  responseType: ServerMessage["type"];
  buildRequest: (data: Req) => ClientMessage;
  timeout?: number;
};

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

  // Request correlation
  private requestCounter = 0;
  private pendingRequests: Map<
    string,
    { resolve: (data: any) => void; reject: (error: Error) => void }
  > = new Map();

  // Window state
  private windowsInternal: Window[] = [];
  public focused: Window | null = null;

  constructor(private readonly wsUrl: string = "ws://localhost:3030/ws") {}

  /** Generate unique request ID */
  private generateRequestId(): string {
    return `r${++this.requestCounter}`;
  }

  /**
   * Get current windows (with roots automatically populated)
   */
  get windows(): ReadonlyArray<Window> {
    return this.windowsInternal;
  }

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
   * Helper to register event listeners with type safety
   */
  private registerListener<T>(
    eventType: string,
    callback: (data: T) => void,
    processor?: (rawData: any) => T | null
  ): void {
    if (!this.listeners.has(eventType)) {
      this.listeners.set(eventType, new Set());
    }
    this.listeners.get(eventType)!.add((data: any) => {
      const processed = processor ? processor(data) : data;
      if (processed !== null) {
        callback(processed as T);
      }
    });
  }

  /**
   * Register callback for window updates
   * Also updates axio.windows and axio.focused automatically
   * Preserves roots for existing windows
   */
  onWindowUpdate(callback: (windows: Window[]) => void): void {
    this.registerListener<Window[]>("window_update", callback, (data) => {
      if (!data.windows) return null;

      const incomingWindows = data.windows as Window[];

      // Create a map of existing windows with their roots
      const existingRoots = new Map<string, AXNode>();
      for (const win of this.windowsInternal) {
        if (win.root) {
          existingRoots.set(win.id, win.root);
        }
      }

      // Merge incoming windows with existing roots
      this.windowsInternal = incomingWindows.map((win) => ({
        ...win,
        root: existingRoots.get(win.id),
      }));

      // Track previous focused window
      const previousFocused = this.focused;

      // Update focused window
      this.focused = this.windowsInternal.find((w) => w.focused) || null;

      // Notify focused window change listeners if changed
      if (previousFocused?.id !== this.focused?.id) {
        this.notifyFocusedWindowChange(this.focused);
      }

      return this.windowsInternal;
    });
  }

  /**
   * Register callback for focused window changes
   * Called whenever the focused window changes (or becomes null)
   */
  onFocusedWindowChange(
    callback: (focusedWindow: Window | null) => void
  ): void {
    this.registerListener<Window | null>("focused_window_change", callback);
  }

  /**
   * Notify all focused window change listeners
   */
  private notifyFocusedWindowChange(focusedWindow: Window | null): void {
    const listeners = this.listeners.get("focused_window_change");
    if (listeners) {
      listeners.forEach((callback) => callback(focusedWindow));
    }
  }

  /**
   * Register callback for global mouse position updates
   * Mouse position is tracked system-wide, even when window is not focused
   */
  onMousePosition(callback: (x: number, y: number) => void): void {
    this.registerListener<{ x: number; y: number }>(
      "mouse_position",
      (data) => callback(data.x, data.y),
      (data) => {
        if (data.x !== undefined && data.y !== undefined) {
          return { x: data.x, y: data.y };
        }
        return null;
      }
    );
  }

  // ============================================================================
  // AXIO Protocol Methods
  // ============================================================================

  /**
   * Type-safe RPC method for request/response pairs
   * Uses request_id for correlation to prevent race conditions
   */
  private async rpc<Req, Res extends { success: boolean; error?: string }>(
    config: RPCConfig<Req>,
    request: Req
  ): Promise<Res> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error("WebSocket not connected"));
        return;
      }

      const requestId = this.generateRequestId();

      // Store pending request handler
      this.pendingRequests.set(requestId, {
        resolve: (responseData: any) => {
          if (responseData.success) {
            resolve(responseData as Res);
          } else {
            reject(new Error(responseData.error || "Request failed"));
          }
        },
        reject,
      });

      // Build and send request with request_id
      const message = config.buildRequest({ ...request, request_id: requestId });
      this.ws.send(JSON.stringify(message));

      // Timeout
      setTimeout(() => {
        if (this.pendingRequests.has(requestId)) {
          this.pendingRequests.delete(requestId);
          reject(
            new Error(`Timeout waiting for ${config.responseType} response`)
          );
        }
      }, config.timeout || 5000);
    });
  }

  /**
   * Get a window by ID (convenience method)
   */
  getWindow(windowId: string): Window | undefined {
    return this.windowsInternal.find((w) => w.id === windowId);
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
    const response = await this.rpc<GetChildren.Request, GetChildren.Response>(
      {
        requestType: "get_children",
        responseType: "get_children_response",
        buildRequest: (req) => ({ type: "get_children", ...req }),
        timeout: 10000,
      },
      {
        element_id: elementId,
        max_depth: maxDepth,
        max_children_per_level: maxChildren,
      }
    );

    return response.children || [];
  }

  /**
   * Write text to an element by element ID
   */
  async writeByElementId(elementId: string, text: string): Promise<void> {
    await this.rpc<WriteToElement.Request, WriteToElement.Response>(
      {
        requestType: "write_to_element",
        responseType: "write_to_element_response",
        buildRequest: (req) => ({ type: "write_to_element", ...req }),
      },
      { element_id: elementId, text }
    );
  }

  /**
   * Click/press an element by element ID
   */
  async clickElement(elementId: string): Promise<void> {
    await this.rpc<ClickElement.Request, ClickElement.Response>(
      {
        requestType: "click_element",
        responseType: "click_element_response",
        buildRequest: (req) => ({ type: "click_element", ...req }),
      },
      { element_id: elementId }
    );
  }

  /**
   * Get accessibility element at screen position
   * Returns an orphan element (not part of any window tree)
   */
  async getElementAtPosition(x: number, y: number): Promise<AXNode | null> {
    const response = await this.rpc<
      GetElementAtPosition.Request,
      GetElementAtPosition.Response
    >(
      {
        requestType: "get_element_at_position",
        responseType: "get_element_at_position_response",
        buildRequest: (req) => ({ type: "get_element_at_position", ...req }),
        timeout: 5000,
      },
      { x, y }
    );

    if (response.element) {
      return this.attachNodeMethods(response.element);
    }
    return null;
  }

  /**
   * Set clickthrough state (window transparency to mouse events)
   */
  async setClickthrough(enabled: boolean): Promise<void> {
    await this.rpc<SetClickthrough.Request, SetClickthrough.Response>(
      {
        requestType: "set_clickthrough",
        responseType: "set_clickthrough_response",
        buildRequest: (req) => ({ type: "set_clickthrough", ...req }),
        timeout: 2000,
      },
      { enabled }
    );
  }

  /**
   * Watch a node for changes by element ID
   * When the node changes, `onElementUpdate` callbacks will fire
   */
  async watchNodeByElementId(elementId: string, nodeId: string): Promise<void> {
    await this.rpc<WatchNode.Request, WatchNode.Response>(
      {
        requestType: "watch_node",
        responseType: "watch_node_response",
        buildRequest: (req) => ({ type: "watch_node", ...req }),
      },
      { element_id: elementId, node_id: nodeId }
    );
  }

  /**
   * Stop watching a node by element ID
   */
  async unwatchNodeByElementId(elementId: string): Promise<void> {
    await this.rpc<UnwatchNode.Request, UnwatchNode.Response>(
      {
        requestType: "unwatch_node",
        responseType: "unwatch_node_response",
        buildRequest: (req) => ({ type: "unwatch_node", ...req }),
      },
      { element_id: elementId }
    );
  }

  /**
   * Register callback for element updates (pushed from backend via AXObserver)
   * Called when a watched element's value, label, or other properties change
   * Also updates window roots to keep them fresh
   */
  onElementUpdate(callback: (update: ElementUpdate) => void): void {
    this.registerListener<ElementUpdate>("element_update", callback, (data) => {
      if (!data.update) return null;

      const update = data.update as ElementUpdate;

      // Apply update to window roots
      this.applyTreeUpdate(update);

      return update;
    });
  }

  /**
   * Apply element update to window roots
   * Keeps trees fresh without refetching
   */
  private applyTreeUpdate(update: ElementUpdate): void {
    // Search all window roots for the element
    for (const window of this.windowsInternal) {
      if (window.root) {
        const node = this.findNodeInTree(window.root, update.element_id);
        if (node) {
          // Found the node - apply update
          switch (update.update_type) {
            case "ValueChanged":
              (node as any).value = update.value;
              break;
            case "LabelChanged":
              (node as any).label = update.label;
              break;
            case "ElementDestroyed":
              // Note: For destruction, we'd need to remove from parent's children
              // This is complex, so for now we just mark it
              // Frontend can handle this by removing from DOM
              console.log(`⚠️ Element ${update.element_id} destroyed in tree`);
              break;
          }
          break; // Found and updated, stop searching
        }
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

  // ============================================================================
  // Private Methods
  // ============================================================================

  private handleMessage(data: string): void {
    try {
      const message: ServerMessage = JSON.parse(data);

      // Type-safe message handling with discriminated union
      switch (message.type) {
        case "window_update":
          // Handled via onWindowUpdate listeners
          const windowListeners = this.listeners.get("window_update");
          if (windowListeners) {
            windowListeners.forEach((callback) => callback(message));
          }
          break;

        case "window_root_update":
          // Attach methods to the root node
          const rootWithMethods = this.attachNodeMethods(message.root);

          // Store root on the window
          const window = this.windowsInternal.find(
            (w) => w.id === message.window_id
          );
          if (window) {
            (window as any).root = rootWithMethods;
            console.log(
              `[AXIO] 📦 Attached root node to window ${message.window_id}`
            );
          } else {
            console.warn(
              `[AXIO] ⚠️ Received root for unknown window ${message.window_id}`
            );
          }

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
          // Apply update to tree
          this.applyTreeUpdate(message.update);
          break;

        // Response messages - handle via request_id correlation
        case "get_children_response":
        case "write_to_element_response":
        case "click_element_response":
        case "set_clickthrough_response":
        case "watch_node_response":
        case "unwatch_node_response":
        case "get_element_at_position_response": {
          // Try request_id correlation first (new pattern)
          const requestId = (message as any).request_id;
          if (requestId && this.pendingRequests.has(requestId)) {
            const pending = this.pendingRequests.get(requestId)!;
            this.pendingRequests.delete(requestId);
            pending.resolve(message);
          }
          break;
        }

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
