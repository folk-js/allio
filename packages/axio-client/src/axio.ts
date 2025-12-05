/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 *
 * Mirrors Rust state via WebSocket events.
 * Flat element storage - elements stored by ID in a map.
 */

import EventEmitter from "eventemitter3";
import type {
  RpcRequest,
  ServerEvent,
  AXElement,
  AXWindow,
  ElementId,
} from "./types";

// === Type helpers (derived from generated RpcRequest) ===
type RpcMethod = RpcRequest["method"];
type RpcArgs<M extends RpcMethod> = Extract<RpcRequest, { method: M }>["args"];

// Manual return type mapping (matches Rust dispatch)
type RpcReturns = {
  element_at: AXElement;
  get: AXElement;
  children: AXElement[];
  refresh: AXElement;
  write: void;
  click: void;
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

export class AXIO extends EventEmitter<AxioEvents> {
  private ws: WebSocket | null = null;
  private requestId = 0;
  private pending = new Map<string, Pending>();

  // === State (mirrors Rust) ===
  readonly windows = new Map<string, AXWindow>();
  readonly elements = new Map<string, AXElement>();
  readonly watched = new Set<string>(); // Elements we're watching
  activeWindow: string | null = null;

  constructor(private url = "ws://localhost:3030/ws", private timeout = 5000) {
    super();
  }

  // === Connection ===
  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => resolve();
      this.ws.onerror = reject;
      this.ws.onmessage = (e) => this.onMessage(e.data);
      this.ws.onclose = () => this.scheduleReconnect();
    });
  }

  disconnect(): void {
    this.ws?.close();
    this.ws = null;
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  // === State access ===
  get(id: string): AXElement | undefined {
    return this.elements.get(id);
  }

  get active(): AXWindow | null {
    return this.activeWindow
      ? this.windows.get(this.activeWindow) ?? null
      : null;
  }

  getChildren(parent: { children: ElementId[] | null }): AXElement[] {
    return (parent.children ?? [])
      .map((id) => this.elements.get(id))
      .filter((e): e is AXElement => !!e);
  }

  // === RPC Methods (typed, nice API) ===
  elementAt = (x: number, y: number) => this.call("element_at", { x, y });
  getElement = (elementId: ElementId) =>
    this.call("get", { element_id: elementId });
  children = (elementId: ElementId, maxChildren = 1000) =>
    this.call("children", { element_id: elementId, max_children: maxChildren });
  refresh = (elementId: ElementId) =>
    this.call("refresh", { element_id: elementId });
  write = (elementId: ElementId, text: string) =>
    this.call("write", { element_id: elementId, text });
  click = (elementId: ElementId) =>
    this.call("click", { element_id: elementId });
  watch = async (elementId: ElementId) => {
    await this.call("watch", { element_id: elementId });
    this.watched.add(elementId);
  };
  unwatch = async (elementId: ElementId) => {
    await this.call("unwatch", { element_id: elementId });
    this.watched.delete(elementId);
  };

  /** Custom RPC for app-specific clickthrough (not in core RpcRequest) */
  async setClickthrough(enabled: boolean): Promise<void> {
    await this.rawCall("set_clickthrough", { enabled });
  }

  // === State effects (declarative) ===
  private effects: Partial<Record<EventName, (d: unknown) => void>> = {
    "sync:snapshot": (d) => {
      const data = d as EventData<"sync:snapshot">;
      this.windows.clear();
      data.windows.forEach((w) => this.windows.set(w.id, w));
      this.activeWindow = data.active_window;
    },
    "window:opened": (d) => {
      const w = d as AXWindow;
      this.windows.set(w.id, w);
    },
    "window:closed": (d) => {
      const { window_id } = d as EventData<"window:closed">;
      this.windows.delete(window_id);
    },
    "window:updated": (d) => {
      const w = d as AXWindow;
      this.windows.set(w.id, w);
    },
    "window:active": (d) => {
      const { window_id } = d as EventData<"window:active">;
      if (window_id !== null) {
        this.activeWindow = window_id;
      }
      // When null (desktop focused), preserve activeWindow
    },
    "element:discovered": (d) => this.register(d as AXElement),
    "element:updated": (d) => {
      const { element } = d as EventData<"element:updated">;
      this.register(element);
    },
    "element:destroyed": (d) => {
      const { element_id } = d as EventData<"element:destroyed">;
      this.elements.delete(element_id);
    },
  };

  // === Raw RPC (for custom methods not in RpcRequest) ===
  rawCall(
    method: string,
    args: Record<string, unknown> = {}
  ): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (!this.connected) return reject(new Error("Not connected"));
      const id = `r${++this.requestId}`;
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
    if (msg.event) {
      this.effects[msg.event as EventName]?.(msg.data);
      (this.emit as Function)(msg.event, msg.data);
    } else if (msg.id && this.pending.has(msg.id)) {
      const { resolve, reject, timer } = this.pending.get(msg.id)!;
      this.pending.delete(msg.id);
      clearTimeout(timer);
      msg.error ? reject(new Error(msg.error)) : resolve(msg.result);
    }
  }

  private async call<M extends RpcMethod>(
    method: M,
    args: RpcArgs<M>
  ): Promise<RpcReturns[M]> {
    const result = await this.rawCall(method, args as Record<string, unknown>);
    this.registerResult(result);
    return result as RpcReturns[M];
  }

  private register(el: AXElement): AXElement {
    const existing = this.elements.get(el.id);
    if (existing) {
      Object.assign(existing, el);
      return existing;
    }
    this.elements.set(el.id, el);
    return el;
  }

  private registerResult(result: unknown): void {
    if (
      result &&
      typeof result === "object" &&
      "id" in result &&
      "role" in result
    ) {
      this.register(result as AXElement);
    } else if (Array.isArray(result)) {
      result.forEach((r) => this.registerResult(r));
    }
  }

  private scheduleReconnect(): void {
    setTimeout(() => this.connect().catch(() => {}), 1000);
  }
}
