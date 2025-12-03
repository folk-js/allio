/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 *
 * A thin WebSocket client for the AXIO accessibility system.
 * Types are auto-generated from Rust via ts-rs.
 *
 * Protocol:
 * - Request: { id, method, args }
 * - Response: { id, result } or { id, error }
 * - Event: { event, data }
 */

// Import auto-generated types from Rust
import type {
  AXNode as GeneratedAXNode,
  ElementUpdate,
  WindowInfo,
} from "./types";

// ============================================================================
// Extended Types (client-side additions)
// ============================================================================

/**
 * Window with optional root node (populated by backend)
 */
export interface Window extends WindowInfo {
  root?: AXNode;
}

/**
 * AXNode with client-side methods attached
 */
export interface AXNode extends Omit<GeneratedAXNode, "children"> {
  children?: AXNode[];
  // Operations (set by AXIO when creating nodes)
  setValue?(text: string): Promise<void>;
  click?(): Promise<void>;
  getChildren?(maxDepth?: number, maxChildren?: number): Promise<AXNode[]>;
}

// ============================================================================
// AXIO Client Class
// ============================================================================

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

  // ============================================================================
  // Connection
  // ============================================================================

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

  // ============================================================================
  // RPC (Request/Response)
  // ============================================================================

  private generateRequestId(): string {
    return `r${++this.requestCounter}`;
  }

  /**
   * Make an RPC call to the backend
   */
  private async rpc<T>(method: string, args: object = {}): Promise<T> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error("WebSocket not connected"));
        return;
      }

      const id = this.generateRequestId();

      this.pendingRequests.set(id, {
        resolve: (response: any) => {
          if (response.error) {
            reject(new Error(response.error));
          } else {
            resolve(response.result as T);
          }
        },
        reject,
      });

      this.ws.send(JSON.stringify({ id, method, args }));

      // Timeout after 5 seconds
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Timeout waiting for ${method} response`));
        }
      }, 5000);
    });
  }

  // ============================================================================
  // API Methods
  // ============================================================================

  /**
   * Get element at screen position
   */
  async elementAt(x: number, y: number): Promise<AXNode> {
    const node = await this.rpc<GeneratedAXNode>("element_at", { x, y });
    return this.attachNodeMethods(node);
  }

  /**
   * Alias for elementAt (for backwards compatibility)
   */
  async getElementAtPosition(x: number, y: number): Promise<AXNode> {
    return this.elementAt(x, y);
  }

  /**
   * Get children of an element
   */
  async getChildren(
    elementId: string,
    maxDepth: number = 50,
    maxChildrenPerLevel: number = 2000
  ): Promise<AXNode[]> {
    const children = await this.rpc<GeneratedAXNode[]>("tree", {
      element_id: elementId,
      max_depth: maxDepth,
      max_children_per_level: maxChildrenPerLevel,
    });
    return children.map((c) => this.attachNodeMethods(c));
  }

  /**
   * Write text to an element
   */
  async write(elementId: string, text: string): Promise<void> {
    await this.rpc<null>("write", { element_id: elementId, text });
  }

  /**
   * Click an element
   */
  async click(elementId: string): Promise<void> {
    await this.rpc<null>("click", { element_id: elementId });
  }

  /**
   * Watch an element for changes
   */
  async watch(elementId: string): Promise<void> {
    await this.rpc<null>("watch", { element_id: elementId });
  }

  /**
   * Stop watching an element
   */
  async unwatch(elementId: string): Promise<void> {
    await this.rpc<null>("unwatch", { element_id: elementId });
  }

  /**
   * Set clickthrough mode (for overlay windows)
   */
  async setClickthrough(enabled: boolean): Promise<void> {
    await this.rpc<{ enabled: boolean }>("set_clickthrough", { enabled });
  }

  // Convenience methods that delegate to node operations
  async watchNodeByElementId(
    elementId: string,
    _nodeId?: string
  ): Promise<void> {
    await this.watch(elementId);
  }

  async unwatchNodeByElementId(elementId: string): Promise<void> {
    await this.unwatch(elementId);
  }

  // ============================================================================
  // Event Handling
  // ============================================================================

  get windows(): ReadonlyArray<Window> {
    return this.windowsInternal;
  }

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

  onWindowUpdate(callback: (windows: Window[]) => void): void {
    this.registerListener<Window[]>("window_update", callback, (windows) => {
      if (!Array.isArray(windows)) return null;

      // Preserve existing roots
      const existingRoots = new Map<string, AXNode>();
      for (const win of this.windowsInternal) {
        if (win.root) {
          existingRoots.set(win.id, win.root);
        }
      }

      // Merge incoming windows with existing roots
      this.windowsInternal = windows.map((win: WindowInfo) => ({
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

  onFocusedWindowChange(
    callback: (focusedWindow: Window | null) => void
  ): void {
    this.registerListener<Window | null>("focused_window_change", callback);
  }

  private notifyFocusedWindowChange(focusedWindow: Window | null): void {
    const listeners = this.listeners.get("focused_window_change");
    if (listeners) {
      listeners.forEach((callback) => callback(focusedWindow));
    }
  }

  onMousePosition(callback: (x: number, y: number) => void): void {
    this.registerListener<{ x: number; y: number }>("mouse_position", (data) =>
      callback(data.x, data.y)
    );
  }

  onElementUpdate(callback: (update: ElementUpdate) => void): void {
    this.registerListener<ElementUpdate>("element_update", callback);
  }

  // ============================================================================
  // Message Handling
  // ============================================================================

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data);

      // Handle RPC responses (has 'id' field)
      if (message.id && this.pendingRequests.has(message.id)) {
        const pending = this.pendingRequests.get(message.id)!;
        this.pendingRequests.delete(message.id);
        pending.resolve(message);
        return;
      }

      // Handle events (has 'event' field)
      if (message.event) {
        const eventData = message.data;

        switch (message.event) {
          case "window_update": {
            const listeners = this.listeners.get("window_update");
            if (listeners) {
              listeners.forEach((callback) => callback(eventData));
            }
            break;
          }

          case "window_root": {
            const { window_id, root } = eventData;
            const rootWithMethods = this.attachNodeMethods(root);

            const window = this.windowsInternal.find((w) => w.id === window_id);
            if (window) {
              (window as any).root = rootWithMethods;
              console.log(
                `[AXIO] 📦 Attached root node to window ${window_id}`
              );

              // If this is the focused window and it just got its root,
              // re-notify focus listeners so they can display the tree
              if (this.focused?.id === window_id) {
                this.notifyFocusedWindowChange(this.focused);
              }
            }
            break;
          }

          case "mouse_position": {
            const listeners = this.listeners.get("mouse_position");
            if (listeners) {
              listeners.forEach((callback) => callback(eventData));
            }
            break;
          }

          case "element_update": {
            const listeners = this.listeners.get("element_update");
            if (listeners) {
              listeners.forEach((callback) => callback(eventData));
            }
            // Apply update to tree
            this.applyTreeUpdate(eventData);
            break;
          }

          default:
            console.warn("[AXIO] Unknown event:", message.event);
        }
        return;
      }

      console.warn("[AXIO] Unhandled message:", message);
    } catch (error) {
      console.error("[AXIO] Failed to parse message:", error);
    }
  }

  // ============================================================================
  // Node Helpers
  // ============================================================================

  private attachNodeMethods(node: GeneratedAXNode): AXNode {
    const axio = this;
    const result: AXNode = {
      ...node,
      children: node.children?.map((c) => this.attachNodeMethods(c)),
    };

    // Add methods
    result.setValue = async function (text: string): Promise<void> {
      await axio.write(result.id, text);
    };

    result.click = async function (): Promise<void> {
      await axio.click(result.id);
    };

    result.getChildren = async function (
      maxDepth: number = 50,
      maxChildren: number = 2000
    ): Promise<AXNode[]> {
      return axio.getChildren(result.id, maxDepth, maxChildren);
    };

    return result;
  }

  private applyTreeUpdate(update: ElementUpdate): void {
    // Find and update the node in all window trees
    for (const window of this.windowsInternal) {
      if (window.root) {
        this.updateNodeInTree(window.root, update);
      }
    }
  }

  private updateNodeInTree(node: AXNode, update: ElementUpdate): boolean {
    const elementId = "element_id" in update ? update.element_id : undefined;

    if (node.id === elementId) {
      // Apply update based on type
      if ("value" in update) {
        (node as any).value = update.value;
      } else if ("label" in update) {
        (node as any).label = update.label;
      }
      return true;
    }

    // Recurse into children
    for (const child of node.children ?? []) {
      if (this.updateNodeInTree(child, update)) {
        return true;
      }
    }

    return false;
  }
}
