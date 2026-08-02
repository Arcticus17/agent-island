import { defineConfig } from "vite";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        overview: fileURLToPath(new URL("./overview.html", import.meta.url)),
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    headers: {
      "Cache-Control": "no-store, no-cache, must-revalidate",
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
