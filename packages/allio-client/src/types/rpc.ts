import type { RpcRequest, AX, TypedElement } from "./";

// === Type helpers ===
export type RpcMethod = RpcRequest["method"];
// For methods with args, extract the args type; for methods without, use empty object
export type RpcArgs<M extends RpcMethod> = Extract<
  RpcRequest,
  { method: M }
> extends {
  args: infer A;
}
  ? A
  : Record<string, never>;

// Re-export Recency from generated types
export type { Recency } from "./generated/Recency";

// Manual return type mapping (matches Rust dispatch)
export type RpcReturns = {
  snapshot: AX.Snapshot;
  element_at: TypedElement;
  get: TypedElement;
  window_root: TypedElement;
  children: TypedElement[];
  parent: TypedElement | null;
  set: boolean;
  perform: boolean;
  watch: void;
  unwatch: void;
  observe: void;
  unobserve: void;
};

// Event types derived from ServerEvent
type EventName = AX.Event["event"];
type EventData<E extends EventName> = Extract<AX.Event, { event: E }>["data"];

export type AllioEvents = { [E in EventName]: [EventData<E>] };

export type Pending = {
  resolve: (r: unknown) => void;
  reject: (e: Error) => void;
  timer: number;
};

export type WatchCallback = (element: TypedElement) => void;
