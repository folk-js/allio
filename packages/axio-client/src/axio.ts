/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 *
 * Uses flat element storage - elements stored by ID in a map,
 * relationships tracked via parent_id/children_ids.
 */

import { RpcClient } from "./rpc";
import type { AXElement, ServerEvent, AXWindow } from "./types";

// Derive backend event types from generated ServerEvent
type EventData<E extends ServerEvent["event"]> = Extract<
  ServerEvent,
  { event: E }
>["data"];
type BackendEvents = { [E in ServerEvent["event"]]: EventData<E> };

// Client-facing events
interface AxioEvents {
  windows: AXWindow[];
  focus: AXWindow | null;
  mouse: { x: number; y: number };
  elements: AXElement[];
  destroyed: string; // element_id
}

type Handler<T> = (data: T) => void;

export class AXIO {
  private rpc: RpcClient<BackendEvents>;
  private handlers = new Map<string, Set<Handler<unknown>>>();

  /** Flat element registry - all elements by ID */
  readonly elements = new Map<string, AXElement>();

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

  // === Element Access ===

  /** Get element by ID from local cache */
  get(elementId: string): AXElement | undefined {
    return this.elements.get(elementId);
  }

  /** Get root element for a window */
  getRoot(window: AXWindow): AXElement | undefined {
    return window.root_element_id
      ? this.elements.get(window.root_element_id)
      : undefined;
  }

  /** Get children of an element (from cache) */
  getChildren(element: AXElement): AXElement[] {
    if (!element.children_ids) return [];
    return element.children_ids
      .map((id) => this.elements.get(id))
      .filter((e): e is AXElement => e !== undefined);
  }

  /** Get parent of an element (from cache) */
  getParent(element: AXElement): AXElement | undefined {
    return element.parent_id ? this.elements.get(element.parent_id) : undefined;
  }

  /** Check if children have been discovered for an element */
  hasDiscoveredChildren(element: AXElement): boolean {
    // children_ids is undefined when not in JSON (skip_serializing_if), null won't happen
    return element.children_ids !== undefined && element.children_ids !== null;
  }

  // === RPC Methods ===

  /** Get the deepest element at screen coordinates */
  async elementAt(x: number, y: number): Promise<AXElement> {
    const element = await this.rpc.call<AXElement>("element_at", { x, y });
    return this.register(element);
  }

  /** Discover children of an element (fetches from macOS, caches locally) */
  async children(elementId: string, maxChildren = 2000): Promise<AXElement[]> {
    const elements = await this.rpc.call<AXElement[]>("children", {
      element_id: elementId,
      max_children: maxChildren,
    });
    return elements.map((e) => this.register(e));
  }

  /** Get tree (recursive children discovery) */
  async tree(
    elementId: string,
    maxDepth = 50,
    maxChildren = 2000
  ): Promise<AXElement[]> {
    const elements = await this.rpc.call<AXElement[]>("tree", {
      element_id: elementId,
      max_depth: maxDepth,
      max_children_per_level: maxChildren,
    });
    return elements.map((e) => this.register(e));
  }

  /** Refresh an element's attributes from macOS */
  async refresh(elementId: string): Promise<AXElement> {
    const element = await this.rpc.call<AXElement>("refresh", {
      element_id: elementId,
    });
    return this.register(element);
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

  // === Events ===

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

  // === Internal ===

  private setup(): void {
    this.rpc.on("window_update", (windows) => {
      this.windows = windows;

      // Update focused: prefer currently focused, fall back to last focused if still exists
      const newFocused = this.windows.find((w) => w.focused);
      const prevFocusedId = this.focused?.id;

      if (newFocused) {
        this.focused = newFocused;
      } else if (this.focused) {
        // When no window is focused (desktop/overlay), preserve last focused if it still exists
        const stillExists = this.windows.find((w) => w.id === this.focused!.id);
        if (stillExists) {
          this.focused = stillExists; // Keep reference fresh with new data
        } else {
          this.focused = null; // Window was closed
        }
      }

      this.emit("windows", this.windows);
      if (this.focused?.id !== prevFocusedId) {
        this.emit("focus", this.focused);
      }
    });

    this.rpc.on("elements", (elements) => {
      const registered = elements.map((e) => this.register(e));
      this.emit("elements", registered);
    });

    this.rpc.on("element_destroyed", ({ element_id }) => {
      this.elements.delete(element_id);
      this.emit("destroyed", element_id);
    });

    this.rpc.on("mouse_position", (pos) => {
      this.emit("mouse", pos);
    });
  }

  /** Register element in cache, returns same instance for reference equality */
  private register(element: AXElement): AXElement {
    const existing = this.elements.get(element.id);
    if (existing) {
      // Update existing element in place (preserves reference equality)
      Object.assign(existing, element);
      return existing;
    }
    this.elements.set(element.id, element);
    return element;
  }
}
