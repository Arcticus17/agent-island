import { mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { page } from "vitest/browser";

import App from "../../src/app/App.svelte";
import { resetUpdateCache } from "../../src/agent/updates";
import type { DiagnosticsBridge, DiagnosticsResponse } from "../../src/bridge/diagnostics";
import type {
  AgentCommand,
  AgentIslandBridge,
  AgentViewSnapshot,
  CommandResult,
  DisplayStatus,
  HookApproval,
  HookEvent,
  TerminalEvent,
  AgentTaskSnapshot,
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
  readonly terminalCalls: () => Array<{ kind: string; [key: string]: unknown }>;
  readonly taskCalls: () => Array<{ id: string; prompt: string }>;
  readonly resizeCalls: () => Array<{ width: number; height: number }>;
  emitTask(task: AgentTaskSnapshot): void;
  emit(event: HookEvent): void;
  emitTerminal(event: TerminalEvent): void;
  emitWindowFocus(focused: boolean): void;
  emitIslandHotkey(): void;
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
  const terminalHandlers = new Set<(event: TerminalEvent) => void>();
  const focusHandlers = new Set<(focused: boolean) => void>();
  const hotkeyHandlers = new Set<() => void>();
  const approvalRules: Array<{ id: string; tool: string; pattern: string }> = [];
  const approvalAudit: Array<{ at_ms: number; id: string; tool: string; command: string; rule: string; decision: string }> = [];
  const cliVersion = "0.1.0";
  const terminalLog: Array<{ kind: string; [key: string]: unknown }> = [];
  const taskLog: Array<{ id: string; prompt: string }> = [];
  const taskHandlers = new Set<(task: AgentTaskSnapshot) => void>();
  const resizeLog: Array<{ width: number; height: number }> = [];
  let terminalSequence = 0;

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
    async resizeWindow(size) {
      resizeLog.push({ ...size });
    },
    async terminalOpen(request) {
      terminalSequence += 1;
      const id = `term-${terminalSequence}`;
      terminalLog.push({ kind: "open", id, program: request.program, args: request.args ?? [], cwd: request.cwd ?? null });
      return id;
    },
    async terminalWrite(id, data) {
      terminalLog.push({ kind: "write", id, data });
    },
    async terminalResize(id, cols, rows) {
      terminalLog.push({ kind: "resize", id, cols, rows });
    },
    async terminalClose(id) {
      terminalLog.push({ kind: "close", id });
    },
    async listenTerminalEvents(handler) {
      terminalHandlers.add(handler);
      return () => terminalHandlers.delete(handler);
    },
    async onWindowFocusChange(handler) {
      focusHandlers.add(handler);
      return () => focusHandlers.delete(handler);
    },
    async listenIslandHotkey(handler) {
      hotkeyHandlers.add(handler);
      return () => hotkeyHandlers.delete(handler);
    },
    async listApprovalRules() {
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async addApprovalRule(tool, pattern) {
      approvalRules.push({ id: `rule-${approvalRules.length + 1}`, tool, pattern });
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async removeApprovalRule(id) {
      const index = approvalRules.findIndex((rule) => rule.id === id);
      if (index >= 0) approvalRules.splice(index, 1);
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async listApprovalAudit(limit) {
      return approvalAudit.slice(0, limit ?? 20).map((entry) => ({ ...entry }));
    },
    async startAgentTask(prompt) {
      const id = `task-${taskLog.length + 1}`;
      taskLog.push({ id, prompt });
      return id;
    },
    async cancelAgentTask() {
      return;
    },
    async listAgentTasks() {
      return [];
    },
    async onAgentTask(handler) {
      taskHandlers.add(handler);
      return () => taskHandlers.delete(handler);
    },
    async probeCliVersion(program) {
      return { program, version: cliVersion, raw: `mock ${cliVersion}`, ok: true };
    },
  };

  return {
    bridge,
    commands,
    snapshotCalls: () => getSnapshotCalls,
    listenerCount: () => handlers.size,
    terminalCalls: () => terminalLog.map((entry) => ({ ...entry })),
    taskCalls: () => taskLog.map((entry) => ({ ...entry })),
    emitTask(task: AgentTaskSnapshot) {
      for (const handler of taskHandlers) handler(structuredClone(task));
    },
    resizeCalls: () => resizeLog.map((entry) => ({ ...entry })),
    emit(event) {
      for (const handler of handlers) handler(structuredClone(event));
    },
    emitTerminal(event) {
      for (const handler of terminalHandlers) handler({ ...event });
    },
    emitWindowFocus(focused: boolean) {
      for (const handler of focusHandlers) handler(focused);
    },
    emitIslandHotkey() {
      for (const handler of hotkeyHandlers) handler();
    },
  };
}

async function waitForElement(target: HTMLElement, selector: string): Promise<HTMLInputElement> {
  await waitFor(() => target.querySelector(selector) !== null);
  return target.querySelector<HTMLInputElement>(selector)!;
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
  options: { pollIntervalMs?: number; diagnosticsBridge?: DiagnosticsBridge } = {},
) {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(App, {
    target,
    props: {
      bridge: harness.bridge,
      diagnosticsBridge: options.diagnosticsBridge,
      pollIntervalMs: options.pollIntervalMs ?? 0,
      initiallyExpanded: true,
    },
  });
  mounted.push(component);
  await waitFor(() => target.querySelector('[data-testid="agent-island"]') !== null);
  return target;
}

function diagnosticsBridgeFrom(
  getDiagnostics: () => Promise<DiagnosticsResponse>,
): DiagnosticsBridge {
  return { getDiagnostics, exportDiagnostics: async () => undefined };
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

beforeEach(() => {
  // 每个用例都别真的打 npm registry：默认返回与本地一致的版本
  resetUpdateCache();
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ version: "0.1.0" }), { status: 200 })));
});

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  localStorage.clear();
  vi.unstubAllGlobals();
  await page.viewport(800, 700);
});

describe("Agent Island commands", () => {
  it("waits for window restoration before auto-sizing and exposes width controls", async () => {
    const ready = deferred<() => void>();
    const harness = createBridgeHarness([snapshot(100)]);
    let sizes = 0;
    const widths: number[] = [];
    harness.bridge.initializeWindowPlacement = () => ready.promise;
    harness.bridge.resizeWindow = async () => { sizes++; };
    harness.bridge.adjustWindowWidth = async (delta) => { widths.push(delta); };
    harness.bridge.startWindowDrag = async () => { throw new Error('drag rejected'); };
    const target = await renderIsland(harness);
    expect(sizes).toBe(0);
    ready.resolve(() => {});
    await waitFor(() => sizes > 0);
    target.querySelector<HTMLButtonElement>('[aria-label="加宽窗口"]')!.click();
    await waitFor(() => widths.length === 1);
    expect(widths).toEqual([40]);
    target.querySelector('[aria-label="拖动窗口"]')!.dispatchEvent(new PointerEvent('pointerdown', { button: 0, bubbles: true }));
    await waitFor(() => target.textContent?.includes('drag rejected') === true);
  });

  it("expands settings below the controls instead of overlaying them", async () => {
    await page.viewport(360, 740);
    const target = await renderIsland(createBridgeHarness([snapshot(100)], [{ ok: true, value: true }]));
    const settings = target.querySelector<HTMLDetailsElement>('details.settings')!;
    settings.open = true;
    await waitFor(() => settings.querySelector<HTMLElement>('.popover')!.getBoundingClientRect().height > 0);
    const panel = settings.querySelector<HTMLElement>('.popover')!;
    const controls = target.querySelector<HTMLElement>('.actions')!;
    expect(getComputedStyle(panel).position).toBe('static');
    expect(panel.getBoundingClientRect().top).toBeGreaterThanOrEqual(controls.getBoundingClientRect().bottom);
    expect(panel.getBoundingClientRect().bottom).toBeLessThanOrEqual(window.innerHeight);
    if (import.meta.env.VITE_CAPTURE_UI === '1') await page.screenshot({ path: '../../.superpowers/inline-settings.png' });
  });

  it("does not silently replace a disappearing inspected record with the live conversation", async () => {
    const value = snapshot(100);
    const history = structuredClone(value.agents[0].active_session!);
    history.id = 'removed-history';
    history.lifecycle = 'Historical';
    history.records[0].text = 'Selected history only';
    value.agents[0].history_sessions = [history];
    const next = snapshot(200, 'working', 'Unrelated live message');
    const harness = createBridgeHarness([value, next], [{ ok: false, code: 'busy', message: 'Retry later', retryable: true }]);
    const target = await renderIsland(harness, { pollIntervalMs: 300 });
    const picker = target.querySelector<HTMLSelectElement>('[data-testid="session-picker"]')!;
    picker.value = history.id;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await waitFor(() => target.textContent?.includes('Selected history only') === true);
    target.querySelector<HTMLButtonElement>('[data-testid="action-directory"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="action-retry"]') !== null);
    await waitFor(() => harness.snapshotCalls() >= 2);
    await waitFor(() => target.textContent?.includes('该记录已不在当前快照中') === true);
    expect(target.textContent).not.toContain('Unrelated live message');
    expect(target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]')!.disabled).toBe(true);
    expect(target.querySelector('[data-testid="action-retry"]')).toBeNull();
    expect(harness.commands).toHaveLength(1);
  });

  it("only shows local history after explicit selection and scopes safe actions to that record", async () => {
    const value = snapshotWithSecondAgent();
    const history = structuredClone(value.agents[0].active_session!);
    history.id = "chosen-history";
    history.lifecycle = "Historical";
    history.cwd = "D:\\chosen-project";
    history.records[0].text = "Explicitly chosen local conversation";
    value.agents[0].active_session = null;
    value.agents[0].active_turn = null;
    value.agents[0].history_sessions = [history];
    const harness = createBridgeHarness([value], [{ ok: true }, { ok: true }]);
    const target = await renderIsland(harness);
    expect(target.textContent).not.toContain("Explicitly chosen local conversation");
    const picker = target.querySelector<HTMLSelectElement>('[data-testid="session-picker"]')!;
    picker.value = history.id;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await waitFor(() => target.textContent?.includes("Explicitly chosen local conversation") === true);
    expect(target.querySelector('[data-testid="expanded-status"]')?.textContent).toBe("本地记录");
    for (const action of ['stop', 'restart', 'focus']) expect(target.querySelector<HTMLButtonElement>(`[data-testid="action-${action}"]`)!.disabled).toBe(true);
    target.querySelector<HTMLButtonElement>('[data-testid="action-directory"]')!.click();
    await waitFor(() => harness.commands.length === 1);
    target.querySelector<HTMLButtonElement>('[data-testid="action-terminal"]')!.click();
    await waitFor(() => harness.commands.length === 2);
    expect(harness.commands).toEqual([
      { name: 'open_path', path: history.cwd },
      { name: 'open_session_terminal', agentName: value.agents[0].name, sessionId: history.id },
    ]);
    if (import.meta.env.VITE_CAPTURE_UI === "1") await page.screenshot({ path: "../../.superpowers/explicit-history.png" });
    const privacy = target.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    privacy.checked = true;
    privacy.dispatchEvent(new Event('change', { bubbles: true }));
    await waitFor(() => picker.textContent?.includes('内容已隐藏') === true);
    expect(picker.textContent).not.toContain(history.id);
    picker.value = '';
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await waitFor(() => !target.textContent?.includes("Explicitly chosen local conversation"));
    picker.value = history.id;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await waitFor(() => target.querySelector('[data-testid="history-inspection-note"]') !== null);
    target.querySelector<HTMLButtonElement>('[data-agent-id="beta"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="history-inspection-note"]') === null);
    expect(target.querySelector('[data-testid="session-path"]')?.textContent).not.toBe(history.cwd);
  });

  it("keeps failure details inside the visible sticky controls in a narrow tall layout", async () => {
    await page.viewport(360, 740);
    const value = snapshot(100);
    value.agents[0].active_session = null;
    value.agents[0].active_turn = null;
    const target = await renderIsland(createBridgeHarness([value]));
    target.querySelector<HTMLButtonElement>('[data-testid="action-directory"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="action-error"]') !== null);
    const error = target.querySelector<HTMLElement>('[data-testid="action-error"]')!;
    expect(error.closest('footer')).not.toBeNull();
    const bounds = error.getBoundingClientRect();
    expect(bounds.top).toBeGreaterThanOrEqual(0);
    expect(bounds.bottom).toBeLessThanOrEqual(window.innerHeight);
    if (import.meta.env.VITE_CAPTURE_UI === "1") await page.screenshot({ path: "../../.superpowers/recording-error-visible.png" });
  });

  it("dismisses settings outside and with Escape and gives native selects a dark scheme", async () => {
    const target = await renderIsland(createBridgeHarness([snapshot(100)], [{ ok: true }]));
    const settings = target.querySelector<HTMLDetailsElement>('details.settings')!;
    settings.open = true;
    expect(getComputedStyle(settings.querySelector('select')!).colorScheme).toBe('dark');
    target.querySelector('[data-testid="action-directory"]')!.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    expect(settings.open).toBe(false);
    settings.open = true;
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(settings.open).toBe(false);
    expect(document.activeElement).toBe(settings.querySelector('summary'));
  });

  it("keeps process focus available without a matched session and explains unsafe actions", async () => {
    const value = snapshot(100);
    value.agents[0].active_session = null;
    value.agents[0].active_turn = null;
    const harness = createBridgeHarness([value], [{ ok: true }]);
    const target = await renderIsland(harness);
    const focus = target.querySelector<HTMLButtonElement>('[data-testid="action-focus"]')!;
    expect(focus.disabled).toBe(false);
    focus.click();
    await waitFor(() => harness.commands.length === 1);
    expect(harness.commands[0]).toEqual({ name: "focus_agent_terminal", agentName: "Claude Code" });
    target.querySelector<HTMLButtonElement>('[data-testid="action-directory"]')!.click();
    await waitFor(() => target.textContent?.includes("尚未确认当前会话的工作目录") === true);
    target.querySelector<HTMLButtonElement>('[data-testid="action-terminal"]')!.click();
    await waitFor(() => target.textContent?.includes("无法确定终端应打开的位置") === true);
    expect(harness.commands).toHaveLength(1);
  });

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

  it("finishes a successful stop even while the supplementary diagnostics request is pending", async () => {
    const pendingDiagnostics = deferred<DiagnosticsResponse>();
    const postStopDiagnostics = deferred<DiagnosticsResponse>();
    let diagnosticCalls = 0;
    const harness = createBridgeHarness(
      [snapshot(100), snapshot(200, "stopped", "Stopped without diagnostics")],
      [{ ok: true }],
    );
    const target = await renderIsland(harness, {
      diagnosticsBridge: diagnosticsBridgeFrom(() => {
        diagnosticCalls += 1;
        return diagnosticCalls === 1
          ? pendingDiagnostics.promise
          : postStopDiagnostics.promise;
      }),
    });
    await waitFor(() => diagnosticCalls === 1);

    const stop = target.querySelector<HTMLButtonElement>('[data-testid="action-stop"]')!;
    stop.click();
    await waitFor(() => stop.dataset.actionState === "confirming");
    stop.click();

    await waitFor(() => stop.dataset.actionState === "succeeded");
    await waitFor(() => target.querySelector('[data-testid="compact-status"]')?.textContent === "已停止");
    expect(target.textContent).toContain("Stopped without diagnostics");
    expect(diagnosticCalls).toBe(1);

    pendingDiagnostics.resolve({
      generated_at_ms: 100,
      sources: [],
      issues: [{
        adapter: "codex",
        code: "pre_stop_diagnostic",
        freshness: { observed_at_ms: 100, stale: false },
        event_type: null,
        event_count: 0,
        message_count: 0,
      }],
    });
    await waitFor(() => diagnosticCalls === 2);
    expect(target.textContent).not.toContain("pre_stop_diagnostic");
    expect(target.textContent).toContain("正在读取诊断信息");

    postStopDiagnostics.resolve({
      generated_at_ms: 201,
      sources: [],
      issues: [{
        // 当前选中的是 Claude Code，诊断只显示它自己的 adapter
        adapter: "claude",
        code: "post_stop_diagnostic",
        freshness: { observed_at_ms: 201, stale: false },
        event_type: null,
        event_count: 0,
        message_count: 0,
      }],
    });
    await waitFor(() => target.textContent?.includes("post_stop_diagnostic") === true);
    expect(target.textContent).not.toContain("pre_stop_diagnostic");
  });

  it("only refreshes diagnostics while the drawer is open, single-flight", async () => {
    const firstDiagnostics = deferred<DiagnosticsResponse>();
    const trailingDiagnostics = deferred<DiagnosticsResponse>();
    let diagnosticCalls = 0;
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness, {
      pollIntervalMs: 20,
      diagnosticsBridge: diagnosticsBridgeFrom(() => {
        diagnosticCalls += 1;
        return diagnosticCalls === 1
          ? firstDiagnostics.promise
          : trailingDiagnostics.promise;
      }),
    });

    // 首轮取一次（徽标要用）；抽屉收起时不再跟着轮询
    await waitFor(() => diagnosticCalls === 1);
    await new Promise((resolve) => setTimeout(resolve, 90));
    expect(harness.snapshotCalls()).toBeGreaterThan(1);
    expect(diagnosticCalls).toBe(1);

    firstDiagnostics.resolve({ generated_at_ms: 100, issues: [], sources: [] });
    await new Promise((resolve) => setTimeout(resolve, 70));
    expect(diagnosticCalls).toBe(1);

    // 展开抽屉 → 立刻补一次，并且与进行中的请求单飞合并
    const drawer = target.querySelector<HTMLDetailsElement>('[data-testid="diagnostics-drawer"]')!;
    drawer.open = true;
    drawer.dispatchEvent(new Event("toggle"));
    await waitFor(() => diagnosticCalls === 2);
    await new Promise((resolve) => setTimeout(resolve, 70));
    expect(diagnosticCalls).toBe(2);
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
    // 不轮询：改用事件显式触发刷新，避免"第二次快照在点击之前就到了"的竞态
    const target = await renderIsland(harness);
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

    // 审批命令挂起期间，明确请求一次刷新：新快照必须被推迟
    harness.emit({ kind: "stop", session: "alpha-live" });
    await waitFor(() => harness.snapshotCalls() >= 2);
    await new Promise((resolve) => setTimeout(resolve, 30));
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
    // 这类"不该出现"的断言不能靠 sleep 猜时间：等一次真实轮询落地再断言。
    const settlePoll = async (harness: BridgeHarness): Promise<void> => {
      const before = harness.snapshotCalls();
      await waitFor(() => harness.snapshotCalls() > before, 2_000);
    };
    const ambiguous = claudeSnapshot();
    const duplicate = structuredClone(ambiguous.agents[0]);
    duplicate.id = "other";
    duplicate.name = "Claude Code";
    ambiguous.agents.push(duplicate);
    const harness = createBridgeHarness([ambiguous, new Error("refresh failed")]);
    const target = await renderIsland(harness, { pollIntervalMs: 30 });
    await waitFor(() => harness.listenerCount() === 1);
    harness.emit({ kind: "notification", session: "alpha-live", message: "Ambiguous" });
    await settlePoll(harness);
    expect(target.textContent).not.toContain("Ambiguous");

    await waitFor(() => target.querySelector(".refresh-note") !== null);
    harness.emit({ kind: "notification", session: "alpha-live", message: "Retained stale" });
    await settlePoll(harness);
    expect(target.textContent).not.toContain("Retained stale");

    const spoofed = snapshot(100);
    spoofed.agents[0].id = "not-claude";
    spoofed.agents[0].name = "Claude Code";
    const spoofedHarness = createBridgeHarness([spoofed]);
    const spoofedTarget = await renderIsland(spoofedHarness, { pollIntervalMs: 30 });
    await waitFor(() => spoofedHarness.listenerCount() === 1);
    spoofedHarness.emit({ kind: "notification", session: "alpha-live", message: "Spoofed name" });
    await settlePoll(spoofedHarness);
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

describe("island prompt composer", () => {
  function snapshotWithHistory(): AgentViewSnapshot {
    const base = snapshot(100);
    const agent = base.agents[0];
    const live = structuredClone(agent.active_session!);
    agent.history_sessions = [{ ...live, id: "alpha-history", name: "Alpha history", lifecycle: "Historical" }];
    return base;
  }

  it("sends a prompt from the island and streams the agent reply", async () => {
    const harness = createBridgeHarness(
      [snapshot(100)],
      [
        { ok: true, value: "send-task-1" },
        { ok: true, value: { done: false, lines: ["正在检查构建…"] } },
        { ok: true, value: { done: true, lines: ["正在检查构建…", "构建已通过"] } },
      ],
    );
    const target = await renderIsland(harness);

    const input = target.querySelector<HTMLInputElement>('[data-testid="prompt-input"]')!;
    expect(input.disabled).toBe(false);
    input.value = "帮我看看构建为什么失败";
    input.dispatchEvent(new Event("input", { bubbles: true }));

    const send = target.querySelector<HTMLButtonElement>('[data-testid="prompt-send"]')!;
    await waitFor(() => !send.disabled);
    send.click();

    await waitFor(() => harness.commands.some((command) => command.name === "send_to_session"));
    expect(harness.commands.find((command) => command.name === "send_to_session")).toEqual({
      name: "send_to_session",
      agentName: "Claude Code",
      sessionId: "alpha-live",
      prompt: "帮我看看构建为什么失败",
    });

    await waitFor(
      () => target.querySelector('[data-testid="prompt-output"]')?.textContent?.includes("正在检查构建…") === true,
      4_000,
    );
    await waitFor(
      () => target.querySelector('[data-testid="prompt-output"]')?.textContent?.includes("构建已通过") === true,
      4_000,
    );
    expect(target.querySelector('[data-testid="prompt-output"]')?.textContent).toContain("已结束");
    // 发出去之后输入框清空，避免重复发送同一条指令
    expect(target.querySelector<HTMLInputElement>('[data-testid="prompt-input"]')?.value).toBe("");
  });

  it("disables sending while inspecting a historical session", async () => {
    const harness = createBridgeHarness([snapshotWithHistory()], []);
    const target = await renderIsland(harness);
    const picker = target.querySelector<HTMLSelectElement>('[data-testid="session-picker"]')!;
    picker.value = "alpha-history";
    picker.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(
      () => target.querySelector<HTMLInputElement>('[data-testid="prompt-input"]')?.disabled === true,
    );
    expect(target.querySelector('[data-testid="prompt-send"]')?.hasAttribute("disabled")).toBe(true);
  });
});

describe("agent-scoped diagnostics", () => {
  it("shows only the selected agent's issues and sources", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness, {
      diagnosticsBridge: diagnosticsBridgeFrom(async () => ({
        generated_at_ms: 100,
        issues: [
          { adapter: "claude", code: "claude_issue", freshness: { observed_at_ms: 100, stale: false }, event_type: null, event_count: 1, message_count: 1 },
          { adapter: "codex", code: "codex_issue", freshness: { observed_at_ms: 100, stale: false }, event_type: null, event_count: 2, message_count: 2 },
        ],
        sources: [
          { source: "claude_usage", status: "ok", records: 3 },
          { source: "codex_usage", status: "ok", records: 4 },
        ],
      })),
    });

    const drawer = target.querySelector<HTMLDetailsElement>('[data-testid="diagnostics-drawer"]')!;
    drawer.open = true;
    await waitFor(() => target.textContent?.includes("claude_issue") === true);
    // 另一个 Agent 的诊断不混进来
    expect(target.textContent).not.toContain("codex_issue");
    expect(target.textContent).not.toContain("Codex 用量");
    expect(target.querySelector('[data-testid="diagnostics-count"]')?.textContent).toContain("1 项提示");
  });
});
describe("pending approvals stay visible", () => {
  it("marks the hidden notch while an approval is pending and refuses to auto-hide", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const island = () => target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;

    harness.emit({
      kind: "approval",
      approval: { id: "approval-keep", session: "alpha-live", tool: "Bash", command: "rm -rf build", cwd: "D:\\work\\alpha" },
    });
    await waitFor(() => target.querySelector('[data-testid="approval-allow"]') !== null);
    expect(island().getAttribute("data-presence")).toBe("expanded");

    // 收起成胶囊后：鼠标移开也不该自动隐藏（审批还挂着）
    target.querySelector<HTMLButtonElement>('[data-testid="compact-toggle"]')!.click();
    await waitFor(() => island().getAttribute("data-presence") === "peek");
    document.body.dispatchEvent(new Event("pointerleave"));
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(island().getAttribute("data-presence")).toBe("peek");

    // 再收起一次（点 ⌄ 之后手动隐藏不可达，改用 Esc）→ 热区带 pending 标记
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await waitFor(() => island().getAttribute("data-presence") === "hidden");
    expect(target.querySelector('[data-testid="idle-notch"]')?.getAttribute("data-pending")).toBe("true");
  });
});
describe("approval allow rules", () => {
  it("adds, lists and removes rules through the bridge", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const drawer = target.querySelector<HTMLDetailsElement>('[data-testid="approval-rules-drawer"]')!;
    drawer.open = true;
    drawer.dispatchEvent(new Event("toggle"));

    const input = await waitForElement(target, '[data-testid="approval-rule-input"]');
    input.value = "npm *";
    input.dispatchEvent(new Event("input"));
    target.querySelector<HTMLButtonElement>('[data-testid="approval-rule-add"]')!.click();

    await waitFor(() => target.querySelectorAll('[data-testid="approval-rule"]').length === 1);
    expect(target.querySelector('[data-testid="approval-rule"]')?.textContent).toContain("任意工具 · npm *");
    expect(target.querySelector('[data-testid="approval-rule-count"]')?.textContent).toContain("1 条");

    target.querySelector<HTMLButtonElement>('[data-testid="approval-rule-remove"]')!.click();
    await waitFor(() => target.querySelectorAll('[data-testid="approval-rule"]').length === 0);
  });
});
describe("remember an approval command", () => {
  it("adds a suggested rule from the approval card", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    harness.emit({
      kind: "approval",
      approval: { id: "approval-remember", session: "alpha-live", tool: "Bash", command: "npm install left-pad", cwd: "D:\\work\\alpha" },
    });
    const remember = await waitForElement(target, '[data-testid="approval-remember"]');
    expect(remember.getAttribute("title")).toContain("npm install *");
    (remember as unknown as HTMLButtonElement).click();

    await waitFor(() => target.querySelector('[data-testid="approval-rule"]') !== null || harness.bridge.listApprovalRules !== undefined);
    const rules = await harness.bridge.listApprovalRules!();
    expect(rules).toHaveLength(1);
    expect(rules[0]).toEqual({ id: expect.any(String), tool: "Bash", pattern: "npm install *" });
  });
});
describe("quota warning", () => {
  it("warns once when the agent usage crosses the threshold", async () => {
    localStorage.clear();
    const hot = snapshot(100);
    hot.agents[0].usage = { ...hot.agents[0].usage!, used_percent: 92, stale: false, resets_at_secs: Math.round(Date.now() / 1000) + 1800 };
    const harness = createBridgeHarness([hot]);
    const target = await renderIsland(harness);
    await waitFor(() => target.textContent?.includes("额度已用 92%") === true);
    expect(target.textContent).toContain("额度已用 92%");

    // 同一窗口再来一次快照：不再重复提醒
    const before = (target.textContent?.match(/额度已用 92%/g) ?? []).length;
    harness.emit({ kind: "stop", session: "alpha-live" });
    await new Promise((resolve) => setTimeout(resolve, 120));
    const after = (target.textContent?.match(/额度已用 92%/g) ?? []).length;
    expect(after).toBeLessThanOrEqual(before);
    localStorage.clear();
  });
});
describe("global hotkey", () => {
  it("summons the island from hidden and hides it again", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const island = () => target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;
    expect(island().getAttribute("data-presence")).toBe("expanded");

    // 展开态按快捷键 → 直接隐藏
    harness.emitIslandHotkey();
    await waitFor(() => island().getAttribute("data-presence") === "hidden");
    await waitFor(() => target.querySelector('[data-testid="idle-notch"]') !== null);

    // 隐藏态再按一次 → 展开
    harness.emitIslandHotkey();
    await waitFor(() => island().getAttribute("data-presence") === "expanded");
    expect(target.querySelector("#expanded-island")).not.toBeNull();
  });
});
describe("agent update button", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("checks the registry and offers an update when a newer version exists", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ version: "9.9.9" }), { status: 200 })));
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const button = () => target.querySelector<HTMLButtonElement>('[data-testid="agent-update"]')!;

    await waitFor(() => button().getAttribute("data-update-state") === "ready");
    expect(button().textContent).toContain("更新 v9.9.9");
    expect(button().getAttribute("title")).toContain("0.1.0");

    button().click();
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    const opened = harness.terminalCalls().find((call) => call.kind === "open")!;
    expect(opened.program).toBe("cmd.exe");
    expect(opened.args).toEqual(["/C", "npm install -g @anthropic-ai/claude-code@latest"]);
  });

  it("says up to date when the registry matches the local version", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ version: "0.1.0" }), { status: 200 })));
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const button = () => target.querySelector<HTMLButtonElement>('[data-testid="agent-update"]')!;
    await waitFor(() => button().getAttribute("data-update-state") === "ready");
    expect(button().textContent).toContain("已是最新");
    expect(button().classList.contains("available")).toBe(false);
  });

  it("falls back to a retry label when the registry is unreachable", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("boom", { status: 503 })));
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const button = () => target.querySelector<HTMLButtonElement>('[data-testid="agent-update"]')!;
    await waitFor(() => button().getAttribute("data-update-state") === "error");
    expect(button().textContent).toContain("重试检查");
  });
});
describe("collapse on window blur", () => {
  it("collapses the expanded island when the window loses focus", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const island = () => target.querySelector<HTMLElement>('[data-testid="agent-island"]')!;
    expect(island().getAttribute("data-presence")).toBe("expanded");

    // 点岛外面（窗口失焦）→ 收起成胶囊
    harness.emitWindowFocus(false);
    await waitFor(() => island().getAttribute("data-presence") === "peek");
    expect(target.querySelector("#expanded-island")).toBeNull();

    // 再失焦一次不该有副作用
    harness.emitWindowFocus(false);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(island().getAttribute("data-presence")).toBe("peek");
  });

  it("keeps the island expanded while the window has focus", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    harness.emitWindowFocus(true);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(target.querySelector('[data-testid="agent-island"]')!.getAttribute("data-presence")).toBe("expanded");
  });
});
describe("pixel mascot motion", () => {
  it.each([
    { status: "stopped" as const, resting: true },
    { status: "idle" as const, resting: true },
    { status: "done" as const, resting: true },
    { status: "working" as const, resting: false },
    { status: "waiting" as const, resting: false },
  ])("marks the mascot resting=$resting for $status", async ({ status, resting }) => {
    const harness = createBridgeHarness([snapshot(100, status)]);
    const target = await renderIsland(harness);
    const mascot = target.querySelector('[data-testid="agent-island"] .pixel-agent');
    expect(mascot).not.toBeNull();
    expect(mascot!.classList.contains("resting")).toBe(resting);
  });
});
describe("terminal explicit resize", () => {
  it("only tells the PTY about a size change when the user maximizes", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    await new Promise((resolve) => setTimeout(resolve, 300));
    const before = harness.terminalCalls().filter((call) => call.kind === "resize").length;
    const screen = target.querySelector<HTMLElement>('[data-testid="terminal-screen"]')!;
    screen.style.height = "260px";
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(harness.terminalCalls().filter((call) => call.kind === "resize").length).toBe(before);
    // 最大化是用户明确要求的尺寸变化 → 允许一次上报
    target.querySelector<HTMLButtonElement>('[data-testid="terminal-maximize"]')!.click();
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(harness.terminalCalls().filter((call) => call.kind === "resize").length).toBe(before + 1);
  });
});

describe("terminal resize storm", () => {
  it("reports at most one PTY resize for a burst of container resizes", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    const screen = target.querySelector<HTMLElement>('[data-testid="terminal-screen"]')!;
    // 等首帧对齐尺寸稳定下来，再开始"拖动"
    await new Promise((resolve) => setTimeout(resolve, 300));
    const baseline = harness.terminalCalls().filter((call) => call.kind === "resize").length;
    // 模拟拖动窗口：连续改高度（ResizeObserver 会连发很多次）
    for (const height of [140, 150, 160, 170, 180]) {
      screen.style.height = `${height}px`;
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
    const resizes = harness.terminalCalls().filter((call) => call.kind === "resize").length;
    // 拖动/伸缩只做本地 reflow：一次都不该通知 PTY（否则 TUI 反复重绘、scrollback 出现重复内容）
    expect(resizes - baseline).toBe(0);
  });
});
describe("terminal launch binding", () => {
  it("marks the terminal as bound when the agent was launched from the island", async () => {
    const bound = snapshot(100);
    (bound.agents[0] as { binding?: string }).binding = "launched";
    const harness = createBridgeHarness([bound]);
    const target = await renderIsland(harness);
    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    const badge = await waitForElement(target, '[data-testid="terminal-binding"]');
    expect(badge.textContent).toContain("已绑定");
    expect(badge.getAttribute("data-binding")).toBe("launched");

    // 不是岛内启动时不显示
    const inferredHarness = createBridgeHarness([snapshot(100)]);
    const inferredTarget = await renderIsland(inferredHarness);
    inferredTarget.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => inferredTarget.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => inferredHarness.terminalCalls().some((call) => call.kind === "open"));
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(inferredTarget.querySelector('[data-testid="terminal-binding"]')).toBeNull();
  });
});
describe("island task mode", () => {
  it("runs a codex task when the agent has no session, then reports completion", async () => {
    const stopped = snapshot(100, "stopped");
    stopped.agents[0].id = "codex";
    stopped.agents[0].name = "Codex CLI";
    stopped.agents[0].active_session = null;
    const harness = createBridgeHarness([stopped]);
    const target = await renderIsland(harness);

    const input = await waitForElement(target, '[data-testid="prompt-input"]') as HTMLInputElement;
    expect(input.placeholder).toContain("任务模式");
    input.value = "把 README 里的错别字改掉";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 30));
    const send = target.querySelector<HTMLButtonElement>('[data-testid="prompt-send"]')!;
    expect(send.disabled).toBe(false);
    send.click();

    await waitFor(() => harness.taskCalls().length === 1);
    expect(harness.taskCalls()[0].prompt).toBe("把 README 里的错别字改掉");
    await waitFor(() => target.querySelector('[data-testid="prompt-output"]') !== null);

    harness.emitTask({
      id: "task-1",
      agentId: "codex",
      state: "succeeded",
      sessionId: "thread-1",
      message: "补丁已应用",
    });
    await waitFor(() => target.querySelector('[data-testid="prompt-output"]')?.textContent?.includes("已结束") ?? false);
    expect(target.querySelector('[data-testid="prompt-output"]')?.textContent).toContain("补丁已应用");
  });
});
describe("island built-in terminal", () => {
  it("opens a PTY for the selected agent and closes it when toggled off", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    const opened = harness.terminalCalls().find((call) => call.kind === "open")!;
    // 选中的是 Claude Code，默认就用 claude 启动，并把会话目录带过去
    expect(opened.program).toBe("claude");
    expect(opened.cwd).toBe("D:\\work\\alpha");
    await waitFor(() => target.querySelector('[data-testid="terminal-phase"]')?.getAttribute("data-phase") === "running");

    // 终端输出经桥接流回来（内容最终画在 xterm 里，这里只验证事件通道不抛错）
    harness.emitTerminal({ kind: "data", id: String(opened.id), data: "pty-ready\r\n" });
    await new Promise((resolve) => setTimeout(resolve, 50));

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-stop"]')!.click();
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "close"));
    await waitFor(() => target.querySelector('[data-testid="terminal-phase"]')?.getAttribute("data-phase") === "idle");
  });

  it("keeps the agent session alive when the island collapses and reopens", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await waitFor(() => harness.terminalCalls().filter((call) => call.kind === "open").length === 1);

    // 收起岛 → 面板卸载；再展开 → 面板重新挂载。PTY 归 App 所有，不能被重启或关掉。
    const compact = () => target.querySelector<HTMLButtonElement>('[data-testid="compact-toggle"]')!;
    compact().click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') === null);
    compact().click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') !== null);
    await new Promise((resolve) => setTimeout(resolve, 120));

    expect(harness.terminalCalls().filter((call) => call.kind === "open")).toHaveLength(1);
    expect(harness.terminalCalls().some((call) => call.kind === "close")).toBe(false);
    expect(target.querySelector('[data-testid="terminal-phase"]')?.getAttribute("data-phase")).toBe("running");
  });
  it("closes the PTY when the terminal panel is hidden again", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const toggle = target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!;
    toggle.click();
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    toggle.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') === null);
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "close"));
  });
});


describe("island terminal maximize", () => {
  it("expands the terminal with the window and restores both", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!.click();
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    const viewport = window.innerWidth;

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-maximize"]')!.click();
    await waitFor(
      () => target.querySelector(".adaptive-details")?.classList.contains("terminal-max") === true,
    );
    expect(
      target.querySelector('[data-testid="terminal-screen"]')?.classList.contains("maximized"),
    ).toBe(true);
    // 放大时至少请求过一次窗口尺寸调整（宽度/高度由宿主决定）
    await waitFor(() => harness.resizeCalls().length > 0);

    target.querySelector<HTMLButtonElement>('[data-testid="terminal-maximize"]')!.click();
    await waitFor(
      () => target.querySelector(".adaptive-details")?.classList.contains("terminal-max") === false,
    );
    expect(
      target.querySelector('[data-testid="terminal-screen"]')?.classList.contains("maximized"),
    ).toBe(false);
  });

  it("restores the window width when the terminal is closed while maximized", async () => {
    const harness = createBridgeHarness([snapshot(100)]);
    const target = await renderIsland(harness);
    const toggle = target.querySelector<HTMLButtonElement>('[data-testid="terminal-toggle"]')!;
    toggle.click();
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "open"));
    target.querySelector<HTMLButtonElement>('[data-testid="terminal-maximize"]')!.click();
    await waitFor(
      () => target.querySelector(".adaptive-details")?.classList.contains("terminal-max") === true,
    );

    toggle.click();
    await waitFor(() => target.querySelector('[data-testid="terminal-panel"]') === null);
    await waitFor(
      () => target.querySelector(".adaptive-details")?.classList.contains("terminal-max") === false,
    );
    await waitFor(() => harness.terminalCalls().some((call) => call.kind === "close"));
  });
});





























