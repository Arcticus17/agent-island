import { mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { page } from "vitest/browser";

import App from "../../src/app/App.svelte";
import { createMockBridge, type MockAgentIslandBridge } from "../../src/bridge/mock";
import type { AgentViewSnapshot, HookApproval, InboxItem, AgentDetection, AgentCandidate } from "../../src/bridge/types";
import { resetUpdateCache } from "../../src/agent/updates";

const mounted: Array<ReturnType<typeof mount>> = [];

function snapshot(): AgentViewSnapshot {
  const now = Date.now();
  return {
    schema_version: 1,
    generated_at_ms: now,
    agents: [
      {
        id: "claude",
        name: "Claude Code",
        can_restart: true,
        state: { process: "running", turn: "executing", attention: "none", result_at_ms: null },
        display_status: "working",
        active_session: {
          id: "alpha-live",
          name: "Alpha live",
          cwd: "D:\\work\\alpha",
          log_path: "D:\\work\\alpha\\session.jsonl",
          records: [],
          recent_output: [],
          current_file: "D:\\work\\alpha\\src\\main.ts",
          log_status: null,
          alert: null,
          lifecycle: "Active",
          last_active_at_ms: now,
          display_status: "working",
        },
        active_turn: { agent_id: "claude", session_id: "alpha-live", turn_id: "turn-1" },
        history_sessions: [],
        diagnostic: null,
        freshness: { observed_at_ms: now, stale: false },
        usage: null,
      },
    ],
  };
}

function inboxItem(overrides: Partial<InboxItem>): InboxItem {
  return {
    id: "i1",
    kind: "notification",
    severity: "normal",
    agent_id: "claude",
    agent_name: "Claude Code",
    session_id: "alpha-live",
    title: "Claude 通知",
    body: "需要你看一下",
    state: "open",
    created_at_ms: Date.now() - 60_000,
    snooze_until_ms: null,
    resolved_at_ms: null,
    dedupe_key: "k1",
    approval_id: null,
    ...overrides,
  };
}

function approval(id: string, overrides: Partial<HookApproval> = {}): HookApproval {
  return {
    id,
    session: "alpha-live",
    tool: "Edit",
    command: "src/auth/session.ts (+18 −4)",
    cwd: "D:\\work\\alpha",
    rule_key: "src/auth/session.ts",
    risk: "high",
    risk_reason: "触碰认证逻辑",
    repo: "D:\\work\\alpha",
    branch: "feat/session-refresh",
    diff: {
      file: "src/auth/session.ts",
      added: 1,
      removed: 1,
      truncated: false,
      lines: [
        { kind: "del", text: "if (expired) return null;" },
        { kind: "add", text: "if (expired) {" },
        { kind: "add", text: "  await refresh();" },
      ],
    },
    ...overrides,
  };
}

async function waitFor(check: () => boolean, timeoutMs = 1_500): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function render(
  bridge: MockAgentIslandBridge,
  props: Record<string, unknown> = {},
): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(App, {
    target,
    props: {
      bridge,
      pollIntervalMs: 0,
      initiallyExpanded: true,
      ...props,
    },
  });
  mounted.push(component);
  await waitFor(() => target.querySelector('[data-testid="agent-island"]') !== null);
  return target;
}

function bridgeWith(
  inbox: InboxItem[],
  results: Array<{ ok: true } | { ok: false; code: string; message: string; retryable: boolean }> = [],
): MockAgentIslandBridge {
  return createMockBridge(
    Array.from({ length: 12 }, () => snapshot()),
    { inbox, commandResults: results },
  );
}

beforeEach(() => {
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

describe("inbox", () => {
  it("renders persisted items and resolves one through the bridge", async () => {
    const items = [
      inboxItem({ id: "i1", title: "完成了", kind: "done" }),
      inboxItem({ id: "i2", title: "需要审批 · Bash", kind: "approval", approval_id: "a1" }),
    ];
    const bridge = bridgeWith(items, []);
    const target = await render(bridge);

    await waitFor(() => target.querySelector('[data-testid="inbox-panel"]') !== null);
    expect(target.querySelector('[data-testid="inbox-count"]')?.textContent).toBe("2");
    expect(target.querySelector('[data-inbox-id="i1"]')?.textContent).toContain("完成了");
    expect(target.querySelector('[data-inbox-id="i2"]')?.textContent).toContain("需要审批");

    target.querySelector<HTMLButtonElement>('[data-inbox-id="i1"] [data-testid="inbox-resolve"]')!.click();
    await waitFor(() => target.querySelector('[data-inbox-id="i1"]') === null);
    expect(target.querySelector('[data-testid="inbox-count"]')?.textContent).toBe("1");
    expect(bridge.inbox().find((item) => item.id === "i1")?.state).toBe("resolved");
  });

  it("shows the inbox count on the compact capsule", async () => {
    const bridge = bridgeWith([
      inboxItem({ id: "i1", title: "one" }),
      inboxItem({ id: "i2", title: "two" }),
    ]);
    const target = await render(bridge, { initiallyExpanded: false, initiallyPeeked: true });
    await waitFor(() => target.querySelector('[data-testid="compact-badge"][data-inbox="true"]') !== null);
    expect(target.querySelector('[data-testid="compact-badge"]')?.textContent).toBe("2");
    expect(target.querySelector('[data-testid="compact-status"]')?.textContent).toBe("待处理");
  });

  it("caps the expanded list instead of rendering an unbounded inbox", async () => {
    const items = Array.from({ length: 8 }, (_, index) => inboxItem({
      id: `i-${index}`,
      title: `item-${index}`,
    }));
    const bridge = bridgeWith(items);
    const target = await render(bridge);
    await waitFor(() => target.querySelector('[data-testid="inbox-panel"]') !== null);
    expect(target.querySelectorAll('[data-inbox-id]').length).toBe(6);
    expect(target.querySelector('[data-testid="inbox-more"]')?.textContent).toContain("还有 2 条");
  });

  it("shows an away digest for items created before the last visit", async () => {
    localStorage.setItem("agent-island-inbox-seen", String(Date.now() - 20 * 60_000));
    const bridge = bridgeWith([
      inboxItem({ id: "i1", title: "one", created_at_ms: Date.now() - 5 * 60_000 }),
      inboxItem({ id: "i2", title: "two", created_at_ms: Date.now() - 4 * 60_000 }),
    ]);
    const target = await render(bridge);
    await waitFor(() => target.querySelector('[data-testid="inbox-digest"]') !== null);
    expect(target.querySelector('[data-testid="inbox-digest"]')?.textContent).toContain("你离开的 20 分钟");
    target.querySelector<HTMLButtonElement>('[data-testid="inbox-digest-dismiss"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="inbox-digest"]') === null);
  });
});

describe("approval 2.0", () => {
  it("shows risk, reason and diff, and sends the selected scope", async () => {
    const bridge = bridgeWith([], [{ ok: true }]);
    const target = await render(bridge);
    await waitFor(() => bridge.listenerCount() === 1);
    bridge.emitHookEvent({ kind: "approval", approval: approval("a-risk") });

    await waitFor(() => target.querySelector('[data-approval-id="a-risk"]') !== null);
    expect(target.querySelector('[data-testid="approval-risk"]')?.textContent).toBe("高风险");
    expect(target.querySelector('[data-testid="approval-risk-reason"]')?.textContent).toContain("触碰认证逻辑");
    const diff = target.querySelector('[data-testid="approval-diff"]');
    expect(diff?.textContent).toContain("src/auth/session.ts");
    expect(diff?.textContent).toContain("await refresh();");
    expect(target.querySelector('[data-testid="approval-remember"]')?.getAttribute("title"))
      .toContain("src/auth/session.ts");

    const scope = target.querySelector<HTMLSelectElement>('[data-approval-id="a-risk"] [data-testid="approval-scope"]')!;
    scope.value = "ten_minutes";
    scope.dispatchEvent(new Event("change", { bubbles: true }));
    target.querySelector<HTMLButtonElement>('[data-approval-id="a-risk"] [data-testid="approval-allow"]')!.click();

    await waitFor(() => bridge.commands().length === 1);
    expect(bridge.commands()[0]).toEqual({
      name: "respond_hook_approval",
      id: "a-risk",
      allow: true,
      scope: "ten_minutes",
    });
  });

  it("sends a deny reason back to the agent", async () => {
    const bridge = bridgeWith([], [{ ok: true }]);
    const target = await render(bridge);
    await waitFor(() => bridge.listenerCount() === 1);
    bridge.emitHookEvent({ kind: "approval", approval: approval("a-reason", { tool: "Bash", command: "npm run migrate -- --production" }) });

    await waitFor(() => target.querySelector('[data-approval-id="a-reason"]') !== null);
    target.querySelector<HTMLButtonElement>('[data-approval-id="a-reason"] [data-testid="approval-reason-toggle"]')!.click();
    const reason = await waitForInput(target, '[data-approval-id="a-reason"] [data-testid="approval-reason"]');
    reason.value = "先在 staging 验证";
    reason.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector<HTMLButtonElement>('[data-approval-id="a-reason"] [data-testid="approval-deny"]')!.click();

    await waitFor(() => bridge.commands().length === 1);
    expect(bridge.commands()[0]).toEqual({
      name: "respond_hook_approval",
      id: "a-reason",
      allow: false,
      reason: "先在 staging 验证",
    });
  });

  it("removes an approval card when the backend expires it", async () => {
    const bridge = bridgeWith([], []);
    const target = await render(bridge);
    await waitFor(() => bridge.listenerCount() === 1);
    bridge.emitHookEvent({ kind: "approval", approval: approval("a-exp") });
    await waitFor(() => target.querySelector('[data-approval-id="a-exp"]') !== null);
    bridge.emitHookEvent({ kind: "approval_expired", ids: ["a-exp"] });
    await waitFor(() => target.querySelector('[data-approval-id="a-exp"]') === null);
  });

  it("only batch-approves explicit low-risk items", async () => {
    const bridge = bridgeWith([], [{ ok: true }, { ok: true }]);
    const target = await render(bridge);
    await waitFor(() => bridge.listenerCount() === 1);
    bridge.emitHookEvent({ kind: "approval", approval: approval("low-1", { risk: "low", tool: "Bash", command: "npm test" }) });
    bridge.emitHookEvent({ kind: "approval", approval: approval("low-2", { risk: "low", tool: "Bash", command: "npm run lint" }) });
    bridge.emitHookEvent({ kind: "approval", approval: approval("high-1", { risk: "high", tool: "Bash", command: "rm -rf build" }) });

    await waitFor(() => target.querySelector('[data-testid="approval-batch-allow"]') !== null);
    target.querySelector<HTMLButtonElement>('[data-testid="approval-batch-allow"]')!.click();
    await waitFor(() => bridge.commands().length === 2);
    const ids = bridge.commands().map((command) => command.name === "respond_hook_approval" ? command.id : "");
    expect(ids.sort()).toEqual(["low-1", "low-2"]);
    expect(target.querySelector('[data-approval-id="high-1"]')).not.toBeNull();
  });

  it("shows exactly which configured agents were found on this machine", async () => {
    const detections: AgentDetection[] = [
      {
        id: "claude",
        name: "Claude Code",
        keyword: "claude",
        installed: true,
        running: true,
        process_count: 1,
        path: "D:\\node_global\\claude.cmd",
        source: "process",
      },
      {
        id: "hermes",
        name: "Hermes",
        keyword: "hermes",
        installed: false,
        running: false,
        process_count: 0,
        path: null,
        source: "missing",
      },
    ];
    const bridge = createMockBridge([snapshot()], { agentDetections: detections });
    const target = await render(bridge, {
      diagnosticsBridge: {
        getDiagnostics: async () => ({ issues: [], sources: [] }),
        exportDiagnostics: async () => undefined,
      },
    });
    const drawer = target.querySelector<HTMLDetailsElement>('[data-testid="diagnostics-drawer"]')!;
    drawer.open = true;
    drawer.dispatchEvent(new Event("toggle"));
    await waitFor(() => target.querySelectorAll('[data-testid="agent-detection-row"]').length === 2);

    const claude = target.querySelector('[data-testid="agent-detection-row"][data-agent-id="claude"]')!;
    const hermes = target.querySelector('[data-testid="agent-detection-row"][data-agent-id="hermes"]')!;
    expect(claude.getAttribute("data-installed")).toBe("true");
    expect(claude.textContent).toContain("D:\\node_global\\claude.cmd");
    expect(hermes.getAttribute("data-installed")).toBe("false");
    expect(hermes.textContent).toContain("未找到可执行文件");
  });
});

describe("agent add panel", () => {
  const candidates: AgentCandidate[] = [
    {
      name: "Claude Code",
      program: "claude",
      description: "已在岛内配置",
      installed: true,
      configured: true,
      path: "D:\\node_global\\claude.cmd",
      keyword: "claude",
    },
    {
      name: "Gemini CLI",
      program: "gemini",
      description: "Google Gemini CLI",
      installed: true,
      configured: false,
      path: "D:\\tools\\gemini.cmd",
      keyword: "gemini",
    },
  ];

  it("opens with + and adds a locally detected agent", async () => {
    const bridge = createMockBridge(
      Array.from({ length: 6 }, () => snapshot()),
      { agentCandidates: candidates },
    );
    const target = await render(bridge);
    target.querySelector<HTMLButtonElement>('[data-testid="agent-add"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="agent-add-panel"]') !== null);
    expect(target.querySelectorAll('[data-testid="agent-candidate"]').length).toBe(2);
    expect(target.querySelector('[data-configured="false"]')?.textContent).toContain("Gemini CLI");

    target.querySelector<HTMLButtonElement>('[data-configured="false"] [data-testid="agent-candidate-add"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="agent-add-panel"]') === null);
  });

  it("supports adding an agent manually when detection has nothing new", async () => {
    const bridge = createMockBridge(
      Array.from({ length: 6 }, () => snapshot()),
      { agentCandidates: candidates.filter((candidate) => candidate.configured) },
    );
    const target = await render(bridge);
    target.querySelector<HTMLButtonElement>('[data-testid="agent-add"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="agent-add-panel"]') !== null);

    const name = target.querySelector<HTMLInputElement>('[data-testid="agent-manual-name"]')!;
    const program = target.querySelector<HTMLInputElement>('[data-testid="agent-manual-program"]')!;
    name.value = "My Agent";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    program.value = "my-agent";
    program.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector<HTMLButtonElement>('[data-testid="agent-manual-add"]')!.click();

    await waitFor(() => target.querySelector('[data-testid="agent-add-panel"]') === null);
  });
});

async function waitForInput(target: HTMLElement, selector: string): Promise<HTMLTextAreaElement> {
  await waitFor(() => target.querySelector(selector) !== null);
  return target.querySelector<HTMLTextAreaElement>(selector)!;
}
