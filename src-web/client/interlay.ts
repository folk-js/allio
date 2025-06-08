interface WindowCoordinates {
  bottom_left_x: number;
  bottom_left_y: number;
  window_width: number;
}

interface ServerMessage {
  type: string;
  success?: boolean;
  message?: string;
  window_id?: string;
  windows?: any[];
  [key: string]: any;
}

export class WebSocketClient {
  private ws: WebSocket | null = null;
  private reconnectTimeout: number | null = null;
  private reconnectDelay = 3000;

  // Event callbacks
  public onConnectionChange?: (
    status: "connecting" | "connected" | "disconnected",
    message: string
  ) => void;
  public onMessage?: (data: ServerMessage) => void;
  public onError?: (error: Event) => void;

  constructor(private url: string = "ws://127.0.0.1:3030/ws") {}

  async connect(): Promise<void> {
    try {
      this.onConnectionChange?.("connecting", "Connecting to overlay...");

      this.ws = new WebSocket(this.url);

      this.ws.onopen = () => {
        this.onConnectionChange?.("connected", "✅ Connected to overlay app");
        console.log("🔗 Connected to overlay WebSocket");
      };

      this.ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          this.onMessage?.(data);
        } catch (e) {
          console.log("📨 Received non-JSON:", event.data);
        }
      };

      this.ws.onclose = () => {
        this.onConnectionChange?.(
          "disconnected",
          "❌ Disconnected from overlay"
        );
        console.log("🔌 Disconnected from overlay WebSocket");
        this.scheduleReconnect();
      };

      this.ws.onerror = (error) => {
        this.onConnectionChange?.("disconnected", "❌ Connection error");
        console.error("❌ WebSocket error:", error);
        this.onError?.(error);
      };
    } catch (error) {
      this.onConnectionChange?.("disconnected", "❌ Failed to connect");
      console.error("Failed to connect:", error);
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimeout) return;

    this.reconnectTimeout = window.setTimeout(() => {
      if (this.ws?.readyState === WebSocket.CLOSED) {
        console.log("🔄 Attempting to reconnect...");
        this.reconnectTimeout = null;
        this.connect();
      }
    }, this.reconnectDelay);
  }

  send(data: any): boolean {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(typeof data === "string" ? data : JSON.stringify(data));
      return true;
    }
    return false;
  }

  sendIdentification(coords: WindowCoordinates): boolean {
    console.log("📤 Sending identification:", coords);
    return this.send(coords);
  }

  getWindowCoordinates(): WindowCoordinates {
    const bottomX = window.screenX;
    const bottomY = window.screenY + window.outerHeight;

    return {
      bottom_left_x: bottomX,
      bottom_left_y: bottomY,
      window_width: window.outerWidth,
    };
  }

  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN || false;
  }

  disconnect(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    this.ws?.close();
  }
}
