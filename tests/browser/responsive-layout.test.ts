import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { page } from "vitest/browser";

import App from "../../src/app/App.svelte";
import { createMockBridge, type MockAgentIslandBridge } from "../../src/bridge/mock";
import type { AgentViewSnapshot } from "../../src/bridge/types";
import { DEFAULT_LAYOUTS } from "../../src/layout/presets";
import { LAYOUT_STORAGE_KEY } from "../../src/layout/persistence";

const mounted: Array<ReturnType<typeof mount>> = [];

function snapshot(generatedAt = 100): AgentViewSnapshot {
  const longText = "长日志内容".repeat(120);
  return {
    schema_version: 1,
    generated_at_ms: generatedAt,
    agents: [{
      id: "alpha",
      name: "Claude Code",
      can_restart: true,
      state: { process: "running", turn: "executing", attention: "none", result_at_ms: null },
      display_status: "working",
      active_session: {
        id: "alpha-live",
        name: "Alpha live",
        cwd: `D:\\${"very-long-directory-name\\".repeat(20)}`,
        log_path: null,
        records: [{ event_id: `event-${generatedAt}`, at_ms: generatedAt, role: "Assistant", text: longText }],
        recent_output: [],
        current_file: `${"long-file-name-".repeat(20)}.ts`,
        log_status: null,
        alert: null,
        lifecycle: "Active",
        last_active_at_ms: generatedAt,
        display_status: "working",
      },
      active_turn: { agent_id: "alpha", session_id: "alpha-live", turn_id: "turn-1" },
      history_sessions: [],
      diagnostic: {
        adapter: "claude",
        code: "ok",
        freshness: { observed_at_ms: generatedAt, stale: false },
        event_type: "assistant",
        event_count: 7,
        message_count: 3,
      },
      freshness: { observed_at_ms: generatedAt, stale: false },
      usage: {
        tokens_total: 12_500,
        tokens_output: 2_500,
        cost_usd: 1.25,
        used_percent: 42,
        window_secs: 18_000,
        resets_at_secs: 1_789_130_031,
        credits: null,
        unlimited: false,
        stale: false,
      },
    }],
  };
}

async function waitFor(check: () => boolean, timeoutMs = 1_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function renderIsland(options: { pollIntervalMs?: number; snapshots?: AgentViewSnapshot[] } = {}): Promise<{
  target: HTMLElement;
  bridge: MockAgentIslandBridge;
}> {
  const target = document.createElement("div");
  document.body.append(target);
  const bridge = createMockBridge(options.snapshots ?? [snapshot()]);
  mounted.push(mount(App, {
    target,
    props: {
      bridge,
      diagnosticsBridge: { getDiagnostics: async () => ({ generated_at_ms: 100, issues: [] }), exportDiagnostics: async () => undefined },
      pollIntervalMs: options.pollIntervalMs ?? 0,
      initiallyExpanded: true,
    },
  }));
  await waitFor(() => target.querySelector('[data-testid="layout-editor"]') !== null);
  return { target, bridge };
}

function layoutGrid(target: HTMLElement): HTMLElement {
  return target.querySelector<HTMLElement>('[data-testid="layout-editor"] [data-testid="card-grid"]')!;
}

function assertNoHorizontalOverflow(target: HTMLElement): void {
  const island = target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth);
  expect(island.scrollWidth).toBeLessThanOrEqual(island.clientWidth);
  expect(layoutGrid(target).scrollWidth).toBeLessThanOrEqual(layoutGrid(target).clientWidth);
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  localStorage.clear();
  vi.restoreAllMocks();
  await page.viewport(800, 700);
});

describe("adaptive island responsive layout", () => {
  it("uses two logical columns around 520px and spans wide cards", async () => {
    await page.viewport(520, 900);
    const { target } = await renderIsland();
    const grid = layoutGrid(target);
    const columns = getComputedStyle(grid).gridTemplateColumns.split(" ").filter(Boolean);

    expect(columns).toHaveLength(2);
    for (const id of ["session", "log"]) {
      const card = target.querySelector<HTMLElement>(`[data-testid="layout-card"][data-card-id="${id}"]`)!;
      expect(getComputedStyle(card).gridColumnStart).toBe("1");
      expect(getComputedStyle(card).gridColumnEnd).toBe("-1");
    }
    expect(target.querySelector('[data-testid="layout-card"][data-card-id="stats"]')?.textContent).toContain("7");
    assertNoHorizontalOverflow(target);
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task4-responsive-520.png",
      });
    }
  });

  it.each([
    { width: 360, scale: "100%" },
    { width: 288, scale: "125%" },
    { width: 240, scale: "150%" },
  ])("degrades to one column without persisting at simulated $scale scale", async ({ width }) => {
    await page.viewport(width, 720);
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(DEFAULT_LAYOUTS.monitoring));
    const writes = vi.spyOn(Storage.prototype, "setItem");
    const { target, bridge } = await renderIsland();
    const grid = layoutGrid(target);

    expect(getComputedStyle(grid).gridTemplateColumns.split(" ").filter(Boolean)).toHaveLength(1);
    for (const card of target.querySelectorAll<HTMLElement>('[data-testid="layout-card"]')) {
      expect(getComputedStyle(card).gridColumnStart).toBe("auto");
      expect(getComputedStyle(card).gridColumnEnd).toBe("auto");
    }
    expect(writes.mock.calls.filter(([key]) => key === LAYOUT_STORAGE_KEY)).toHaveLength(0);
    assertNoHorizontalOverflow(target);
    expect(getComputedStyle(target.querySelector<HTMLElement>('[data-testid="compact-toggle"]')!).position).toBe("sticky");
    expect(getComputedStyle(target.querySelector<HTMLElement>('[aria-label="Agent 操作"]')!).position).toBe("sticky");

    await waitFor(() => bridge.listenerCount() === 1);
    bridge.emitHookEvent({
      kind: "approval",
      approval: {
        id: `approval-${width}`,
        session: "alpha-live",
        tool: "Bash",
        command: `npm run ${"very-long-command-".repeat(10)}`,
        cwd: "D:\\work\\alpha",
      },
    });
    await waitFor(() => target.querySelector('[data-testid="approval-allow"]') !== null);
    const island = target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;
    island.scrollTop = island.scrollHeight;
    for (const selector of ['[data-testid="action-overview"]', "details.settings", '[data-testid="approval-allow"]']) {
      const element = target.querySelector<HTMLElement>(selector)!;
      expect(getComputedStyle(element).display).not.toBe("none");
      expect(element.getBoundingClientRect().right).toBeLessThanOrEqual(window.innerWidth);
    }
    if (import.meta.env.VITE_CAPTURE_UI === "1" && width !== 288) {
      await page.screenshot({
        path: `../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task4-responsive-${width}.png`,
      });
    }
  });

  it("loads the saved logical layout and persists only a user layout change", async () => {
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(DEFAULT_LAYOUTS.minimal));
    const { target } = await renderIsland();

    expect([...target.querySelectorAll<HTMLElement>('[data-testid="layout-card"]')]
      .map(({ dataset }) => dataset.cardId)).toEqual(["status", "log"]);
    target.querySelector<HTMLButtonElement>('[data-testid="layout-edit-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="card-restore"][data-card-id="usage"]') !== null);
    target.querySelector<HTMLButtonElement>('[data-testid="card-restore"][data-card-id="usage"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="layout-card"][data-card-id="usage"]') !== null);

    const persisted = JSON.parse(localStorage.getItem(LAYOUT_STORAGE_KEY)!) as typeof DEFAULT_LAYOUTS.minimal;
    expect(persisted.preset).toBe("custom");
    expect(persisted.cards.find(({ id }) => id === "usage")?.visible).toBe(true);
    expect(target.querySelector('[data-testid="layout-persistence-status"]')).toBeNull();
  });

  it("does not label structured records as adapter message statistics", async () => {
    const withoutDiagnostic = snapshot();
    withoutDiagnostic.agents[0].diagnostic = null;
    expect(withoutDiagnostic.agents[0].active_session?.records.length).toBeGreaterThan(0);
    const { target } = await renderIsland({ snapshots: [withoutDiagnostic] });

    expect(target.querySelector('[data-testid="stats-event-count"]')?.textContent).toBe("—");
    expect(target.querySelector('[data-testid="stats-message-count"]')?.textContent).toBe("—");
    expect(target.querySelector('[aria-label="当前 Agent 统计"]')?.textContent).toContain("1会话");
  });

  it("switches from graphite glass to a solid pure-black theme", async () => {
    await page.viewport(520, 900);
    const { target } = await renderIsland();
    const island = target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;

    expect(island.dataset.themeStyle).toBe("graphite");
    expect(getComputedStyle(island).getPropertyValue("--surface-island").trim()).not.toBe("");
    expect(getComputedStyle(island).backdropFilter).not.toBe("none");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task5-graphite.png",
      });
    }

    const settings = target.querySelector<HTMLDetailsElement>("details.settings")!;
    settings.open = true;
    settings.dispatchEvent(new Event("toggle"));
    const theme = settings.querySelectorAll<HTMLSelectElement>("select")[1];
    theme.value = "pure-black";
    theme.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => island.dataset.themeStyle === "pure-black");
    settings.open = false;
    settings.dispatchEvent(new Event("toggle"));

    expect(getComputedStyle(island).getPropertyValue("--surface-island").trim()).toBe("#050506");
    expect(getComputedStyle(island).backdropFilter).toBe("none");
    expect(getComputedStyle(island).backgroundImage).toBe("none");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task5-pure-black.png",
      });
      await page.viewport(360, 720);
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task5-pure-black-narrow.png",
      });
    }
  });

  it("keeps edit mode through polling and reports persistence failure without blocking actions", async () => {
    const snapshots = Array.from({ length: 8 }, (_, index) => snapshot(100 + index));
    const originalSetItem = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      if (key === LAYOUT_STORAGE_KEY) throw new Error("quota exceeded");
      originalSetItem.call(this, key, value);
    });
    const { target, bridge } = await renderIsland({ snapshots, pollIntervalMs: 100 });
    const getSnapshot = vi.spyOn(bridge, "getSnapshot");
    target.querySelector<HTMLButtonElement>('[data-testid="layout-edit-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="layout-card-controls"]') !== null);
    const source = target.querySelector<HTMLElement>('[data-testid="layout-card"][data-card-id="status"]')!;
    source.dispatchEvent(new DragEvent("dragstart", {
      bubbles: true,
      dataTransfer: new DataTransfer(),
    }));
    await waitFor(() => getSnapshot.mock.calls.length > 0);
    expect(target.querySelector('[data-event-id="event-101"]')).toBeNull();
    source.dispatchEvent(new DragEvent("dragend", { bubbles: true }));
    await waitFor(() => target.querySelector('[data-event-id="event-101"]') !== null);
    target.querySelector<HTMLButtonElement>('[data-testid="card-move-up"][data-card-id="stats"]')!.click();

    await waitFor(() => target.querySelector('[data-testid="layout-persistence-status"]') !== null);
    expect(target.querySelector('[data-testid="layout-persistence-status"]')?.getAttribute("role")).toBe("status");
    expect(target.querySelector('[data-testid="layout-persistence-status"]')?.textContent).toContain("未保存");
    await waitFor(() => bridge.windowSizes().length > 1);
    expect(target.querySelector('[data-testid="layout-editor"]')?.getAttribute("data-editing")).toBe("true");
    expect(target.querySelector<HTMLButtonElement>('[data-testid="action-overview"]')?.disabled).toBe(false);
  });
});
