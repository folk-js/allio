import { InterlayClient } from "./interlay-client.ts";

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

class WindowOverlay {
  private windowContainer: HTMLElement;
  private wsClient: InterlayClient;
  private windowElements: Map<string, HTMLElement> = new Map();

  constructor() {
    this.windowContainer = document.getElementById("windowContainer")!;
    this.wsClient = new InterlayClient();
    this.setupWebSocketListener();
  }

  private async setupWebSocketListener() {
    try {
      // Set up message handler for window updates
      this.wsClient.onMessage = (data) => {
        if (data.windows) {
          this.updateWindowRectangles(data.windows);
        }
      };

      // Connect to websocket
      await this.wsClient.connect();

      console.log("📡 WebSocket window update listener established");
    } catch (error) {
      console.error("❌ Failed to setup websocket listener:", error);
    }
  }

  private updateWindowRectangles(windows: WindowInfo[]) {
    // Keep track of current window IDs
    const currentWindowIds = new Set(windows.map((w) => w.id));

    // Remove rectangles for windows that no longer exist
    for (const [windowId, element] of this.windowElements) {
      if (!currentWindowIds.has(windowId)) {
        element.remove();
        this.windowElements.delete(windowId);
      }
    }

    // Update or create rectangles for each window
    windows.forEach((window) => {
      this.updateWindowRectangle(window);
    });
  }

  private updateWindowRectangle(window: WindowInfo) {
    let windowElement = this.windowElements.get(window.id);

    // Create new rectangle element if it doesn't exist
    if (!windowElement) {
      windowElement = document.createElement("div");
      windowElement.className = "window-rectangle";

      // Create label element
      const label = document.createElement("div");
      label.className = "window-label";
      windowElement.appendChild(label);

      this.windowContainer.appendChild(windowElement);
      this.windowElements.set(window.id, windowElement);
    }

    // Get client status
    const hasClient = !!(window as any).client_id;
    const clientStatus = hasClient ? "🔗" : "○";

    // Update label content
    const label = windowElement.querySelector(".window-label") as HTMLElement;
    label.textContent = `${clientStatus} ${window.name} (${window.id})`;

    // Update CSS classes
    windowElement.className = "window-rectangle";
    label.className = "window-label";

    if (window.focused) {
      windowElement.classList.add("focused");
      label.classList.add("focused");
    }

    if (hasClient) {
      windowElement.classList.add("has-client");
      label.classList.add("has-client");
    }

    // Position and size the rectangle
    windowElement.style.left = `${window.x}px`;
    windowElement.style.top = `${window.y}px`;
    windowElement.style.width = `${window.w}px`;
    windowElement.style.height = `${window.h}px`;

    // Hide very small windows (they might be system UI elements)
    if (window.w < 50 || window.h < 50) {
      windowElement.style.display = "none";
    } else {
      windowElement.style.display = "block";
    }
  }
}

// Initialize the overlay when the page loads
document.addEventListener("DOMContentLoaded", () => {
  new WindowOverlay();
  console.log("🪟 Window overlay initialized");
});
