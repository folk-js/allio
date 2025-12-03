/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 */

import { RpcClient } from "./rpc";
import type { AXNode, ElementUpdate, ServerEvent, AXWindow } from "./types";

// Derive backend event types from generated ServerEvent
type EventData<E extends ServerEvent["event"]> = Extract<
  ServerEvent,
  { event: E }
>["data"];
type BackendEvents = { [E in ServerEvent["event"]]: EventData<E> };

// Client-facing events (cleaner names)
interface AxioEvents {
  windows: AXWindow[];
  focus: AXWindow | null;
  mouse: { x: number; y: number };
  update: ElementUpdate;
}

type Handler<T> = (data: T) => void;

export class AXIO {
  private rpc: RpcClient<BackendEvents>;
  private handlers = new Map<string, Set<Handler<unknown>>>();

  /** Node registry for reference equality */
  readonly nodes = new Map<string, AXNode>();

  /** Current windows */
  windows: AXWindow[] = [];

  /** Currently focused window (preserved when focus goes to overlay/desktop) */
  focused: AXWindow | null = null;

  constructor(url = "ws://localhost:3030/ws") {
    this.rpc = new RpcClient<BackendEvents>({ url });
    this.setup();
  }

  async connect(): Promise<void> {
    await this.rpc.connect();
    console.log("[AXIO] Connected");
  }

  disconnect(): void {
    this.rpc.disconnect();
  }

  get connected(): boolean {
    return this.rpc.connected;
  }

  // RPC
  async elementAt(x: number, y: number): Promise<AXNode> {
    return this.register(await this.rpc.call<AXNode>("element_at", { x, y }));
  }

  async tree(
    elementId: string,
    maxDepth = 50,
    maxChildren = 2000
  ): Promise<AXNode[]> {
    const nodes = await this.rpc.call<AXNode[]>("tree", {
      element_id: elementId,
      max_depth: maxDepth,
      max_children_per_level: maxChildren,
    });
    return nodes.map((n) => this.register(n));
  }

  async write(elementId: string, text: string): Promise<void> {
    await this.rpc.call("write", { element_id: elementId, text });
  }

  async click(elementId: string): Promise<void> {
    await this.rpc.call("click", { element_id: elementId });
  }

  async watch(elementId: string): Promise<void> {
    await this.rpc.call("watch", { element_id: elementId });
  }

  async unwatch(elementId: string): Promise<void> {
    await this.rpc.call("unwatch", { element_id: elementId });
  }

  async setClickthrough(enabled: boolean): Promise<void> {
    await this.rpc.call("set_clickthrough", { enabled });
  }

  // Events
  on<K extends keyof AxioEvents>(
    event: K,
    handler: Handler<AxioEvents[K]>
  ): () => void {
    if (!this.handlers.has(event)) this.handlers.set(event, new Set());
    const set = this.handlers.get(event)!;
    set.add(handler as Handler<unknown>);
    return () => set.delete(handler as Handler<unknown>);
  }

  private emit<K extends keyof AxioEvents>(
    event: K,
    data: AxioEvents[K]
  ): void {
    this.handlers.get(event)?.forEach((h) => h(data));
  }

  // Internal
  private setup(): void {
    this.rpc.on("window_update", (incoming) => {
      // Preserve roots
      const roots = new Map(
        this.windows.filter((w) => w.root).map((w) => [w.id, w.root!])
      );
      this.windows = incoming.map((w) => ({
        ...w,
        root: roots.get(w.id) ?? null,
      }));

      // Update focused (preserve last focused when overlay/desktop gets focus)
      const newFocused = this.windows.find((w) => w.focused);
      const prevFocusedId = this.focused?.id;

      if (newFocused) {
        this.focused = newFocused;
      } else if (this.focused) {
        // Keep reference fresh
        this.focused =
          this.windows.find((w) => w.id === this.focused!.id) ?? this.focused;
      }

      this.emit("windows", this.windows);
      if (this.focused?.id !== prevFocusedId) {
        this.emit("focus", this.focused);
      }
    });

    this.rpc.on("window_root", ({ window_id, root }) => {
      const window = this.windows.find((w) => w.id === window_id);
      if (window) {
        window.root = this.register(root);
        console.log(`[AXIO] 📦 Root for window ${window_id}`);
        this.emit("windows", this.windows);
      }
    });

    this.rpc.on("mouse_position", (pos) => {
      this.emit("mouse", pos);
    });

    this.rpc.on("element_update", (update) => {
      const id = "element_id" in update ? update.element_id : undefined;
      const node = id ? this.nodes.get(id) : undefined;
      if (node) {
        if ("value" in update)
          (node as { value?: unknown }).value = update.value;
        if ("label" in update)
          (node as { label?: string }).label = update.label;
      }
      this.emit("update", update);
    });
  }

  private register(node: AXNode): AXNode {
    const existing = this.nodes.get(node.id);
    if (existing) {
      Object.assign(existing, node);
      if (node.children)
        existing.children = node.children.map((c) => this.register(c));
      return existing;
    }
    this.nodes.set(node.id, node);
    if (node.children)
      node.children = node.children.map((c) => this.register(c));
    return node;
  }
}
