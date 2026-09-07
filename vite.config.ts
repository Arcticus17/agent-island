import { defineConfig } from "vitest/config";
import { fileURLToPath, URL } from "node:url";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  clearScreen: false,
  plugins: [svelte()],
  test: {
    include: ["tests/*.test.ts"],
  },
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        overview: fileURLToPath(new URL("./overview.html", import.meta.url)),
      },
    },
  },
  server: {
    host: "127.0.0.1",
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
