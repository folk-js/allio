import { listen } from "@tauri-apps/api/event";

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

interface EnhancedWindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
  process_id: number;
  client_id?: string; // Full client UUID
}

interface WindowUpdatePayload {
  windows: WindowInfo[];
}

interface EnhancedWindowUpdatePayload {
  windows: EnhancedWindowInfo[];
}

class CoordinateOverlay {
  private coordinateCard: HTMLElement;

  constructor() {
    this.coordinateCard = document.getElementById("coordinateCard")!;
    this.setupWindowListener();
  }

  private async setupWindowListener() {
    try {
      // Listen for enhanced window updates with client information
      await listen<EnhancedWindowUpdatePayload>(
        "enhanced-window-update",
        (event) => {
          const { windows } = event.payload;
          this.updateCoordinateCard(windows);
        }
      );

      console.log("📡 Enhanced window update listener established");
    } catch (error) {
      console.error("❌ Failed to setup window listener:", error);
    }
  }

  private updateCoordinateCard(windows: EnhancedWindowInfo[]) {
    // Find the focused window
    const focusedWindow = windows.find((w) => w.focused);

    if (focusedWindow) {
      // Position the card slightly above and to the right of the window's top-left corner
      const cardX = focusedWindow.x + 8;
      const cardY = focusedWindow.y - 30; // 30px above the window

      // Get window ID and client status from enhanced window info
      const windowId = focusedWindow.id;
      const hasClient = !!focusedWindow.client_id;
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
