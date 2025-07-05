import { defineConfig } from "vite";

export default defineConfig({
  root: "./src-web",
  server: {
    port: 3000,
    host: "localhost",
    open: "/client/demo.html",
  },
  build: {
    outDir: "../dist-client",
    rollupOptions: {
      input: {
        demo: "./src-web/client/demo.html",
      },
    },
  },
});
