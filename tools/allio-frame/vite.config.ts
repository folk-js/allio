import { defineConfig } from "vite";
import cssInjectedByJsPlugin from "vite-plugin-css-injected-by-js";
import externals from "@inkandswitch/patchwork-bootloader/externals";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  base: "./",
  plugins: [cssInjectedByJsPlugin({ relativeCSSInjection: true })],

  resolve: {
    alias: {
      // Use the real Allio client straight from the workspace source, rather
      // than duplicating it. Patchwork/automerge deps come from npm (and are
      // marked external below so the host importmap provides them).
      allio: path.resolve(__dirname, "../../packages/allio-client/src/index.ts"),
    },
  },

  build: {
    cssCodeSplit: true,
    emptyOutDir: true,
    minify: false,
    sourcemap: true,
    rollupOptions: {
      external: externals,
      input: "./src/index.ts",
      output: {
        format: "es",
        entryFileNames: "[name].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name][extname]",
      },
      preserveEntrySignatures: "strict",
    },
  },
});
