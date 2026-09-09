import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { page, userEvent } from "vitest/browser";

import OverviewApp from "../../src/windows/overview/OverviewApp.svelte";
import type {
  AgentCommand,
  AgentIslandBridge,
  AgentViewSnapshot,
  CommandResult,
  DisplayStatus,
  HookEvent,
  SessionLifecycle,
  SessionView,
} from "../../src/bridge/types";

const mounted: Array<ReturnType<typeof mount>> = [];

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function session(
  id: string,
  lifecycle: SessionLifecycle,
  status: DisplayStatus,
  cwd: string,
  marker: string,
): SessionView {
  return {
    id,
    name: `${id} name`,
    cwd,
    log_path: `${cwd}\\session.jsonl`,
    records: [{ event_id: `${id}-event`, at_ms: 100, role: "Assistant", text: marker }],
    recent_output: ["legacy recent_output must not win"],
    current_file: `${cwd}\\src\\main.ts`,
    log_status: null,
    alert: null,
    lifecycle,
    last_active_at_ms: 100,
    display_status: status,
  };
}

function snapshot(generatedAt: number, order = ["alpha", "beta"]): AgentViewSnapshot {
  const definitions = {
    alpha: {
      id: "alpha",
      name: "Claude Code",
      status: "working" as const,
      active: session("alpha-live", "Active", "working", "D:\\live\\alpha", "Alpha live log"),
      history: session("alpha-history", "Historical", "done", "D:\\history\\alpha", "Alpha history log"),
    },
    beta: {
      id: "beta",
      name: "Codex CLI",
      status: "error" as const,
      active: session("beta-live", "Active", "error", "D:\\live\\beta", "Beta live failure"),
      history: session("beta-history", "Historical", "waiting", "D:\\history\\beta", "Beta history waiting"),
    },
  };

  return {
    schema_version: 1,
    generated_at_ms: generatedAt,
    agents: order.map((id) => {
      const definition = definitions[id as keyof typeof definitions];
      return {
        id: definition.id,
        name: definition.name,
        can_restart: true,
        state: {
          process: "running" as const,
          turn: definition.status === "error" ? "failed" as const : "executing" as const,
          attention: "none" as const,
          result_at_ms: definition.status === "error" ? generatedAt : null,
        },
        display_status: definition.status,
        active_session: structuredClone(definition.active),
        active_turn: {
          agent_id: definition.id,
          session_id: definition.active.id,
          turn_id: `${definition.id}-turn`,
        },
        history_sessions: [structuredClone(definition.history)],
        diagnostic: null,
        freshness: { observed_at_ms: generatedAt, stale: false },
        usage: null,
      };
    }),
  };
}

interface BridgeHarness {
  bridge: AgentIslandBridge;
  snapshotCalls(): number;
  commands: AgentCommand[];
}

function createBridgeHarness(
  snapshots: AgentViewSnapshot[],
  commandHandler?: (command: AgentCommand) => Promise<CommandResult>,
): BridgeHarness {
  const queue = snapshots.map((value) => structuredClone(value));
  const first = queue[0];
  if (!first) throw new Error("overview_snapshot_required");
  let latest = structuredClone(first);
  let calls = 0;
  const commands: AgentCommand[] = [];
  const listeners = new Set<(event: HookEvent) => void>();

  const bridge: AgentIslandBridge = {
    async getSnapshot() {
      calls += 1;
      latest = queue.shift() ?? latest;
      return structuredClone(latest);
    },
    async runCommand(command): Promise<CommandResult> {
      commands.push(structuredClone(command));
      if (commandHandler) return commandHandler(command);
      if (command.name === "send_to_session") return { ok: true, value: "task-1" };
      if (command.name === "get_send_output") return { ok: true, value: { done: true, lines: [] } };
      return { ok: true };
    },
    async listenHookEvents(handler) {
      listeners.add(handler);
      return () => listeners.delete(handler);
    },
    async resizeWindow() {},
  };

  return { bridge, snapshotCalls: () => calls, commands };
}

async function waitFor(check: () => boolean, timeoutMs = 1_000) {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function renderOverview(
  harness: BridgeHarness,
  pollIntervalMs = 0,
): Promise<HTMLElement> {
  const target = document.createElement("div");
  target.style.height = "100vh";
  document.body.append(target);
  mounted.push(mount(OverviewApp, { target, props: { bridge: harness.bridge, pollIntervalMs } }));
  await waitFor(
    () => target.querySelector('[data-testid="overview-app"]') !== null
      && target.querySelector('[data-testid="overview-row"]') !== null,
  );
  return target;
}

function row(target: HTMLElement, agentId: string, sessionId: string): HTMLButtonElement {
  const value = target.querySelector<HTMLButtonElement>(
    `[data-testid="overview-row"][data-agent-id="${agentId}"][data-session-id="${sessionId}"]`,
  );
  if (!value) throw new Error(`overview_row_missing:${agentId}:${sessionId}`);
  return value;
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  localStorage.clear();
  await page.viewport(800, 700);
});

describe("Svelte session overview", () => {
  it("makes historical sessions selectable in the overview", async () => {
    const target = await renderOverview(createBridgeHarness([snapshot(100)]));
    const historical = row(target, "alpha", "alpha-history");

    expect(historical.dataset.lifecycle).toBe("Historical");
    historical.click();
    await waitFor(() => historical.getAttribute("aria-selected") === "true");
    expect(target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent).toBe("alpha-history");
    expect(target.querySelector('[data-testid="overview-path"]')?.textContent).toBe("D:\\history\\alpha");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-07-svelte-frontend-migration/task7-overview.png",
      });
      await page.viewport(390, 700);
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-07-svelte-frontend-migration/task7-overview-narrow.png",
      });
    }
  });

  it("preserves the selected agent/session IDs when polling reorders rows", async () => {
    const harness = createBridgeHarness([snapshot(100), snapshot(200, ["beta", "alpha"])]);
    const target = await renderOverview(harness, 50);
    row(target, "beta", "beta-history").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "beta-history");

    await waitFor(() => harness.snapshotCalls() >= 2);

    expect(target.querySelector('[data-testid="overview-selected-agent-id"]')?.textContent).toBe("beta");
    expect(target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent).toBe("beta-history");
    expect(row(target, "beta", "beta-history").getAttribute("aria-selected")).toBe("true");
  });

  it("does not lose an in-progress send input during background polling", async () => {
    const harness = createBridgeHarness([snapshot(100), snapshot(200, ["beta", "alpha"])]);
    const target = await renderOverview(harness, 50);
    row(target, "beta", "beta-live").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "beta-live");
    const input = target.querySelector<HTMLInputElement>('[data-testid="overview-prompt"]')!;

    await userEvent.fill(input, "continue beta safely");
    await waitFor(() => harness.snapshotCalls() >= 2);

    expect(input.value).toBe("continue beta safely");
    expect(target.querySelector<HTMLInputElement>('[data-testid="overview-prompt"]')).toBe(input);
  });

  it("renders status, typed log and path from one selected session identity", async () => {
    const target = await renderOverview(createBridgeHarness([snapshot(100)]));
    row(target, "beta", "beta-history").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "beta-history");

    expect(target.querySelector('[data-testid="overview-status"]')?.textContent).toBe("等待确认");
    expect(target.querySelector('[data-testid="overview-path"]')?.textContent).toBe("D:\\history\\beta");
    expect(target.querySelector('[data-testid="overview-log"]')?.textContent).toContain("Beta history waiting");
    expect(target.querySelector('[data-testid="overview-log"]')?.textContent).not.toContain("Beta live failure");
    expect(target.textContent).not.toContain("legacy recent_output must not win");
  });

  it("binds session actions to the exact selected agent/session pair", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderOverview(harness);
    row(target, "beta", "beta-history").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "beta-history");

    target.querySelector<HTMLButtonElement>('[data-testid="overview-open-terminal"]')!.click();
    await waitFor(() => harness.commands.length === 1);
    const restart = target.querySelector<HTMLButtonElement>('[data-testid="overview-restart"]')!;
    await waitFor(() => restart.disabled === false);
    restart.click();
    await waitFor(() => harness.commands.length === 2);

    expect(harness.commands).toEqual([
      { name: "open_session_terminal", agentName: "Codex CLI", sessionId: "beta-history" },
      { name: "restart_session", agentName: "Codex CLI", sessionId: "beta-history" },
    ]);
    expect(target.querySelector<HTMLButtonElement>('[data-testid="overview-stop"]')!.disabled).toBe(true);
    row(target, "alpha", "alpha-live").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "alpha-live");
    expect(target.querySelector<HTMLButtonElement>('[data-testid="overview-restart"]')!.disabled).toBe(true);
  });

  it("sends the draft to the selected session without borrowing another row", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderOverview(harness);
    row(target, "alpha", "alpha-history").click();
    const input = target.querySelector<HTMLInputElement>('[data-testid="overview-prompt"]')!;
    await userEvent.fill(input, "review only this history");
    target.querySelector<HTMLButtonElement>('[data-testid="overview-send"]')!.click();

    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({
      name: "send_to_session",
      agentName: "Claude Code",
      sessionId: "alpha-history",
      prompt: "review only this history",
    });
    await waitFor(() => input.value === "");
    await waitFor(() => harness.commands.some((command) => command.name === "get_send_output"), 1_500);
    await waitFor(() => target.querySelector('[data-testid="overview-log"]')?.textContent?.includes("已完成（无输出）") === true);
  });

  it("keeps a delayed action failure attached to the session that started it", async () => {
    const pending = deferred<CommandResult>();
    const harness = createBridgeHarness([snapshot(100)], async (command) => {
      if (command.name === "open_session_terminal") return pending.promise;
      return { ok: true };
    });
    const target = await renderOverview(harness);
    row(target, "alpha", "alpha-history").click();
    target.querySelector<HTMLButtonElement>('[data-testid="overview-open-terminal"]')!.click();
    await waitFor(() => harness.commands.length === 1);

    row(target, "beta", "beta-history").click();
    await waitFor(() => target.querySelector('[data-testid="overview-selected-session-id"]')?.textContent === "beta-history");
    pending.resolve({ ok: false, code: "alpha_terminal_failed", message: "private path", retryable: false });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(target.textContent).not.toContain("alpha_terminal_failed");
    expect(target.textContent).not.toContain("private path");

    row(target, "alpha", "alpha-history").click();
    await waitFor(() => target.textContent?.includes("alpha_terminal_failed") === true);
  });

  it("locks the selected statistics period while its report is pending", async () => {
    const pending = deferred<CommandResult>();
    const harness = createBridgeHarness([snapshot(100)], async (command) => {
      if (command.name === "get_stats_report") return pending.promise;
      return { ok: true };
    });
    const target = await renderOverview(harness);
    target.querySelector<HTMLButtonElement>('[data-testid="overview-stats-toggle"]')!.click();
    await waitFor(() => harness.commands.some((command) => command.name === "get_stats_report"));

    const days = target.querySelector<HTMLButtonElement>('[data-testid="overview-stats-days"]')!;
    expect(days.disabled).toBe(true);
    expect(days.textContent).toContain("最近 7 天");
    days.click();
    expect(days.textContent).toContain("最近 7 天");
  });
});
