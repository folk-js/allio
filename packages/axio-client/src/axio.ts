/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 */

import { RpcClient } from "./rpc";
import type { AXNode, ElementUpdate, WindowInfo } from "./types";

export interface Window extends WindowInfo {
  root?: AXNode;
}

interface AxioEvents {
  windows: Window[];
  focus: Window | null;
  root: { windowId: string; root: AXNode };
  mouse: { x: number; y: number };
  update: ElementUpdate;
}

type Handler<T> = (data: T) => void;

export class AXIO {
  private rpc: RpcClient;
  private handlers = new Map<string, Set<Handler<unknown>>>();

  /** Node registry for reference equality */
  readonly nodes = new Map<string, AXNode>();

  /** Current windows */
  windows: Window[] = [];

  /** Focused window (computed from windows) */
  get focused(): Window | null {
    return this.windows.find((w) => w.focused) ?? null;
  }

  constructor(url = "ws://localhost:3030/ws") {
    this.rpc = new RpcClient({ url });
    this.setup();
  }

  // ============================================================================
  // Connection
  // ============================================================================

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

  // ============================================================================
  // RPC
  // ============================================================================

  async elementAt(x: number, y: number): Promise<AXNode> {
    const node = await this.rpc.call<AXNode>("element_at", { x, y });
    return this.register(node);
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

  // ============================================================================
  // Events
  // ============================================================================

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

  // ============================================================================
  // Internal
  // ============================================================================

  private setup(): void {
    this.rpc.on("window_update", (data) => {
      const incoming = data as WindowInfo[];
      if (!Array.isArray(incoming)) return;

      // Preserve roots
      const roots = new Map(
        this.windows.filter((w) => w.root).map((w) => [w.id, w.root!])
      );
      const prevFocusedId = this.focused?.id;

      this.windows = incoming.map((w) => ({ ...w, root: roots.get(w.id) }));
      this.emit("windows", this.windows);

      if (this.focused?.id !== prevFocusedId) {
        this.emit("focus", this.focused);
      }
    });

    this.rpc.on("window_root", (data) => {
      const { window_id, root } = data as { window_id: string; root: AXNode };
      const registered = this.register(root);
      const window = this.windows.find((w) => w.id === window_id);

      if (window) {
        const hadRoot = !!window.root;
        window.root = registered;
        console.log(`[AXIO] 📦 Root for window ${window_id}`);
        this.emit("root", { windowId: window_id, root: registered });

        // Re-emit focus if focused window just got its root
        if (this.focused?.id === window_id && !hadRoot) {
          this.emit("focus", this.focused);
        }
      }
    });

    this.rpc.on("mouse_position", (data) => {
      this.emit("mouse", data as { x: number; y: number });
    });

    this.rpc.on("element_update", (data) => {
      const update = data as ElementUpdate;
      const id = "element_id" in update ? update.element_id : undefined;
      const node = id ? this.nodes.get(id) : undefined;

      if (node) {
        if ("value" in update) (node as any).value = update.value;
        if ("label" in update) (node as any).label = update.label;
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
