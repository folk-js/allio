import { register } from "@tauri-apps/api/globalShortcut";
import { getCurrent } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/tauri";
import { Tldraw } from "tldraw";
import { useState, useEffect } from "react";
import "./App.css";

const appWindow = getCurrent();
let clickthrough = false;
await register("CommandOrControl+Shift+E", () => {
  console.log("Toggling clickthrough");
  clickthrough = !clickthrough;
  appWindow.setIgnoreCursorEvents(clickthrough);
});

// Add window management shortcut
await register("CommandOrControl+Shift+W", async () => {
  try {
    const windows = await invoke("get_windows");
    console.log("Available windows:", windows);

    const activeWindow = await invoke("get_active_window");
    console.log("Active window:", activeWindow);
  } catch (error) {
    console.error("Error getting window info:", error);
  }
});

interface WindowInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export default function App() {
  const [activeWindow, setActiveWindow] = useState<WindowInfo | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);

  // Function to fetch window information
  const fetchWindowInfo = async () => {
    try {
      const activeWin = (await invoke(
        "get_active_window"
      )) as WindowInfo | null;
      const allWindows = (await invoke("get_windows")) as WindowInfo[];

      setActiveWindow(activeWin);
      setWindows(allWindows);
    } catch (error) {
      console.error("Error fetching window info:", error);
    }
  };

  // Fetch window info on component mount
  useEffect(() => {
    fetchWindowInfo();

    // Set up interval to periodically update window info
    const interval = setInterval(fetchWindowInfo, 2000);

    return () => clearInterval(interval);
  }, []);

  return (
    <div style={{ position: "fixed", inset: 0 }}>
      <Tldraw
        persistenceKey="overlay"
        components={{
          MenuPanel: null,
          DebugPanel: null,
          Minimap: null,
          ZoomMenu: null,
          HelpMenu: null,
        }}
        cameraOptions={{
          isLocked: true,
        }}
      />

      {/* Window info overlay - for debugging/development */}
      <div
        style={{
          position: "absolute",
          top: 10,
          right: 10,
          background: "rgba(0, 0, 0, 0.8)",
          color: "white",
          padding: "10px",
          borderRadius: "5px",
          fontSize: "12px",
          maxWidth: "300px",
          pointerEvents: "none", // Don't interfere with drawing
          fontFamily: "monospace",
        }}
      >
        <div>Press Cmd+Shift+W to log window info</div>
        {activeWindow && (
          <div style={{ marginTop: "10px" }}>
            <strong>Active Window:</strong>
            <div>Name: {activeWindow.name}</div>
            <div>
              Size: {Math.round(activeWindow.w)}x{Math.round(activeWindow.h)}
            </div>
            <div>
              Position: ({Math.round(activeWindow.x)},{" "}
              {Math.round(activeWindow.y)})
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
