/**
 * Typed WebSocket RPC Client
 *
 * Protocol:
 * - Request:  { id, method, args }
 * - Response: { id, result } or { id, error }
 * - Event:    { event, data }
 */

import EventEmitter from "eventemitter3";

export interface RpcClientOptions {
  url: string;
  timeout?: number;
  reconnectDelay?: number;
}

type Pending = {
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
  timer: number;
};

// Convert { event: data } map to EventEmitter3's tuple format
type ToEmitterEvents<T> = { [K in keyof T]: [T[K]] };

export class RpcClient<
  TEvents extends Record<string, unknown> = Record<string, unknown>
> extends EventEmitter<ToEmitterEvents<TEvents>> {
  private ws: WebSocket | null = null;
  private requestId = 0;
  private pending = new Map<string, Pending>();
  private reconnectTimer: number | null = null;

  readonly url: string;
  private readonly timeout: number;
  private readonly reconnectDelay: number;

  constructor(options: RpcClientOptions) {
    super();
    this.url = options.url;
    this.timeout = options.timeout ?? 5000;
    this.reconnectDelay = options.reconnectDelay ?? 1000;
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => {
        this.clearReconnect();
        resolve();
      };
      this.ws.onmessage = (e) => this.handleMessage(e.data);
      this.ws.onerror = (e) => reject(e);
      this.ws.onclose = () => this.scheduleReconnect();
    });
  }

  disconnect(): void {
    this.clearReconnect();
    this.ws?.close();
    this.ws = null;
  }

  call<T>(method: string, args: Record<string, unknown> = {}): Promise<T> {
    return new Promise((resolve, reject) => {
      if (!this.connected) {
        reject(new Error("Not connected"));
        return;
      }

      const id = `r${++this.requestId}`;
      const timer = window.setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timeout: ${method}`));
      }, this.timeout);

      this.pending.set(id, { resolve: (r) => resolve(r as T), reject, timer });
      this.ws!.send(JSON.stringify({ id, method, args }));
    });
  }

  private handleMessage(raw: string): void {
    let msg: {
      id?: string;
      result?: unknown;
      error?: string;
      event?: string;
      data?: unknown;
    };
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }

    // RPC response
    if (msg.id && this.pending.has(msg.id)) {
      const { resolve, reject, timer } = this.pending.get(msg.id)!;
      this.pending.delete(msg.id);
      clearTimeout(timer);
      msg.error ? reject(new Error(msg.error)) : resolve(msg.result);
      return;
    }

    // Server event - cast needed since event name comes from wire
    if (msg.event) {
      (this.emit as Function)(msg.event, msg.data);
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer === null) {
      this.reconnectTimer = window.setTimeout(() => {
        this.reconnectTimer = null;
        this.connect().catch(() => {});
      }, this.reconnectDelay);
    }
  }

  private clearReconnect(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}
