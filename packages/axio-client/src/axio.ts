/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 *
 * Mirrors Rust Registry via WebSocket events.
 * Elements are primary, trees are views.
 */

import EventEmitter from "eventemitter3";
import type {
  RpcRequest,
  ServerEvent,
  AXElement,
  AXWindow,
  ElementId,
  WindowId,
} from "./types";

// === Type helpers ===
type RpcMethod = RpcRequest["method"];
type RpcArgs<M extends RpcMethod> = Extract<RpcRequest, { method: M }>["args"];

// Manual return type mapping (matches Rust dispatch)
type RpcReturns = {
  element_at: AXElement;
  get: AXElement;
  children: AXElement[];
  refresh: AXElement;
  write: boolean;
  click: boolean;
  watch: void;
  unwatch: void;
};

// Event types derived from ServerEvent
type EventName = ServerEvent["event"];
type EventData<E extends EventName> = Extract<
  ServerEvent,
  { event: E }
>["data"];
type AxioEvents = { [E in EventName]: [EventData<E>] };

type Pending = {
  resolve: (r: unknown) => void;
  reject: (e: Error) => void;
  timer: number;
};

type WatchCallback = (element: AXElement) => void;

export class AXIO extends EventEmitter<AxioEvents> {
  private ws: WebSocket | null = null;
  private requestId = 0;
  private pending = new Map<number, Pending>();
  private watchCallbacks = new Map<string, Set<WatchCallback>>();

  // === State (mirrors Registry) ===
  readonly windows = new Map<string, AXWindow>();
  readonly elements = new Map<string, AXElement>();
  readonly watched = new Set<string>();
  activeWindow: WindowId | null = null;
  focusedWindow: WindowId | null = null;
  clickthrough = false;

  // === Options ===
  debug: boolean;

  constructor(
    private url = "ws://localhost:3030/ws",
    private timeout = 5000,
    options: { debug?: boolean } = {}
  ) {
    super();
    this.debug = options.debug ?? true; // Enabled by default for now
  }

  private log(...args: unknown[]) {
    if (this.debug) console.log("[axio]", ...args);
  }

  private logError(...args: unknown[]) {
    if (this.debug) console.error("[axio]", ...args);
  }

  // === Connection ===
  connect(): Promise<void> {
    this.log("connecting to", this.url);
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => {
        this.log("connected ✓");
        resolve();
      };
      this.ws.onerror = (e) => {
        this.logError("connection error", e);
        reject(e);
      };
      this.ws.onmessage = (e) => this.onMessage(e.data);
      this.ws.onclose = () => {
        this.log("disconnected, reconnecting...");
        this.scheduleReconnect();
      };
    });
  }

  disconnect(): void {
    this.log("disconnecting");
    this.ws?.close();
    this.ws = null;
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  // === State access (derived queries, local only) ===

  /** Get element by ID from local cache */
  get(id: string): AXElement | undefined {
    return this.elements.get(id);
  }

  /** Get the active window (last valid focused window) */
  get active(): AXWindow | null {
    return this.activeWindow
      ? this.windows.get(this.activeWindow) ?? null
      : null;
  }

  /** Get the currently focused window (null if desktop) */
  get focused(): AXWindow | null {
    return this.focusedWindow
      ? this.windows.get(this.focusedWindow) ?? null
      : null;
  }

  /** Get all elements for a window */
  getWindowElements(windowId: WindowId): AXElement[] {
    return Array.from(this.elements.values()).filter(
      (el) => el.window_id === windowId
    );
  }

  /** Get root elements for a window (parent_id is null) */
  getRootElements(windowId: WindowId): AXElement[] {
    return Array.from(this.elements.values()).filter(
      (el) => el.window_id === windowId && el.parent_id === null
    );
  }

  /** Get children of an element from local cache */
  getChildren(parent: { children: ElementId[] | null }): AXElement[] {
    return (parent.children ?? [])
      .map((id) => this.elements.get(id))
      .filter((e): e is AXElement => !!e);
  }

  // === RPC Methods (questions + actions) ===

  /** Get element at screen coordinates (fetches from OS) */
  elementAt = (x: number, y: number) => this.call("element_at", { x, y });

  /** Get element by ID (from registry, fetches if needed) */
  getElement = (elementId: ElementId) =>
    this.call("get", { element_id: elementId });

  /** Get children of element (fetches from OS) */
  children = (elementId: ElementId, maxChildren = 1000) =>
    this.call("children", { element_id: elementId, max_children: maxChildren });

  /** Force re-fetch element from OS */
  refresh = (elementId: ElementId) =>
    this.call("refresh", { element_id: elementId });

  /** Write text to element */
  write = (elementId: ElementId, text: string) =>
    this.call("write", { element_id: elementId, text });

  /** Click element */
  click = (elementId: ElementId) =>
    this.call("click", { element_id: elementId });

  /**
   * Watch an element for changes.
   * Returns a cleanup function.
   * Optionally pass a callback to be called when the element changes.
   */
  watch(elementId: ElementId, callback?: WatchCallback): () => void {
    if (callback) {
      if (!this.watchCallbacks.has(elementId)) {
        this.watchCallbacks.set(elementId, new Set());
      }
      this.watchCallbacks.get(elementId)!.add(callback);
    }

    const isFirst = !this.watched.has(elementId);
    if (isFirst) {
      this.watched.add(elementId);
      this.rawCall("watch", { element_id: elementId }).catch(() => {});
    }

    return () => {
      if (callback && this.watchCallbacks.has(elementId)) {
        this.watchCallbacks.get(elementId)!.delete(callback);
      }
      const hasCallbacks =
        this.watchCallbacks.has(elementId) &&
        this.watchCallbacks.get(elementId)!.size > 0;
      if (!hasCallbacks) {
        this.watchCallbacks.delete(elementId);
        this.watched.delete(elementId);
        this.rawCall("unwatch", { element_id: elementId }).catch(() => {});
      }
    };
  }

  /** Unwatch an element */
  unwatch(elementId: ElementId): Promise<void> {
    this.watched.delete(elementId);
    this.watchCallbacks.delete(elementId);
    return this.rawCall("unwatch", { element_id: elementId }) as Promise<void>;
  }

  /** Set clickthrough mode (for overlay apps) */
  async setClickthrough(enabled: boolean): Promise<void> {
    // Always tell server - state may have changed via global shortcut
    this.clickthrough = enabled;
    this.log("clickthrough", enabled ? "enabled" : "disabled");
    await this.rawCall("set_clickthrough", { enabled });
  }

  // === Raw RPC ===
  rawCall(
    method: string,
    args: Record<string, unknown> = {}
  ): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (!this.connected) return reject(new Error("Not connected"));
      const id = ++this.requestId;
      const timer = window.setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timeout: ${method}`));
      }, this.timeout);
      this.pending.set(id, { resolve, reject, timer });
      this.ws!.send(JSON.stringify({ id, method, args }));
    });
  }

  // === Internal ===

  private onMessage(raw: string): void {
    const msg = JSON.parse(raw);

    // RPC response - early out
    if (msg.id && this.pending.has(msg.id)) {
      const { resolve, reject, timer } = this.pending.get(msg.id)!;
      this.pending.delete(msg.id);
      clearTimeout(timer);
      msg.error ? reject(new Error(msg.error)) : resolve(msg.result);
      return;
    }

    // Event - apply to state and emit
    if (!msg.event) return;

    const event = msg as ServerEvent;
    switch (event.event) {
      case "sync:init": {
        const { windows, elements, active_window, focused_window } = event.data;
        this.windows.clear();
        this.elements.clear();
        windows.forEach((w) => this.windows.set(w.id, w));
        elements.forEach((e) => this.elements.set(e.id, e));
        this.activeWindow = active_window;
        this.focusedWindow = focused_window;
        this.log(
          `synced: ${windows.length} windows, ${elements.length} elements`
        );
        break;
      }

      case "window:added": {
        this.windows.set(event.data.window.id, event.data.window);
        break;
      }

      case "window:changed": {
        this.windows.set(event.data.window.id, event.data.window);
        break;
      }

      case "window:removed": {
        const { window } = event.data;
        this.windows.delete(window.id);
        for (const [id, el] of this.elements) {
          if (el.window_id === window.id) {
            this.elements.delete(id);
          }
        }
        break;
      }

      case "element:added": {
        this.elements.set(event.data.element.id, event.data.element);
        break;
      }

      case "element:changed": {
        const { element } = event.data;
        this.elements.set(element.id, element);
        this.watchCallbacks.get(element.id)?.forEach((cb) => cb(element));
        break;
      }

      case "element:removed": {
        const { element } = event.data;
        this.elements.delete(element.id);
        this.watchCallbacks.delete(element.id);
        break;
      }

      case "focus:changed": {
        this.focusedWindow = event.data.window_id;
        break;
      }

      case "active:changed": {
        this.activeWindow = event.data.window_id;
        break;
      }

      case "mouse:position":
        // No state update needed
        break;
    }

    // Emit for external listeners
    (this.emit as Function)(event.event, event.data);
  }

  private async call<M extends RpcMethod>(
    method: M,
    args: RpcArgs<M>
  ): Promise<RpcReturns[M]> {
    const result = await this.rawCall(method, args as Record<string, unknown>);
    return result as RpcReturns[M];
  }

  private scheduleReconnect(): void {
    setTimeout(() => this.connect().catch(() => {}), 1000);
  }
}
