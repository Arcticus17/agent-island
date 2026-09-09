<script lang="ts">
  import { onMount, untrack } from "svelte";
  import "../../themes/tokens.css";
  import "../../themes/graphite.css";
  import "../../themes/pure-black.css";
  import type {
    AgentCommand,
    AgentIslandBridge,
    AgentStatsRow,
    AgentView,
    CommandResult,
    DisplayStatus,
    SessionView,
    StatsReport,
  } from "../../bridge/types";
  import { tauriBridge } from "../../bridge/tauri";
  import { createAgentStore, type AgentStoreState } from "../../stores/agent-store";

  let {
    bridge = tauriBridge,
    pollIntervalMs = 2_000,
  }: { bridge?: AgentIslandBridge; pollIntervalMs?: number } = $props();

  interface OverviewRow {
    key: string;
    agent: AgentView;
    session: SessionView;
  }

  type SendState = {
    state: "pending" | "streaming" | "done" | "failed";
    code: string | null;
    lines: string[];
    taskId: string | null;
  };

  type ActionUiState = {
    state: "pending" | "failed";
    code: string | null;
    request: number;
  };

  const store = createAgentStore(untrack(() => bridge));
  let viewState: AgentStoreState = $state(store.current());
  let updatedAt = $state<number | null>(null);
  let drafts = $state<Record<string, string>>({});
  let sends = $state<Record<string, SendState>>({});
  let actions = $state<Record<string, ActionUiState>>({});
  let actionSequence = 0;
  let stopConfirmationKey = $state<string | null>(null);
  let stopConfirmationTimer: number | undefined;
  let statsVisible = $state(false);
  let statsDays = $state<1 | 7>(7);
  let statsState = $state<"idle" | "pending" | "ready" | "failed">("idle");
  let statsReport = $state<StatsReport | null>(null);
  let disposed = false;
  const sendTimers = new Map<string, number>();

  let rows = $derived.by((): OverviewRow[] => {
    const result: OverviewRow[] = [];
    for (const agent of viewState.snapshot?.agents ?? []) {
      const sessions = agent.active_session
        ? [agent.active_session, ...agent.history_sessions]
        : agent.history_sessions;
      const seen = new Set<string>();
      for (const session of sessions) {
        if (seen.has(session.id)) continue;
        seen.add(session.id);
        result.push({ key: `${agent.id}::${session.id}`, agent, session });
      }
    }
    return result;
  });
  let selectedKey = $derived(
    viewState.selectedAgent && viewState.selectedSession
      ? `${viewState.selectedAgent.id}::${viewState.selectedSession.id}`
      : null,
  );
  let selectedRow = $derived(rows.find((row) => row.key === selectedKey) ?? null);
  let selectedAction = $derived(selectedKey ? actions[selectedKey] : undefined);

  const statusText: Record<DisplayStatus, string> = {
    stopped: "已停止",
    idle: "等待中",
    working: "工作中",
    done: "已完成",
    error: "报错",
    waiting: "等待确认",
  };

  function isRecord(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function parseStatsReport(value: unknown): StatsReport | null {
    if (!isRecord(value) || !Array.isArray(value.days)) return null;
    const days: StatsReport["days"] = [];
    for (const day of value.days) {
      if (!isRecord(day) || typeof day.date !== "string" || !Array.isArray(day.agents)) return null;
      const agents: AgentStatsRow[] = [];
      for (const agent of day.agents) {
        if (
          !isRecord(agent) || typeof agent.name !== "string" ||
          !Number.isSafeInteger(agent.total_seconds) || Number(agent.total_seconds) < 0 ||
          !Number.isSafeInteger(agent.error_count) || Number(agent.error_count) < 0 ||
          !Number.isSafeInteger(agent.done_count) || Number(agent.done_count) < 0
        ) return null;
        agents.push({
          name: agent.name,
          total_seconds: Number(agent.total_seconds),
          error_count: Number(agent.error_count),
          done_count: Number(agent.done_count),
        });
      }
      days.push({ date: day.date, agents });
    }
    return { days };
  }

  function selectRow(row: OverviewRow): void {
    if (stopConfirmationTimer !== undefined) window.clearTimeout(stopConfirmationTimer);
    stopConfirmationKey = null;
    store.selectAgent(row.agent.id);
    store.selectSession(row.session.id);
  }

  function selectFirstAvailable(): void {
    const first = rows[0];
    if (first) selectRow(first);
  }

  async function refresh(): Promise<void> {
    await store.refresh();
    if (!disposed && !store.current().refreshError) updatedAt = Date.now();
  }

  async function runAction(key: string, command: AgentCommand): Promise<CommandResult> {
    const request = ++actionSequence;
    actions[key] = { state: "pending", code: null, request };
    let result: CommandResult;
    try {
      result = await bridge.runCommand(command);
    } catch {
      result = { ok: false, code: "command_failed", message: "command_failed", retryable: true };
    }
    if (disposed || actions[key]?.request !== request) return result;
    if (result.ok) {
      if (command.name === "stop_session" || command.name === "restart_session") await refresh();
      if (!disposed && actions[key]?.request === request) delete actions[key];
    } else {
      actions[key] = { state: "failed", code: result.code, request };
    }
    return result;
  }

  function stopSelected(): void {
    if (!selectedRow || selectedRow.session.lifecycle !== "Active" || selectedRow.session.display_status === "stopped" || selectedAction?.state === "pending") return;
    if (stopConfirmationKey !== selectedRow.key) {
      stopConfirmationKey = selectedRow.key;
      if (stopConfirmationTimer !== undefined) window.clearTimeout(stopConfirmationTimer);
      stopConfirmationTimer = window.setTimeout(() => {
        if (stopConfirmationKey === selectedRow?.key) stopConfirmationKey = null;
      }, 3_000);
      return;
    }
    stopConfirmationKey = null;
    if (stopConfirmationTimer !== undefined) window.clearTimeout(stopConfirmationTimer);
    void runAction(selectedRow.key, {
      name: "stop_session",
      agentName: selectedRow.agent.name,
      sessionId: selectedRow.session.id,
    });
  }

  function updateDraft(key: string, value: string): void {
    drafts[key] = value;
  }

  async function pollSend(key: string, taskId: string): Promise<void> {
    const current = sends[key];
    if (!current || current.taskId !== taskId || current.state === "done" || current.state === "failed") return;
    let result: CommandResult;
    try {
      result = await bridge.runCommand({ name: "get_send_output", taskId });
    } catch {
      result = { ok: false, code: "send_poll_failed", message: "send_poll_failed", retryable: true };
    }
    if (disposed) return;
    const latest = sends[key];
    if (!latest || latest.taskId !== taskId) return;
    if (!result.ok) {
      sends[key] = { ...latest, state: "failed", code: result.code };
      return;
    }
    if (
      !isRecord(result.value) || typeof result.value.done !== "boolean" ||
      !Array.isArray(result.value.lines) || !result.value.lines.every((line) => typeof line === "string")
    ) {
      sends[key] = { ...latest, state: "failed", code: "invalid_send_output" };
      return;
    }
    const output = result.value as { done: boolean; lines: string[] };
    sends[key] = {
      ...latest,
      state: output.done ? "done" : "streaming",
      lines: [...output.lines],
    };
    if (!output.done) {
      sendTimers.set(key, window.setTimeout(() => void pollSend(key, taskId), 800));
    }
  }

  async function sendPrompt(): Promise<void> {
    if (!selectedRow) return;
    const key = selectedRow.key;
    const prompt = (drafts[key] ?? "").trim();
    if (!prompt || sends[key]?.state === "pending" || sends[key]?.state === "streaming") return;
    const agentName = selectedRow.agent.name;
    const sessionId = selectedRow.session.id;
    sends[key] = { state: "pending", code: null, lines: [], taskId: null };
    let result: CommandResult;
    try {
      result = await bridge.runCommand({ name: "send_to_session", agentName, sessionId, prompt });
    } catch {
      result = { ok: false, code: "send_failed", message: "send_failed", retryable: true };
    }
    if (disposed) return;
    if (!result.ok) {
      sends[key] = { state: "failed", code: result.code, lines: [], taskId: null };
      return;
    }
    if (typeof result.value !== "string" || result.value.length === 0) {
      sends[key] = { state: "failed", code: "invalid_send_task", lines: [], taskId: null };
      return;
    }
    const taskId = result.value;
    drafts[key] = "";
    sends[key] = { state: "streaming", code: null, lines: [], taskId };
    sendTimers.set(key, window.setTimeout(() => void pollSend(key, taskId), 800));
  }

  async function loadStats(): Promise<void> {
    if (statsState === "pending") return;
    statsState = "pending";
    let result: CommandResult;
    try {
      result = await bridge.runCommand({ name: "get_stats_report", days: statsDays });
    } catch {
      result = { ok: false, code: "stats_failed", message: "stats_failed", retryable: true };
    }
    if (disposed) return;
    const report = result.ok ? parseStatsReport(result.value) : null;
    if (report) {
      statsReport = report;
      statsState = "ready";
    } else {
      statsReport = null;
      statsState = "failed";
    }
  }

  function toggleStats(): void {
    statsVisible = !statsVisible;
    if (statsVisible) void loadStats();
  }

  function statsTotal(rows: AgentStatsRow[], field: "total_seconds" | "error_count" | "done_count"): number {
    return rows.reduce((total, row) => total + row[field], 0);
  }

  function formatDuration(seconds: number): string {
    const minutes = Math.floor(seconds / 60);
    return minutes < 60 ? `${minutes} 分钟` : `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
  }

  function canRestart(row: OverviewRow): boolean {
    return row.agent.can_restart === true &&
      (row.session.lifecycle === "Historical" || row.agent.state.process === "stopped");
  }

  function sendOutputText(send: SendState): string {
    if (send.lines.length > 0) return send.lines.join("\n");
    if (send.state === "failed") return send.code ?? "send_failed";
    if (send.state === "done") return "已完成（无输出）";
    return "处理中…";
  }

  function navigateRows(event: KeyboardEvent, index: number): void {
    const keys = ["ArrowDown", "ArrowUp", "Home", "End"];
    if (!keys.includes(event.key) || rows.length === 0) return;
    event.preventDefault();
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? rows.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
    selectRow(rows[nextIndex]);
    const currentTarget = event.currentTarget as HTMLButtonElement;
    const options = currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
      '[data-testid="overview-row"]',
    );
    queueMicrotask(() => options?.[nextIndex]?.focus());
  }

  onMount(() => {
    const unsubscribe = store.subscribe((next) => {
      viewState = next;
      if (next.snapshot && !next.selectedSession) queueMicrotask(selectFirstAvailable);
    });
    void refresh();
    const timer = pollIntervalMs > 0
      ? window.setInterval(() => void refresh(), pollIntervalMs)
      : undefined;
    return () => {
      disposed = true;
      unsubscribe();
      if (timer !== undefined) window.clearInterval(timer);
      if (stopConfirmationTimer !== undefined) window.clearTimeout(stopConfirmationTimer);
      for (const timer of sendTimers.values()) window.clearTimeout(timer);
      sendTimers.clear();
    };
  });
</script>

<div class="overview" data-testid="overview-app" data-theme-style="graphite">
  <header>
    <div><span class="eyebrow">AGENT ISLAND</span><h1>会话总览</h1></div>
    <div class="header-actions">
      <span class="summary">{rows.length} 个会话</span>
      {#if updatedAt}<time>{new Date(updatedAt).toLocaleTimeString()}</time>{/if}
      <button data-testid="overview-stats-toggle" type="button" class:active={statsVisible} onclick={toggleStats}>统计</button>
    </div>
  </header>

  {#if viewState.refreshError && rows.length > 0}
    <p class="refresh-warning" role="status">刷新暂时失败，正在显示上一次确认的数据。</p>
  {/if}

  {#if statsVisible}
    <section class="stats" aria-label="统计报表">
      <div class="stats-toolbar">
        <h2>本地使用统计</h2>
        <button data-testid="overview-stats-days" type="button" disabled={statsState === "pending"} onclick={() => { statsDays = statsDays === 7 ? 1 : 7; void loadStats(); }}>{statsDays === 7 ? "最近 7 天" : "今天"}</button>
      </div>
      {#if statsState === "pending"}<p class="notice">正在读取统计…</p>
      {:else if statsState === "failed"}<p class="notice error" role="alert">统计暂不可用</p>
      {:else if statsReport}
        {#each statsReport.days as day (day.date)}
          <article class="stat-day"><h3>{day.date}</h3>
            {#each day.agents as agent (agent.name)}<div class="stat-row"><strong>{agent.name}</strong><span>{formatDuration(agent.total_seconds)}</span><span>报错 {agent.error_count}</span><span>完成 {agent.done_count}</span></div>{/each}
            <div class="stat-row total"><strong>合计</strong><span>{formatDuration(statsTotal(day.agents, "total_seconds"))}</span><span>报错 {statsTotal(day.agents, "error_count")}</span><span>完成 {statsTotal(day.agents, "done_count")}</span></div>
          </article>
        {/each}
      {/if}
    </section>
  {:else}
    <main>
      <div class="session-list" aria-label="全部会话" role="listbox">
        {#if rows.length === 0}
          <p class="empty">{viewState.refreshError ? "会话刷新失败" : "暂无会话"}</p>
        {:else}
          {#each rows as row (row.key)}
            <button
              type="button"
              role="option"
              class="session-row"
              class:selected={row.key === selectedKey}
              data-testid="overview-row"
              data-agent-id={row.agent.id}
              data-session-id={row.session.id}
              data-lifecycle={row.session.lifecycle}
              aria-selected={row.key === selectedKey}
              onclick={() => selectRow(row)}
              onkeydown={(event) => navigateRows(event, rows.indexOf(row))}
            >
              <span class={`dot status-${row.session.display_status}`} aria-hidden="true"></span>
              <span class="row-copy"><strong>{row.agent.name} · {row.session.name}</strong><small>{row.session.cwd ?? "无目录"}</small></span>
              <span class="row-state">{row.session.lifecycle === "Active" ? "当前" : "历史"} · {statusText[row.session.display_status]}</span>
            </button>
          {/each}
        {/if}
      </div>

      <section class="detail" aria-label="会话详情">
        {#if selectedRow}
          <span class="sr-only" data-testid="overview-selected-agent-id">{selectedRow.agent.id}</span>
          <span class="sr-only" data-testid="overview-selected-session-id">{selectedRow.session.id}</span>
          <div class="detail-heading"><div><span class="eyebrow">{selectedRow.session.lifecycle === "Active" ? "ACTIVE SESSION" : "HISTORY"}</span><h2>{selectedRow.agent.name} · {selectedRow.session.name}</h2></div><strong class={`status status-${selectedRow.session.display_status}`} data-testid="overview-status">{statusText[selectedRow.session.display_status]}</strong></div>
          <p class="path" data-testid="overview-path">{selectedRow.session.cwd ?? "无目录"}</p>
          <div class="detail-actions">
            <button data-testid="overview-open-terminal" type="button" disabled={!selectedRow.session.cwd || selectedAction?.state === "pending"} onclick={() => void runAction(selectedRow.key, { name: "open_session_terminal", agentName: selectedRow.agent.name, sessionId: selectedRow.session.id })}>打开终端</button>
            <button data-testid="overview-restart" type="button" disabled={!canRestart(selectedRow) || selectedAction?.state === "pending"} onclick={() => void runAction(selectedRow.key, { name: "restart_session", agentName: selectedRow.agent.name, sessionId: selectedRow.session.id })}>恢复</button>
            <button data-testid="overview-stop" class="danger" type="button" disabled={selectedRow.session.lifecycle !== "Active" || selectedRow.session.display_status === "stopped" || selectedAction?.state === "pending"} onclick={stopSelected}>{stopConfirmationKey === selectedRow.key ? "确认停止" : "停止"}</button>
            {#if selectedAction?.state === "failed"}<span class="action-error" role="alert">{selectedAction.code}</span>{/if}
          </div>
          <form class="composer" onsubmit={(event) => { event.preventDefault(); void sendPrompt(); }}>
            <input data-testid="overview-prompt" aria-label="给当前会话发消息" value={drafts[selectedRow.key] ?? ""} oninput={(event) => updateDraft(selectedRow.key, event.currentTarget.value)} placeholder="给这个会话发消息…" />
            <button data-testid="overview-send" type="submit" disabled={!(drafts[selectedRow.key] ?? "").trim() || sends[selectedRow.key]?.state === "pending" || sends[selectedRow.key]?.state === "streaming"}>发送</button>
          </form>
          <div class="log" data-testid="overview-log" aria-label="结构化会话记录">
            {#if selectedRow.session.records.length === 0}<p class="empty">暂无结构化记录</p>{/if}
            {#each selectedRow.session.records as record (record.event_id)}<article class={`message role-${record.role.toLowerCase()}`}><span>{record.role}</span><p>{record.text}</p></article>{/each}
            {#if sends[selectedRow.key]}
              <article class="message send-output"><span>Agent</span><p>{sendOutputText(sends[selectedRow.key])}</p></article>
            {/if}
          </div>
        {:else}
          <p class="empty">选择一个会话查看详情</p>
        {/if}
      </section>
    </main>
  {/if}
</div>

<style>
  :global(html), :global(body), :global(#overview-root) { height: 100%; }
  :global(body) { margin: 0; }
  :global(*) { box-sizing: border-box; }
  .overview { --bg:var(--surface-island); --surface:var(--surface-card); --raised:var(--surface-raised); --border:var(--border-subtle); --text:var(--text-primary); --muted:var(--text-secondary); --accent:var(--accent-primary); --green:var(--status-done); --yellow:var(--status-waiting); --red:var(--status-error); --error-text:var(--refresh-error-text); --error-surface:var(--refresh-error-surface); display:flex; flex-direction:column; height:100%; overflow:hidden; color:var(--text); background:var(--island-background); font:500 13px/1.45 Inter,system-ui,"Segoe UI",sans-serif; }
  header { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:15px 18px 12px; border-bottom:1px solid var(--border); }
  h1,h2,h3,p { margin:0; } h1 { font-size:16px; } h2 { font-size:14px; } h3 { padding:9px 11px; font-size:12px; border-bottom:1px solid var(--border); }
  .eyebrow { display:block; color:var(--muted); font-size:9px; letter-spacing:.14em; }
  .header-actions,.detail-actions,.composer,.stats-toolbar { display:flex; align-items:center; gap:8px; }
  .header-actions { justify-content:flex-end; } .summary,time { color:var(--muted); font-size:10px; }
  button,input { min-height:34px; border:1px solid var(--border); border-radius:var(--radius-control); color:var(--text); background:var(--control-surface); font:inherit; }
  button { padding:6px 11px; cursor:pointer; } button:hover:not(:disabled),button.active { background:var(--control-hover); border-color:color-mix(in srgb,var(--accent) 40%,transparent); } button:disabled { opacity:.4; cursor:default; } button:focus-visible,input:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  main { min-height:0; flex:1; display:grid; grid-template-columns:minmax(210px,.8fr) minmax(0,1.4fr); }
  .session-list { min-height:0; overflow:auto; padding:10px; border-right:1px solid var(--border); }
  .session-row { width:100%; display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:9px; margin-bottom:5px; padding:10px; text-align:left; background:transparent; border-color:transparent; }
  .session-row.selected { background:var(--raised); border-color:color-mix(in srgb,var(--accent) 34%,transparent); box-shadow:inset 3px 0 var(--accent); }
  .dot { width:8px; height:8px; border-radius:50%; background:var(--muted); } .dot.status-working,.dot.status-done { background:var(--green); } .dot.status-error,.dot.status-stopped { background:var(--red); } .dot.status-waiting { background:var(--yellow); }
  .row-copy { min-width:0; display:grid; gap:2px; } .row-copy strong,.row-copy small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .row-copy small,.row-state { color:var(--muted); font-size:10px; } .row-state { text-align:right; }
  .detail { min-width:0; min-height:0; display:flex; flex-direction:column; gap:9px; padding:13px 15px; }
  .detail-heading { display:flex; align-items:flex-start; justify-content:space-between; gap:12px; } .status { flex:0 0 auto; font-size:11px; color:var(--muted); } .status-working,.status-done { color:var(--green); } .status-error,.status-stopped { color:var(--red); } .status-waiting { color:var(--yellow); }
  .path { overflow:hidden; color:var(--muted); font-size:11px; text-overflow:ellipsis; white-space:nowrap; }
  .detail-actions { flex-wrap:wrap; } .danger { color:var(--error-text); } .action-error { color:var(--error-text); font-size:10px; }
  .composer input { min-width:0; flex:1; padding:7px 10px; }
  .log { min-height:0; flex:1; overflow:auto; display:flex; flex-direction:column; gap:7px; padding:9px; border:1px solid var(--border); border-radius:12px; background:var(--surface-log); overscroll-behavior:contain; }
  .message { max-width:88%; padding:8px 10px; border:1px solid var(--border); border-radius:11px; background:var(--surface); white-space:pre-wrap; overflow-wrap:anywhere; } .message span { display:block; margin-bottom:2px; color:var(--accent); font-size:9px; } .message.role-user { align-self:flex-end; background:color-mix(in srgb,var(--accent) 12%,var(--surface)); } .message.role-user span { text-align:right; } .message p { font-size:12px; } .send-output { opacity:.86; }
  .empty,.notice { padding:20px; color:var(--muted); text-align:center; } .notice.error { color:var(--error-text); }
  .refresh-warning { padding:7px 14px; color:var(--error-text); background:var(--error-surface); font-size:11px; }
  .stats { min-height:0; overflow:auto; flex:1; padding:13px; } .stats-toolbar { justify-content:space-between; margin-bottom:10px; } .stat-day { margin-bottom:10px; overflow:hidden; border:1px solid var(--border); border-radius:12px; background:var(--surface); } .stat-row { display:grid; grid-template-columns:1.4fr 1fr .8fr .8fr; gap:8px; padding:8px 11px; color:var(--muted); font-size:11px; border-bottom:1px solid var(--border); } .stat-row:last-child { border:0; } .stat-row strong,.stat-row.total { color:var(--text); }
  .sr-only { position:absolute; width:1px; height:1px; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; }
  @media (max-width:620px) { main { grid-template-columns:1fr; grid-template-rows:minmax(120px,36%) minmax(0,1fr); } .session-list { border-right:0; border-bottom:1px solid var(--border); } }
  @media (max-width:420px) { header { align-items:flex-start; flex-direction:column; padding-inline:13px; } .header-actions { width:100%; justify-content:flex-start; flex-wrap:wrap; } .detail { padding-inline:11px; } .composer { align-items:stretch; flex-direction:column; } .composer button { width:100%; } .stat-row { grid-template-columns:1fr 1fr; } }
</style>
