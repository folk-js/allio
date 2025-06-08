import { WebSocketClient } from "../client/interlay.ts";

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
  process_id: number;
}

class CoordinateOverlay {
  private coordinateCard: HTMLElement;
  private wsClient: WebSocketClient;

  constructor() {
    this.coordinateCard = document.getElementById("coordinateCard")!;
    this.wsClient = new WebSocketClient();
    this.setupWebSocketListener();
  }

  private async setupWebSocketListener() {
    try {
      // Set up message handler for window updates
      this.wsClient.onMessage = (data) => {
        if (data.windows) {
          this.updateCoordinateCard(data.windows);
        }
      };

      // Connect to websocket
      await this.wsClient.connect();

      console.log("📡 WebSocket window update listener established");
    } catch (error) {
      console.error("❌ Failed to setup websocket listener:", error);
    }
  }

  private updateCoordinateCard(windows: WindowInfo[]) {
    // Find the focused window
    const focusedWindow = windows.find((w) => w.focused);

    if (focusedWindow) {
      // Position the card slightly above and to the right of the window's top-left corner
      const cardX = focusedWindow.x + 8;
      const cardY = focusedWindow.y - 30; // 30px above the window

      // Get window ID and client status
      const windowId = focusedWindow.id;
      const hasClient = !!(focusedWindow as any).client_id;
      const clientStatus = hasClient ? "🔗" : "○";

      // Update card content and position - show coordinates and connection status
      this.coordinateCard.innerHTML = `
        <div style="font-size: 11px; opacity: 0.8; line-height: 1.2; color: ${
          hasClient ? "#4CAF50" : "#999"
        };">
          ${clientStatus} ${windowId}
        </div>
        <div style="line-height: 1.2;">${focusedWindow.x}, ${
        focusedWindow.y
      }</div>
      `;
      this.coordinateCard.style.left = `${cardX}px`;
      this.coordinateCard.style.top = `${cardY}px`;
      this.coordinateCard.style.display = "block";

      // Hide the card if it would be off-screen or too close to edges
      if (cardY < 0 || cardX < 0) {
        this.coordinateCard.style.display = "none";
      }
    } else {
      // No focused window, hide the card
      this.coordinateCard.style.display = "none";
    }
  }
}

// Initialize the overlay when the page loads
document.addEventListener("DOMContentLoaded", () => {
  new CoordinateOverlay();
  console.log("🎯 Coordinate overlay initialized");
});
