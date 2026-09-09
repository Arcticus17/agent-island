import { playwright } from "@vitest/browser-playwright";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Use the lockfile's Playwright Chromium, never an auto-updating system Chrome.
export default defineConfig({
  plugins: [svelte()],
  test: {
    update: "none",
    include: ["tests/visual/**/*.visual.test.ts"],
    attachmentsDir: "tests/visual/artifacts",
    browser: {
      enabled: true,
      headless: true,
      provider: playwright({ launchOptions: { channel: "chromium" }, contextOptions: {
        locale: "zh-CN", timezoneId: "Asia/Shanghai", deviceScaleFactor: 1,
        colorScheme: "dark", reducedMotion: "reduce",
      } }),
      instances: [{ browser: "chromium" }],
    },
  },
});
