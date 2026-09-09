import { mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";
import App from "../../src/app/App.svelte";
import { createMockBridge } from "../../src/bridge/mock";
import type { AgentViewSnapshot } from "../../src/bridge/types";
import { DEFAULT_LAYOUTS } from "../../src/layout/presets";
import { LAYOUT_STORAGE_KEY } from "../../src/layout/persistence";

const NOW = 1_789_130_000_000;
let app: ReturnType<typeof mount> | undefined;

function snapshot(status: "idle" | "working" | "error", longLog: boolean): AgentViewSnapshot {
  return {
    schema_version: 1, generated_at_ms: NOW,
    agents: [{
      id: "visual-agent", name: "Claude Code", can_restart: true,
      state: { process: "running", turn: status === "working" ? "executing" : status === "error" ? "failed" : "idle", attention: "none", result_at_ms: status === "error" ? NOW : null },
      display_status: status,
      active_session: {
        id: "visual-session", name: "灵动岛界面验收", cwd: "D:\\demo\\agent-island", log_path: null,
        records: [
          { event_id: "user-1", at_ms: NOW - 20_000, role: "User", text: "请检查布局保存与状态显示。" },
          { event_id: "assistant-1", at_ms: NOW - 10_000, role: "Assistant", text: longLog ? "正在检查布局保存、键盘排序与会话状态。".repeat(80) : "布局已读取，正在核对当前会话。" },
        ],
        recent_output: [], current_file: "src/app/App.svelte", log_status: null,
        alert: status === "error" ? "构建失败，请查看当前会话日志。" : null,
        lifecycle: "Active", last_active_at_ms: NOW, display_status: status,
      },
      active_turn: status === "working" ? { agent_id: "visual-agent", session_id: "visual-session", turn_id: "turn-1" } : null,
      history_sessions: [],
      diagnostic: { adapter: "claude", code: "ok", freshness: { observed_at_ms: NOW, stale: false }, event_type: "assistant", event_count: 7, message_count: 3 },
      freshness: { observed_at_ms: NOW, stale: false },
      usage: { tokens_total: 12_500, tokens_output: 2_500, cost_usd: 1.25, used_percent: 42, window_secs: 18_000, resets_at_secs: NOW / 1000 + 3600, credits: null, unlimited: false, stale: false },
    }],
  };
}

afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  document.body.replaceChildren();
  localStorage.clear();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

for (const theme of ["graphite", "pure-black"] as const) {
  for (const scenario of ["compact-idle", "compact-working", "compact-error", "monitoring", "approval", "long-log", "single-column"] as const) {
    it(`${theme} ${scenario}`, async () => {
      vi.useFakeTimers({ toFake: ["Date"] });
      vi.setSystemTime(NOW);
      await page.viewport(scenario === "single-column" ? 360 : 520, 900);
      localStorage.setItem("agent-island-theme-style-v2", theme);
      localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(DEFAULT_LAYOUTS.monitoring));
      const style = document.createElement("style");
      style.textContent = "html,body { margin:0; background:#181b24; } *,*::before,*::after { animation:none!important; transition:none!important; caret-color:transparent!important; }";
      document.body.append(style);
      const target = document.createElement("div");
      document.body.append(target);
      const status = scenario === "compact-idle" ? "idle" : scenario === "compact-error" ? "error" : "working";
      const bridge = createMockBridge([snapshot(status, scenario === "long-log" || scenario === "single-column")]);
      app = mount(App, { target, props: {
        bridge, pollIntervalMs: 0, initiallyExpanded: !scenario.startsWith("compact"),
        diagnosticsBridge: { getDiagnostics: async () => ({ generated_at_ms: NOW, issues: [] }), exportDiagnostics: async () => undefined },
      } });
      await expect.poll(() => target.textContent).toContain("Claude Code");
      await expect.poll(() => bridge.listenerCount()).toBe(1);
      if (scenario === "approval") {
        bridge.emitHookEvent({ kind: "approval", approval: {
          id: "approval-1", session: "visual-session", tool: "Bash", command: "npm run check && npm run build", cwd: "D:\\demo\\agent-island",
        } });
        await expect.element(page.getByTestId("approval-allow")).toBeVisible();
      }
      await document.fonts.ready;
      await expect(page).toMatchScreenshot(`${theme}-${scenario}`, {
        comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
        screenshotOptions: { animations: "disabled", caret: "hide" },
      });
    });
  }
}
