import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { page } from "vitest/browser";

import App from "../../src/app/App.svelte";
import type {
  AgentCommand,
  AgentIslandBridge,
  AgentViewSnapshot,
  CommandResult,
  DisplayStatus,
  HookApproval,
  HookEvent,
} from "../../src/bridge/types";

const mounted: Array<ReturnType<typeof mount>> = [];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function snapshot(
  generatedAt: number,
  status: DisplayStatus = "working",
  marker = "Initial snapshot marker",
): AgentViewSnapshot {
  return {
    schema_version: 1,
    generated_at_ms: generatedAt,
    agents: [
      {
        id: "alpha",
        name: "Claude Code",
        can_restart: true,
        state: {
          process: status === "stopped" ? "stopped" : "running",
          turn: status === "done" ? "succeeded" : "executing",
          attention: "none",
          result_at_ms: status === "done" ? generatedAt : null,
        },
        display_status: status,
        active_session: {
          id: "alpha-live",
          name: "Alpha live",
          cwd: "D:\\work\\alpha",
          log_path: "D:\\work\\alpha\\session.jsonl",
          records: [
            {
              event_id: `alpha-${generatedAt}`,
              at_ms: generatedAt,
              role: "Assistant",
              text: marker,
            },
          ],
          recent_output: [],
          current_file: "D:\\work\\alpha\\src\\main.ts",
          log_status: null,
          alert: null,
          lifecycle: "Active",
          last_active_at_ms: generatedAt,
          display_status: status,
        },
        active_turn: {
          agent_id: "alpha",
          session_id: "alpha-live",
          turn_id: "alpha-turn",
        },
        history_sessions: [],
        diagnostic: null,
        freshness: { observed_at_ms: generatedAt, stale: false },
        usage: null,
      },
    ],
  };
}

type CommandOutcome = CommandResult | Promise<CommandResult>;

interface BridgeHarness {
  readonly bridge: AgentIslandBridge;
  readonly commands: AgentCommand[];
  readonly snapshotCalls: () => number;
  readonly listenerCount: () => number;
  emit(event: HookEvent): void;
}

function createBridgeHarness(
  snapshots: Array<AgentViewSnapshot | Error>,
  commandOutcomes: CommandOutcome[] = [],
): BridgeHarness {
  const snapshotQueue = snapshots.map((value) => structuredClone(value));
  const firstSnapshot = snapshotQueue[0];
  if (!firstSnapshot || firstSnapshot instanceof Error) throw new Error("first_snapshot_must_succeed");
  let latestSnapshot = structuredClone(firstSnapshot);
  let getSnapshotCalls = 0;
  const outcomes = [...commandOutcomes];
  const commands: AgentCommand[] = [];
  const handlers = new Set<(event: HookEvent) => void>();

  const bridge: AgentIslandBridge = {
    async getSnapshot() {
      getSnapshotCalls += 1;
      const next = snapshotQueue.shift();
      if (next instanceof Error) throw next;
      if (next) latestSnapshot = next;
      return structuredClone(latestSnapshot);
    },
    async runCommand(command) {
      commands.push(structuredClone(command));
      const outcome = outcomes.shift();
      if (!outcome) throw new Error("test_command_outcome_missing");
      return structuredClone(await outcome);
    },
    async listenHookEvents(handler) {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    async resizeWindow() {},
  };

  return {
    bridge,
    commands,
    snapshotCalls: () => getSnapshotCalls,
    listenerCount: () => handlers.size,
    emit(event) {
      for (const handler of handlers) handler(structuredClone(event));
    },
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
  harness: BridgeHarness,
  options: { pollIntervalMs?: number } = {},
) {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(App, {
    target,
    props: {
      bridge: harness.bridge,
      pollIntervalMs: options.pollIntervalMs ?? 0,
      initiallyExpanded: true,
    },
  });
  mounted.push(component);
  await waitFor(() => target.querySelector('[data-testid="agent-island"]') !== null);
  return target;
}

function approvalEvent(id = "approval-1"): Extract<HookEvent, { kind: "approval" }> {
  const approval: HookApproval = {
    id,
    session: "alpha-live",
    tool: "Bash",
    command: "npm test",
    cwd: "D:\\work\\alpha",
  };
  return { kind: "approval", approval };
}

function snapshotWithSecondAgent(): AgentViewSnapshot {
  const value = snapshot(100);
  const beta = structuredClone(value.agents[0]);
  beta.id = "beta";
  beta.name = "Codex";
  beta.active_session!.id = "beta-live";
  beta.active_session!.name = "Beta live";
  beta.active_session!.display_status = "error";
  beta.active_turn!.agent_id = "beta";
  beta.active_turn!.session_id = "beta-live";
  beta.state.turn = "failed";
  beta.state.result_at_ms = 100;
  beta.display_status = "error";
  value.agents.push(beta);
  return value;
}

function claudeSnapshot(): AgentViewSnapshot {
  const value = snapshot(100);
  value.agents[0].id = "claude";
  value.agents[0].active_turn!.agent_id = "claude";
  return value;
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  localStorage.clear();
  await page.viewport(800, 700);
});

describe("Agent Island commands", () => {
  it("routes every core action through the typed bridge", async () => {
    const harness = createBridgeHarness(
      [snapshot(100)],
      Array.from({ length: 5 }, () => ({ ok: true }) as CommandResult),
    );
    const target = await renderIsland(harness);

    for (const testId of ["overview", "directory", "terminal", "focus", "restart"]) {
      target.querySelector<HTMLButtonElement>(`[data-testid="action-${testId}"]`)!.click();
      await waitFor(() => harness.commands.length === ["overview", "directory", "terminal", "focus", "restart"].indexOf(testId) + 1);
    }

    expect(harness.commands).toEqual([
      { name: "open_overview" },
      { name: "open_path", path: "D:\\work\\alpha" },
      { name: "open_session_terminal", agentName: "Claude Code", sessionId: "alpha-live" },
      { name: "focus_agent_terminal", agentName: "Claude Code" },
      { name: "restart_agent", agentName: "Claude Code" },
    ]);
  });

  it("requires a second stop click and blocks duplicate submissions while pending", async () => {
    const pending = deferred<CommandResult>();
    const harness = createBridgeHarness(
      [snapshot(100), snapshot(200, "stopped", "Stopped snapshot marker")],
      [pending.promise],
    );
    const target = await renderIsland(harness);
    const stop = target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]');

    expect(stop).not.toBeNull();
    stop!.click();
    expect(harness.commands).toHaveLength(0);
    await waitFor(() => stop!.dataset.actionState === "confirming");
    expect(stop!.textContent).toContain("确认停止");

    stop!.click();
    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({
      name: "stop_agent",
      agentName: "Claude Code",
    });
    expect(stop!.dataset.actionState).toBe("pending");
    expect(stop!.disabled).toBe(true);

    stop!.click();
    expect(harness.commands).toHaveLength(1);

    pending.resolve({ ok: true });
    await waitFor(() => stop!.dataset.actionState === "succeeded");
    await waitFor(() => target.querySelector('[data-testid="compact-status"]')?.textContent === "已停止");
    expect(target.textContent).toContain("Stopped snapshot marker");
  });

  it("cancels stop confirmation when the selected agent changes", async () => {
    const harness = createBridgeHarness([snapshotWithSecondAgent()], [{ ok: true }]);
    const target = await renderIsland(harness);
    const firstStop = target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]')!;
    firstStop.click();
    await waitFor(() => firstStop.dataset.actionState === "confirming");

    target.querySelector<HTMLButtonElement>('[data-agent-id="beta"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="selected-agent-id"]')?.textContent === "beta");
    const betaStop = target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]')!;
    expect(betaStop).not.toBe(firstStop);
    expect(betaStop.dataset.actionState).toBe("idle");
    betaStop.click();
    await waitFor(() => betaStop.dataset.actionState === "confirming");
    expect(harness.commands).toHaveLength(0);
    betaStop.click();
    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({ name: "stop_agent", agentName: "Codex" });
  });

  it.each([
    { retryable: true, expectedRetry: true },
    { retryable: false, expectedRetry: false },
  ])(
    "shows structured command failure and exposes retry only when retryable=$retryable",
    async ({ retryable, expectedRetry }) => {
      const harness = createBridgeHarness(
        [snapshot(100)],
        [
          {
            ok: false,
            code: retryable ? "backend_busy" : "invalid_request",
            message: retryable ? "temporarily unavailable" : "request is invalid",
            retryable,
          },
          { ok: true },
        ],
      );
      const target = await renderIsland(harness);
      const stop = target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]');

      expect(stop).not.toBeNull();
      stop!.click();
      await waitFor(() => stop!.dataset.actionState === "confirming");
      stop!.click();
      await waitFor(() => stop!.dataset.actionState === "failed");

      const feedback = target.querySelector('[data-testid="action-error"]');
      expect(feedback?.textContent).toContain(retryable ? "backend_busy" : "invalid_request");
      expect(feedback?.textContent).toContain(
        retryable ? "temporarily unavailable" : "request is invalid",
      );

      const retry = target.querySelector<HTMLButtonElement>('[data-testid="action-retry"]');
      expect(retry !== null).toBe(expectedRetry);
      if (retry) {
        retry.click();
        await waitFor(() => harness.commands.length === 2);
        expect(harness.commands[1]).toEqual(harness.commands[0]);
      }
    },
  );
});

describe("Agent Island event priority", () => {
  it("keeps approvals in a fixed layer ahead of ordinary notifications", async () => {
    const harness = createBridgeHarness([claudeSnapshot()]);
    const target = await renderIsland(harness);
    const eventLayer = target.querySelector<HTMLElement>('[data-testid="event-layer"]');

    expect(eventLayer).not.toBeNull();
    await waitFor(() => harness.listenerCount() === 1);
    harness.emit({
      kind: "notification",
      session: "alpha-live",
      message: "Needs attention",
    });
    harness.emit(approvalEvent());

    await waitFor(
      () => target.querySelector('[data-approval-id="approval-1"]') !== null,
    );
    const approvalStack = target.querySelector<HTMLElement>('[data-testid="approval-stack"]')!;
    const notificationStack = target.querySelector<HTMLElement>('[data-testid="notification-stack"]')!;
    expect(getComputedStyle(eventLayer!).position).toBe("fixed");
    expect(eventLayer!.firstElementChild).toBe(approvalStack);
    expect(approvalStack.textContent).toContain("npm test");
    expect(notificationStack.textContent).toContain("Needs attention");
    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-07-svelte-frontend-migration/task5-events.png",
      });
    }
  });

  it("defers a newer snapshot while an approval command is pending, then applies it", async () => {
    const pendingApproval = deferred<CommandResult>();
    const harness = createBridgeHarness(
      [
        snapshot(100, "working", "Initial snapshot marker"),
        snapshot(200, "done", "Deferred snapshot marker"),
      ],
      [pendingApproval.promise],
    );
    const target = await renderIsland(harness, { pollIntervalMs: 40 });
    const eventLayer = target.querySelector<HTMLElement>('[data-testid="event-layer"]');

    expect(eventLayer).not.toBeNull();
    await waitFor(() => harness.listenerCount() === 1);
    harness.emit(approvalEvent("approval-deferred"));
    await waitFor(
      () => target.querySelector('[data-approval-id="approval-deferred"]') !== null,
    );

    const allow = target.querySelector<HTMLButtonElement>(
      '[data-approval-id="approval-deferred"] [data-testid="approval-allow"]',
    );
    expect(allow).not.toBeNull();
    allow!.click();
    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({
      name: "respond_hook_approval",
      id: "approval-deferred",
      allow: true,
    });
    expect(allow!.disabled).toBe(true);

    await waitFor(() => harness.snapshotCalls() >= 2);
    expect(target.querySelector('[data-testid="expanded-status"]')?.textContent).toBe("工作中");
    expect(target.textContent).not.toContain("Deferred snapshot marker");

    pendingApproval.resolve({ ok: true });
    await waitFor(
      () => target.querySelector('[data-testid="expanded-status"]')?.textContent === "已完成",
    );
    expect(target.textContent).toContain("Deferred snapshot marker");
  });

  it("groups duplicate notifications, exposes full text, and supports manual dismissal", async () => {
    const harness = createBridgeHarness([claudeSnapshot()]);
    const target = await renderIsland(harness);
    await waitFor(() => harness.listenerCount() === 1);
    const message = "A long notification remains available to the user instead of disappearing behind a single-line ellipsis.";
    harness.emit({ kind: "notification", session: "alpha-live", message });
    harness.emit({ kind: "notification", session: "alpha-live", message });
    await waitFor(() => target.querySelectorAll('[data-notification-id]').length === 1);
    const notification = target.querySelector<HTMLElement>('[data-notification-id]')!;
    expect(notification.textContent).toContain(message);
    expect(notification.textContent).toContain("×2");
    notification.querySelector<HTMLButtonElement>('button[aria-label="关闭通知"]')!.click();
    await waitFor(() => target.querySelector('[data-notification-id]') === null);
  });

  it("keeps approval actions reachable in a short narrow viewport", async () => {
    await page.viewport(360, 320);
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    await waitFor(() => harness.listenerCount() === 1);
    for (let index = 1; index <= 4; index += 1) {
      const event = approvalEvent(`approval-overflow-${index}`);
      event.approval.command = `npm run command-with-a-long-name-${index} -- --verbose`;
      harness.emit(event);
    }
    await waitFor(() => target.querySelectorAll('[data-approval-id]').length === 4);

    const eventLayer = target.querySelector<HTMLElement>('[data-testid="event-layer"]')!;
    expect(eventLayer.clientHeight).toBeLessThan(eventLayer.scrollHeight);
    eventLayer.scrollTop = eventLayer.scrollHeight;
    await waitFor(() => eventLayer.scrollTop > 0);
    const finalAllow = target.querySelector<HTMLButtonElement>(
      '[data-approval-id="approval-overflow-4"] [data-testid="approval-allow"]',
    )!;
    expect(finalAllow.getBoundingClientRect().bottom).toBeLessThanOrEqual(window.innerHeight);
    const top = eventLayer.getBoundingClientRect().top;
    target.querySelector<HTMLElement>('[data-testid="agent-island"]')!.scrollTop = 100;
    expect(eventLayer.getBoundingClientRect().top).toBe(top);
  });

  it("rejects ambiguous or spoofed notification targets and retained snapshots after refresh failure", async () => {
    const ambiguous = claudeSnapshot();
    const duplicate = structuredClone(ambiguous.agents[0]);
    duplicate.id = "other";
    duplicate.name = "Claude Code";
    ambiguous.agents.push(duplicate);
    const harness = createBridgeHarness([ambiguous, new Error("refresh failed")]);
    const target = await renderIsland(harness, { pollIntervalMs: 30 });
    await waitFor(() => harness.listenerCount() === 1);
    harness.emit({ kind: "notification", session: "alpha-live", message: "Ambiguous" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(target.textContent).not.toContain("Ambiguous");

    await waitFor(() => target.querySelector(".refresh-note") !== null);
    harness.emit({ kind: "notification", session: "alpha-live", message: "Retained stale" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(target.textContent).not.toContain("Retained stale");

    const spoofed = snapshot(100);
    spoofed.agents[0].id = "not-claude";
    spoofed.agents[0].name = "Claude Code";
    const spoofedHarness = createBridgeHarness([spoofed]);
    const spoofedTarget = await renderIsland(spoofedHarness);
    await waitFor(() => spoofedHarness.listenerCount() === 1);
    spoofedHarness.emit({ kind: "notification", session: "alpha-live", message: "Spoofed name" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(spoofedTarget.textContent).not.toContain("Spoofed name");
  });
});

describe("Agent Island display settings", () => {
  it("persists privacy and theme settings and masks sensitive content", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const settings = target.querySelector<HTMLDetailsElement>("details.settings")!;
    settings.open = true;
    settings.dispatchEvent(new Event("toggle"));

    const privacy = settings.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    privacy.click();
    await waitFor(() => target.querySelector('[data-testid="agent-island"]')?.classList.contains("privacy") === true);
    expect(localStorage.getItem("agent-island-privacy")).toBe("1");
    expect(getComputedStyle(target.querySelector<HTMLElement>('[data-testid="session-path"]')!).filter).not.toBe("none");
    expect(target.querySelector('[data-testid="session-path"]')?.getAttribute("title")).toBeNull();

    const theme = settings.querySelectorAll<HTMLSelectElement>("select")[1];
    theme.value = "pure-black";
    theme.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => target.querySelector('[data-testid="agent-island"]')?.getAttribute("data-theme-style") === "pure-black");
    expect(localStorage.getItem("agent-island-theme-style-v2")).toBe("pure-black");
  });

  it("persists quiet focus mode and suppresses ordinary hook notifications", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    await waitFor(() => harness.listenerCount() === 1);
    const settings = target.querySelector<HTMLDetailsElement>("details.settings")!;
    settings.open = true;
    const focus = settings.querySelector<HTMLSelectElement>("select")!;
    focus.value = "quiet";
    focus.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => localStorage.getItem("agent-island-focus") === "quiet");

    harness.emit({ kind: "notification", session: "alpha-live", message: "Should stay quiet" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(target.querySelector('[data-testid="notification-stack"]')?.textContent).not.toContain("Should stay quiet");
  });

  it("loads and toggles the Claude hook connection from settings", async () => {
    const harness = createBridgeHarness(
      [snapshot(100)],
      [{ ok: true, value: false }, { ok: true, value: "已接入 Claude hooks" }],
    );
    const target = await renderIsland(harness);
    const settings = target.querySelector<HTMLDetailsElement>("details.settings")!;
    settings.open = true;
    settings.dispatchEvent(new Event("toggle"));
    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({ name: "get_hook_status" });
    const hookToggle = settings.querySelector<HTMLButtonElement>('[data-testid="hook-toggle"]')!;
    await waitFor(() => hookToggle.textContent?.includes("未接入") === true);
    hookToggle.click();
    await waitFor(() => harness.commands.length === 2);
    expect(harness.commands[1]).toEqual({ name: "set_hook_enabled", enabled: true });
    await waitFor(() => hookToggle.textContent?.includes("已接入") === true);
  });

  it("moves selection to a fresh failed agent in error-focus mode", async () => {
    const harness = createBridgeHarness([snapshotWithSecondAgent()]);
    const target = await renderIsland(harness);
    const settings = target.querySelector<HTMLDetailsElement>("details.settings")!;
    settings.open = true;
    const focus = settings.querySelector<HTMLSelectElement>("select")!;
    focus.value = "errors";
    focus.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => target.querySelector('[data-testid="selected-agent-id"]')?.textContent === "beta");
    expect(localStorage.getItem("agent-island-focus")).toBe("errors");
  });
});
