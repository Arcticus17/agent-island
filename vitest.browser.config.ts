import { existsSync } from "node:fs";
import { playwright } from "@vitest/browser-playwright";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

const configuredChrome = process.env.AGENT_ISLAND_CHROME_PATH;
const windowsChrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const chromePath = configuredChrome ?? windowsChrome;

export default defineConfig({
  plugins: [svelte()],
  test: {
    include: ["tests/browser/**/*.test.ts"],
    browser: {
      enabled: true,
      provider: playwright({
        launchOptions: existsSync(chromePath) ? { executablePath: chromePath } : {},
      }),
      instances: [{ browser: "chromium" }],
    },
  },
});
