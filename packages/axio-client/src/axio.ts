/**
 * AXIO - Accessibility I/O Layer (TypeScript Client)
 *
 * Uses flat element storage - elements stored by ID in a map,
 * relationships tracked via parent_id/children_ids.
 */

import EventEmitter from "eventemitter3";
import { RpcClient } from "./rpc";
import type {
  AXElement,
  ServerEvent,
  AXWindow,
  RpcRequest,
  ElementId,
} from "./types";

// Extract args type for a specific RPC method (type-safe against Rust)
type RpcArgs<M extends RpcRequest["method"]> = Extract<
  RpcRequest,
  { method: M }
>["args"];

// Derive backend event types from generated ServerEvent
type EventData<E extends ServerEvent["event"]> = Extract<
  ServerEvent,
  { event: E }
>["data"];
type BackendEvents = { [E in ServerEvent["event"]]: EventData<E> };

// Client-facing events
interface AxioEvents {
  windows: [AXWindow[]];
  focus: [AXWindow | null];
  mouse: [{ x: number; y: number }];
  elements: [AXElement[]];
  destroyed: [string]; // element_id
}

export class AXIO extends EventEmitter<AxioEvents> {
  private rpc: RpcClient<BackendEvents>;

  /** Flat element registry - all elements by ID */
  readonly elements = new Map<string, AXElement>();

  /** Current windows */
  windows: AXWindow[] = [];

  /** Currently focused window (preserved when focus goes to overlay/desktop) */
  focused: AXWindow | null = null;

  constructor(url = "ws://localhost:3030/ws") {
    super();
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

  /** Get children of an element (from cache). Returns empty if not discovered. */
  getChildren(element: AXElement): AXElement[] {
    if (element.children_ids === null) return []; // Not discovered yet
    return element.children_ids
      .map((id) => this.elements.get(id))
      .filter((e): e is AXElement => e !== undefined);
  }

  /** Get parent of an element (from cache) */
  getParent(element: AXElement): AXElement | undefined {
    return element.parent_id ? this.elements.get(element.parent_id) : undefined;
  }

  // === RPC Methods (typed against Rust RpcRequest) ===

  /** Get the deepest element at screen coordinates */
  async elementAt(x: number, y: number): Promise<AXElement> {
    const args: RpcArgs<"element_at"> = { x, y };
    const element = await this.rpc.call<AXElement>("element_at", args);
    return this.register(element);
  }

  /** Get cached element by ID */
  async getFromServer(elementId: ElementId): Promise<AXElement> {
    const args: RpcArgs<"get"> = { element_id: elementId };
    const element = await this.rpc.call<AXElement>("get", args);
    return this.register(element);
  }

  /** Discover children of an element (fetches from macOS, caches locally) */
  async children(
    elementId: ElementId,
    maxChildren = 2000
  ): Promise<AXElement[]> {
    const args: RpcArgs<"children"> = {
      element_id: elementId,
      max_children: maxChildren,
    };
    const elements = await this.rpc.call<AXElement[]>("children", args);
    return elements.map((e) => this.register(e));
  }

  /** Refresh an element's attributes from macOS */
  async refresh(elementId: ElementId): Promise<AXElement> {
    const args: RpcArgs<"refresh"> = { element_id: elementId };
    const element = await this.rpc.call<AXElement>("refresh", args);
    return this.register(element);
  }

  /** Write text to element */
  async write(elementId: ElementId, text: string): Promise<void> {
    const args: RpcArgs<"write"> = { element_id: elementId, text };
    await this.rpc.call("write", args);
  }

  /** Click element */
  async click(elementId: ElementId): Promise<void> {
    const args: RpcArgs<"click"> = { element_id: elementId };
    await this.rpc.call("click", args);
  }

  /** Watch element for changes */
  async watch(elementId: ElementId): Promise<void> {
    const args: RpcArgs<"watch"> = { element_id: elementId };
    await this.rpc.call("watch", args);
  }

  /** Stop watching element */
  async unwatch(elementId: ElementId): Promise<void> {
    const args: RpcArgs<"unwatch"> = { element_id: elementId };
    await this.rpc.call("unwatch", args);
  }

  /** Set clickthrough (app-specific, not in core RPC) */
  async setClickthrough(enabled: boolean): Promise<void> {
    await this.rpc.call("set_clickthrough", { enabled });
  }

  // === Internal ===

  private setup(): void {
    this.rpc.on("window_update", (windows) => {
      // Find closed windows and clear their elements from cache
      const newWindowIds = new Set(windows.map((w) => w.id));
      for (const [elementId, element] of this.elements) {
        if (!newWindowIds.has(element.window_id)) {
          this.elements.delete(elementId);
        }
      }

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
          this.focused = stillExists;
        } else {
          this.focused = null;
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
