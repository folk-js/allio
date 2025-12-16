/**
 * Allio - Accessibility (A11y) I/O Layer (TypeScript Client)
 *
 * Mirrors Rust Registry via WebSocket events.
 * Elements are primary, trees are views.
 */

import EventEmitter from "eventemitter3";
import type {
  AX,
  AllioEvents,
  WatchCallback,
  Pending,
  RpcMethod,
  RpcArgs,
  RpcReturns,
  TypedElement,
  ElementOfRole,
  PrimitiveForRole,
  WritableRole,
  Recency,
} from "./types";
import { ROLE_VALUES } from "./types";

export class Allio extends EventEmitter<AllioEvents> {
  private ws: WebSocket | null = null;
  private requestId = 0;
  private pending = new Map<number, Pending>();
  private watchCallbacks = new Map<AX.ElementId, Set<WatchCallback>>();

  // === State (mirrors Registry) ===
  readonly windows = new Map<AX.WindowId, AX.Window>();
  readonly elements = new Map<AX.ElementId, TypedElement>();
  readonly watched = new Set<AX.ElementId>();

  /** Window IDs sorted by z-order (front to back) */
  zOrder: AX.WindowId[] = [];

  // Focus tracking
  focusedWindow: AX.WindowId | null = null;
  focusedElement: TypedElement | null = null;
  selection: AX.TextSelection | null = null;
  passthrough = false;

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
    if (this.debug) console.log("[allio]", ...args);
  }

  private logError(...args: unknown[]) {
    if (this.debug) console.error("[allio]", ...args);
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
  get(id: AX.ElementId): TypedElement | undefined {
    return this.elements.get(id);
  }

  /** Get the currently focused window (null if desktop is focused) */
  get focused(): AX.Window | null {
    return this.focusedWindow
      ? this.windows.get(this.focusedWindow) ?? null
      : null;
  }

  /** Get all elements for a window */
  getWindowElements(windowId: AX.WindowId): TypedElement[] {
    return Array.from(this.elements.values()).filter(
      (el) => el.window_id === windowId
    );
  }

  /** Get root element for a window (element with is_root === true) */
  getRootElement(windowId: AX.WindowId): TypedElement | undefined {
    return Array.from(this.elements.values()).find(
      (el) => el.window_id === windowId && el.is_root
    );
  }

  /** Get children of an element from local cache */
  getChildren(parent: { children: AX.ElementId[] | null }): TypedElement[] {
    return (parent.children ?? [])
      .map((id) => this.elements.get(id))
      .filter((e): e is TypedElement => !!e);
  }

  // === RPC Methods (questions + actions) ===

  /**
   * Request a full state snapshot from the server.
   * Use this to re-sync state if you suspect the client is out of sync.
   * Automatically updates local state (windows, elements, etc.).
   */
  async snapshot(): Promise<AX.Snapshot> {
    const snap = await this.call("snapshot", {});
    // Apply snapshot to local state
    this.windows.clear();
    this.elements.clear();
    snap.windows.forEach((w) => this.windows.set(w.id, w));
    snap.elements.forEach((e) => this.elements.set(e.id, e as TypedElement));
    this.focusedWindow = snap.focused_window;
    this.focusedElement = snap.focused_element as TypedElement | null;
    this.selection = snap.selection;
    this.zOrder = snap.z_order;
    return snap;
  }

  /** Get element at screen coordinates (fetches from OS).
   * Returns null if no tracked window exists at the position. */
  elementAt = (x: number, y: number): Promise<TypedElement | null> =>
    this.call("element_at", { x, y });

  /**
   * Get element by ID with optional recency control.
   * @param recency - "any" (cache), "current" (fetch from OS), or { max_age_ms: number }
   */
  getElement = (element_id: AX.ElementId, recency: Recency | null = null) =>
    this.call("get", { element_id, recency });

  /** Get root element for a window (fetches from OS if not cached) */
  windowRoot = (window_id: AX.WindowId) =>
    this.call("window_root", { window_id });

  /** Get children of element (fetches from OS) */
  children = (element_id: AX.ElementId, max_children = 1000) =>
    this.call("children", { element_id, max_children });

  /** Get parent of element (fetches from OS, null if element is root) */
  parent = (element_id: AX.ElementId): Promise<TypedElement | null> =>
    this.call("parent", { element_id });

  /**
   * Set element value with type-safe primitive.
   * TypeScript enforces correct value type based on element's role.
   *
   * @example
   * await allio.set(slider, 50);      // number for slider
   * await allio.set(textfield, "hi"); // string for textfield
   * await allio.set(checkbox, true);  // boolean for checkbox
   */
  set<R extends WritableRole>(
    element: ElementOfRole<R>,
    value: PrimitiveForRole<R>
  ): Promise<boolean> {
    const valueType = ROLE_VALUES[element.role];
    if (!valueType) {
      throw new Error(`Role ${element.role} does not accept values`);
    }
    return this.call("set", {
      element_id: element.id,
      value: value as AX.Value,
    });
  }

  /**
   * Perform an action on an element.
   *
   * Actions are platform-agnostic operations like press, showmenu, increment, etc.
   *
   * @example
   * await allio.perform(buttonId, 'press');
   * await allio.perform(sliderId, 'increment');
   * await allio.perform(menuId, 'showmenu');
   */
  perform = (element_id: AX.ElementId, action: AX.Action) =>
    this.call("perform", { element_id, action });

  /**
   * Watch an element for changes.
   * Returns a cleanup function.
   * Optionally pass a callback to be called when the element changes.
   */
  watch(element_id: AX.ElementId, callback?: WatchCallback): () => void {
    if (callback) {
      if (!this.watchCallbacks.has(element_id)) {
        this.watchCallbacks.set(element_id, new Set());
      }
      this.watchCallbacks.get(element_id)!.add(callback);
    }

    const isFirst = !this.watched.has(element_id);
    if (isFirst) {
      this.watched.add(element_id);
      this.rawCall("watch", { element_id }).catch(() => {});
    }

    return () => {
      if (callback && this.watchCallbacks.has(element_id)) {
        this.watchCallbacks.get(element_id)!.delete(callback);
      }
      const hasCallbacks =
        this.watchCallbacks.has(element_id) &&
        this.watchCallbacks.get(element_id)!.size > 0;
      if (!hasCallbacks) {
        this.watchCallbacks.delete(element_id);
        this.watched.delete(element_id);
        this.rawCall("unwatch", { element_id }).catch(() => {});
      }
    };
  }

  /** Unwatch an element */
  unwatch(element_id: AX.ElementId): Promise<void> {
    this.watched.delete(element_id);
    this.watchCallbacks.delete(element_id);
    return this.rawCall("unwatch", { element_id }) as Promise<void>;
  }

  // === Observation ===

  /**
   * Observe a subtree for changes.
   *
   * The subtree will be polled periodically and changes will fire:
   * - Individual element:added/changed/removed events
   * - One subtree:changed event per polling cycle (if anything changed)
   *
   * @param element_id - Root of subtree to observe
   * @param options.depth - Maximum depth to traverse (undefined = infinite)
   * @param options.wait_between_ms - Wait time between sweeps in ms (default: 100)
   */
  async observe(
    element_id: AX.ElementId,
    options: { depth?: number; wait_between_ms?: number } = {}
  ): Promise<void> {
    await this.rawCall("observe", {
      element_id,
      depth: options.depth,
      wait_between_ms: options.wait_between_ms,
    });
  }

  /**
   * Stop observing a subtree.
   */
  async unobserve(element_id: AX.ElementId): Promise<void> {
    await this.rawCall("unobserve", { element_id });
  }

  /**
   * Set passthrough mode (for overlay apps).
   * When enabled (true), clicks pass through to underlying apps.
   * When disabled (false), the overlay captures clicks AND becomes key window for pointer events.
   */
  async setPassthrough(enabled: boolean): Promise<void> {
    await this.rawCall("set_passthrough", { enabled });
    this.passthrough = enabled;
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

    if (msg.event !== "mouse:position") {
      this.log(msg.event, msg.data);
    }

    const event = msg as AX.Event;

    switch (event.event) {
      case "sync:init": {
        const {
          windows,
          elements,
          focused_window,
          focused_element,
          selection,
          z_order,
        } = event.data;
        this.windows.clear();
        this.elements.clear();
        windows.forEach((w) => this.windows.set(w.id, w));
        elements.forEach((e) => this.elements.set(e.id, e as TypedElement));
        this.focusedWindow = focused_window;
        this.focusedElement = focused_element as TypedElement | null;
        this.selection = selection;
        this.zOrder = z_order;
        break;
      }

      case "window:added": {
        const { window } = event.data;
        this.windows.set(window.id, window);
        this.updateZOrder();
        break;
      }

      case "window:changed": {
        const { window } = event.data;
        this.windows.set(window.id, window);
        this.updateZOrder();
        break;
      }

      case "window:removed": {
        const { window_id } = event.data;
        this.windows.delete(window_id);
        for (const [id, el] of this.elements) {
          if (el.window_id === window_id) {
            this.elements.delete(id);
          }
        }
        this.updateZOrder();
        break;
      }

      case "element:added": {
        this.elements.set(
          event.data.element.id,
          event.data.element as TypedElement
        );
        break;
      }

      case "element:changed": {
        const { element } = event.data;
        this.elements.set(element.id, element as TypedElement);
        this.watchCallbacks
          .get(element.id)
          ?.forEach((cb) => cb(element as TypedElement));
        break;
      }

      case "element:removed": {
        const { element_id } = event.data;
        this.elements.delete(element_id);
        this.watchCallbacks.delete(element_id);
        break;
      }

      case "focus:window": {
        this.focusedWindow = event.data.window_id;
        break;
      }

      case "focus:element": {
        const { element } = event.data;
        this.focusedElement = element as TypedElement;
        this.elements.set(element.id, element as TypedElement);
        break;
      }

      case "selection:changed": {
        const { element_id, text, range } = event.data;
        // Clear selection if text is empty
        this.selection = text ? { text, element_id, range } : null;
        break;
      }

      case "mouse:position":
        // No state update needed
        break;

      case "subtree:changed":
        // No state update needed - elements already updated by individual events
        break;
    }

    // Emit specific event for external listeners
    (this.emit as Function)(event.event, event.data);
  }

  private updateZOrder() {
    this.zOrder = Array.from(this.windows.values())
      .sort((a, b) => a.z_index - b.z_index)
      .map((w) => w.id);
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
