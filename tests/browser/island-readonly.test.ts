import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { page, userEvent } from "vitest/browser";

import App from "../../src/app/App.svelte";
import { createMockBridge } from "../../src/bridge/mock";
import type { AgentViewSnapshot, DisplayStatus } from "../../src/bridge/types";

const mounted: Array<ReturnType<typeof mount>> = [];

function session(
  agentId: string,
  id: string,
  status: DisplayStatus,
  cwd: string,
  text: string,
) {
  const records = Array.from({ length: 32 }, (_, index) => ({
    event_id: `${id}-event-${index + 1}`,
    at_ms: 1_700_000_000_000 + index,
    role: index % 3 === 0 ? "User" as const : index % 3 === 1 ? "Assistant" as const : "Tool" as const,
    text: index === 0 ? text : `${text} · line ${index + 1}`,
  }));
  return {
    id,
    name: `${agentId}-${id}`,
    cwd,
    log_path: `${cwd}\\session.jsonl`,
    records,
    recent_output: ["legacy output must not be parsed"],
    current_file: `${cwd}\\src\\main.ts`,
    log_status: "legacy-status-must-not-win",
    alert: null,
    lifecycle: "Active" as const,
    last_active_at_ms: 1_700_000_000_000,
    display_status: status,
  };
}

function snapshot(
  generatedAt: number,
  order = ["alpha", "beta"],
  appendBetaRecord = false,
): AgentViewSnapshot {
  const definitions = {
    alpha: {
      id: "alpha",
      name: "Claude Code",
      display_status: "working" as const,
      active_session: session("alpha", "alpha-live", "working", "D:\\work\\alpha", "Alpha is compiling"),
    },
    beta: {
      id: "beta",
      name: "Codex CLI",
      display_status: "error" as const,
      active_session: session("beta", "beta-live", "error", "D:\\work\\beta", "Beta failed safely"),
    },
  };

  return {
    schema_version: 1,
    generated_at_ms: generatedAt,
    agents: order.map((id) => {
      const definition = definitions[id as keyof typeof definitions];
      const activeSession = structuredClone(definition.active_session);
      if (appendBetaRecord && definition.id === "beta") {
        activeSession.records.push({
          event_id: "beta-live-event-new",
          at_ms: 1_700_000_000_100,
          role: "Assistant",
          text: "Beta update after refresh",
        });
      }
      return {
        ...definition,
        active_session: activeSession,
        state: {
          process: "running" as const,
          turn: definition.display_status === "error" ? "failed" as const : "executing" as const,
          attention: "none" as const,
          result_at_ms: null,
        },
        active_turn: {
          agent_id: definition.id,
          session_id: activeSession.id,
          turn_id: `${definition.id}-turn`,
        },
        history_sessions: [
          {
            ...session(definition.id, `${definition.id}-old`, "done", `D:\\archive\\${definition.id}`, "Historical text must stay out"),
            lifecycle: "Historical" as const,
          },
        ],
        diagnostic: null,
        freshness: { observed_at_ms: generatedAt, stale: false },
        usage: {
          tokens_total: definition.id === "beta" ? 6_337_526 : 12_500,
          tokens_output: definition.id === "beta" ? 24_262 : 2_500,
          cost_usd: definition.id === "beta" ? null : 1.25,
          used_percent: definition.id === "beta" ? 42 : null,
          window_secs: 18_000,
          resets_at_secs: 1_789_130_031,
          credits: null,
          unlimited: definition.id === "beta",
          stale: false,
        },
      };
    }),
  };
}

async function waitFor(check: () => boolean, timeoutMs = 1_000) {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function renderIsland(
  snapshots: unknown[],
  options: { expanded?: boolean; pollIntervalMs?: number } = {},
) {
  const target = document.createElement("div");
  document.body.append(target);
  const bridge = createMockBridge(snapshots);
  const component = mount(App, {
    target,
    props: {
      bridge,
      pollIntervalMs: options.pollIntervalMs ?? 0,
      initiallyExpanded: options.expanded ?? true,
    },
  });
  mounted.push(component);
  await waitFor(() => target.querySelector('[data-testid="agent-island"]') !== null);
  return { target, bridge };
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  await page.viewport(800, 700);
});

describe("read-only Agent Island", () => {
  it("starts in the system Chrome provider and renders the active display status", async () => {
    await page.viewport(1_440, 900);
    const { target } = await renderIsland([snapshot(100)]);
    expect(navigator.userAgent).toContain("Chrome");
    expect(target.querySelector('[data-testid="compact-status"]')?.textContent).toBe("工作中");
    expect(target.querySelector('[data-testid="expanded-status"]')?.textContent).toBe("工作中");
    expect(target.querySelector('[data-testid="agent-island"]')?.classList).toContain("status-working");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        element: target.querySelector<HTMLElement>('[data-testid="agent-island"]')!,
        path: "../../.superpowers/sdd/2026-09-07-svelte-frontend-migration/task4-expanded.png",
      });
    }
  });

  it("switches status, session, path and typed log as one agent identity", async () => {
    const { target } = await renderIsland([snapshot(100)]);
    const betaButton = target.querySelector<HTMLButtonElement>('[data-agent-id="beta"]');
    betaButton?.click();
    await waitFor(() => target.querySelector('[data-testid="active-session-id"]')?.textContent === "beta-live");

    expect(target.querySelector('[data-testid="compact-status"]')?.textContent).toBe("错误");
    expect(target.querySelector('[data-testid="status-session"]')?.textContent).toContain("beta-live");
    expect(target.querySelector('[data-testid="session-path"]')?.textContent).toBe("D:\\work\\beta");
    expect(target.querySelector('[data-event-id="beta-live-event-1"]')?.textContent).toContain("Beta failed safely");
    expect(target.querySelector('[data-testid="usage-total"]')?.textContent).toBe("6.34M");
    expect(target.querySelector('[data-testid="usage-detail"]')?.textContent).toContain("额度 42.0%");
    expect(target.querySelector('[data-testid="usage-detail"]')?.textContent).toContain("无限额度");
    expect(target.textContent).not.toContain("D:\\archive\\beta");
    expect(target.textContent).not.toContain("Historical text must stay out");
    expect(target.textContent).not.toContain("legacy output must not be parsed");
  });

  it("keeps selected identity, stable DOM nodes and scrolled-away log position across polling", async () => {
    const { target } = await renderIsland(
      [snapshot(100), snapshot(200, ["beta", "alpha"], true)],
      { pollIntervalMs: 120 },
    );
    target.querySelector<HTMLButtonElement>('[data-agent-id="beta"]')?.click();
    await waitFor(() => target.querySelector('[data-testid="active-session-id"]')?.textContent === "beta-live");

    const island = target.querySelector('[data-testid="agent-island"]');
    const compactStatus = target.querySelector('[data-testid="compact-status"]');
    const stableRecord = target.querySelector('[data-event-id="beta-live-event-1"]');
    const log = target.querySelector<HTMLElement>('[data-testid="log-scroll"]')!;
    expect(log.scrollHeight).toBeGreaterThan(log.clientHeight);
    log.scrollTop = 0;

    await waitFor(() => target.textContent?.includes("Beta update after refresh") === true);

    expect(target.querySelector('[data-testid="selected-agent-id"]')?.textContent).toBe("beta");
    expect(target.querySelector('[data-testid="agent-island"]')).toBe(island);
    expect(target.querySelector('[data-testid="compact-status"]')).toBe(compactStatus);
    expect(target.querySelector('[data-event-id="beta-live-event-1"]')).toBe(stableRecord);
    expect(log.scrollTop).toBe(0);
  });

  it("never substitutes history when the active session is unknown", async () => {
    const noActive = snapshot(100);
    noActive.agents[0] = {
      ...noActive.agents[0],
      active_session: null,
      active_turn: null,
      display_status: "idle",
    };
    const { target } = await renderIsland([noActive]);

    expect(target.querySelector('[data-testid="empty-session"]')?.textContent).toContain("无法确认当前会话");
    expect(target.textContent).not.toContain("alpha-old");
    expect(target.textContent).not.toContain("D:\\archive\\alpha");
  });

  it("marks retained usage as retained when the latest refresh fails", async () => {
    const { target } = await renderIsland(
      [snapshot(100), new Error("temporary poll failure")],
      { pollIntervalMs: 60 },
    );

    await waitFor(() => target.querySelector(".refresh-note") !== null);
    expect(target.querySelector('[data-testid="usage-freshness"]')?.textContent).toBe("保留上次用量");
  });

  it("distinguishes missing usage from a failed refresh without cached usage", async () => {
    const noUsage = snapshot(100);
    noUsage.agents[0].usage = null;
    const { target: currentTarget } = await renderIsland([noUsage]);
    expect(currentTarget.querySelector('[data-testid="usage-freshness"]')?.textContent).toBe("暂无用量数据");

    const failedNoUsage = snapshot(100);
    failedNoUsage.agents[0].usage = null;
    const { target: failedTarget } = await renderIsland(
      [failedNoUsage, new Error("temporary poll failure")],
      { pollIntervalMs: 60 },
    );
    await waitFor(() => failedTarget.querySelector(".refresh-note") !== null);
    expect(failedTarget.querySelector('[data-testid="usage-freshness"]')?.textContent).toBe("刷新失败，暂无缓存用量");
  });

  it("supports keyboard expansion and collapses cards to one column at narrow width", async () => {
    await page.viewport(360, 700);
    const { target, bridge } = await renderIsland([snapshot(100)], { expanded: false });
    const toggle = target.querySelector<HTMLButtonElement>('[data-testid="compact-toggle"]')!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => bridge.windowSizes().length > 0);
    expect(bridge.windowSizes().at(-1)).toEqual({ width: window.innerWidth, height: 60 });
    toggle.focus();
    await userEvent.keyboard("{Enter}");
    await waitFor(() => toggle.getAttribute("aria-expanded") === "true");
    await waitFor(() => (bridge.windowSizes().at(-1)?.height ?? 0) > 60);
    const island = target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;
    const renderedHeight = Math.ceil(Math.max(island.scrollHeight, island.getBoundingClientRect().height));
    const safeHeight = Math.max(60, Math.floor((window.screen.availHeight || renderedHeight) * 0.92));
    expect(bridge.windowSizes().at(-1)).toEqual({
      width: window.innerWidth,
      height: Math.min(renderedHeight, safeHeight),
    });
    expect(getComputedStyle(island).overflowY).toBe("auto");
    expect(island.scrollWidth).toBeLessThanOrEqual(island.clientWidth);

    const cards = target.querySelector<HTMLElement>('[data-testid="card-grid"]')!;
    expect(getComputedStyle(cards).gridTemplateColumns.split(" ")).toHaveLength(1);
    expect(target.querySelector('[role="status"]')).not.toBeNull();
    expect(toggle.getAttribute("aria-controls")).toBe("expanded-island");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        element: target.querySelector<HTMLElement>('[data-testid="agent-island"]')!,
        path: "../../.superpowers/sdd/2026-09-07-svelte-frontend-migration/task4-narrow.png",
      });
    }
    toggle.click();
    await waitFor(() => bridge.windowSizes().at(-1)?.height === 60);
  });
});
