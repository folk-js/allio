import { defineConfig } from "vite";
import * as path from "node:path";
import * as fs from "node:fs";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Get all HTML files from overlays folder
const overlaysDir = path.resolve(__dirname, "src-web/overlays");
const htmlFiles = fs.existsSync(overlaysDir)
  ? fs.readdirSync(overlaysDir).filter((f) => f.endsWith(".html"))
  : [];

if (htmlFiles.length === 0) {
  console.warn("Warning: No overlay HTML files found in", overlaysDir);
}

const input = Object.fromEntries(
  htmlFiles.map((f) => [f.replace(".html", ""), path.join(overlaysDir, f)])
);

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  // Path aliases
  resolve: {
    alias: {
      "@axio/client": path.resolve(__dirname, "packages/axio-client/src"),
    },
  },
  // Build config for multi-page app
  root: "src-web/overlays",
  build: {
    outDir: path.resolve(__dirname, "dist"),
    emptyOutDir: true,
    rollupOptions: {
      input,
    },
  },
}));
