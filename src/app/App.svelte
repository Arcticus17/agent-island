<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import "../themes/tokens.css";
  import "../themes/graphite.css";
  import "../themes/pure-black.css";
  import CompactIsland from "../components/island/CompactIsland.svelte";
  import ApprovalStack from "../components/approvals/ApprovalStack.svelte";
  import LogCard from "../components/cards/LogCard.svelte";
  import SessionCard from "../components/cards/SessionCard.svelte";
  import StatusCard from "../components/cards/StatusCard.svelte";
  import UsageCard from "../components/cards/UsageCard.svelte";
  import CardBoundary from "../components/common/CardBoundary.svelte";
  import ActionBar from "../components/controls/ActionBar.svelte";
  import DiagnosticsPanel from "../components/diagnostics/DiagnosticsPanel.svelte";
  import LayoutEditor from "../components/layout/LayoutEditor.svelte";
  import NotificationStack from "../components/notifications/NotificationStack.svelte";
  import type { ApprovalItem, CommandFailure, CommandUiState, NotificationItem } from "../components/events/types";
  import { tauriBridge } from "../bridge/tauri";
  import { tauriDiagnosticsBridge, type DiagnosticsBridge } from "../bridge/diagnostics";
  import type { AgentIslandBridge, CommandResult, DiagnosticView, HookEvent } from "../bridge/types";
  import { loadLayout, saveLayout } from "../layout/persistence";
  import type { CardPlacement, LayoutConfigV1 } from "../layout/schema";
  import { createAgentStore, type AgentStoreState } from "../stores/agent-store";

  let {
    bridge = tauriBridge,
    diagnosticsBridge = tauriDiagnosticsBridge,
    pollIntervalMs = 1_500,
    initiallyExpanded = false,
  }: {
    bridge?: AgentIslandBridge;
    diagnosticsBridge?: DiagnosticsBridge;
    pollIntervalMs?: number;
    initiallyExpanded?: boolean;
  } = $props();

  const store = createAgentStore(untrack(() => bridge));
  let viewState: AgentStoreState = $state(store.current());
  let expanded = $state(untrack(() => initiallyExpanded));
  let islandElement = $state<HTMLElement>();
  let eventLayerElement = $state<HTMLElement>();
  let approvals = $state<ApprovalItem[]>([]);
  let notifications = $state<NotificationItem[]>([]);
  let privacy = $state(localStorage.getItem("agent-island-privacy") === "1");
  let focusMode = $state<"off" | "errors" | "quiet">(
    localStorage.getItem("agent-island-focus") === "errors"
      ? "errors"
      : localStorage.getItem("agent-island-focus") === "quiet"
        ? "quiet"
        : "off",
  );
  let themeStyle = $state<"graphite" | "pure-black">(
    localStorage.getItem("agent-island-theme-style-v2") === "pure-black"
      ? "pure-black"
      : "graphite",
  );
  let layout = $state<LayoutConfigV1>(loadLayout(localStorage));
  let layoutPersistenceFailed = $state(false);
  let releaseLayoutDrag: (() => void) | null = null;
  let hookEnabled = $state<boolean | null>(null);
  let hookState = $state<CommandUiState>("idle");
  let hookError = $state<CommandFailure | null>(null);
  let listenerError = $state(false);
  let diagnosticIssues = $state<DiagnosticView[]>([]);
  let diagnosticStatus = $state<"loading" | "ready" | "failed">("loading");
  let diagnosticRequest = 0;
  let diagnosticInFlight: Promise<void> | null = null;
  let diagnosticRefreshQueued = false;
  let disposed = false;
  let resizeRevision = 0;
  let notificationSequence = 0;
  let notificationAutoExpanded = false;
  const notificationTimers = new Map<string, { handle: number; remainingMs: number; startedAt: number; pauseCount: number }>();
  const collapsedWindowHeight = 60;
  let selectedAgent = $derived(viewState.selectedAgent);
  let selectedSession = $derived(selectedAgent?.active_session ?? null);

  async function loadDiagnostics(): Promise<void> {
    if (disposed) return;
    if (diagnosticInFlight) {
      diagnosticRefreshQueued = true;
      diagnosticStatus = "loading";
      void syncWindowSize();
      return;
    }
    diagnosticRefreshQueued = false;
    const request = ++diagnosticRequest;
    if (diagnosticStatus !== "ready") diagnosticStatus = "loading";
    const operation = (async () => {
      try {
        const response = await diagnosticsBridge.getDiagnostics();
        if (disposed || request !== diagnosticRequest || diagnosticRefreshQueued) return;
        diagnosticIssues = response.issues;
        diagnosticStatus = "ready";
      } catch {
        if (disposed || request !== diagnosticRequest || diagnosticRefreshQueued) return;
        diagnosticIssues = [];
        diagnosticStatus = "failed";
      }
      void syncWindowSize();
    })();
    diagnosticInFlight = operation;
    try {
      await operation;
    } finally {
      if (diagnosticInFlight === operation) diagnosticInFlight = null;
      if (diagnosticRefreshQueued && !disposed) void loadDiagnostics();
    }
  }

  async function refreshAll(): Promise<void> {
    await store.refresh();
    if (!disposed) void loadDiagnostics();
  }

  async function syncWindowSize(): Promise<void> {
    const revision = ++resizeRevision;
    const targetExpanded = expanded;
    await tick();
    if (revision !== resizeRevision) return;
    const islandHeight = islandElement
      ? Math.max(islandElement.scrollHeight, islandElement.getBoundingClientRect().height)
      : 0;
    const eventHeight = targetExpanded && eventLayerElement
      ? eventLayerElement.scrollHeight + 74
      : 0;
    const renderedHeight = Math.ceil(Math.max(islandHeight, eventHeight));
    const availableHeight = window.screen?.availHeight || renderedHeight;
    const safeExpandedHeight = Math.max(
      collapsedWindowHeight,
      Math.floor(availableHeight * 0.92),
    );
    const height = targetExpanded
      ? Math.min(
          Math.max(collapsedWindowHeight, renderedHeight),
          safeExpandedHeight,
        )
      : collapsedWindowHeight;
    try {
      await bridge.resizeWindow?.({
        width: Math.max(1, Math.round(window.innerWidth)),
        height,
      });
    } catch {
      // A browser preview or closing Tauri window may not support resizing.
    }
  }

  function toggleExpanded(): void {
    notificationAutoExpanded = false;
    expanded = !expanded;
    void syncWindowSize();
  }

  function notificationAgent(event: Extract<HookEvent, { kind: "notification" }>) {
    if (store.current().refreshError) return null;
    const agents = store.current().snapshot?.agents ?? [];
    const eligible = agents.filter((agent) =>
      !agent.freshness.stale && agent.active_session !== null,
    );
    if (event.session) {
      const matches = eligible.filter((agent) => agent.active_session?.id === event.session);
      return matches.length === 1 && matches[0].id === "claude" ? matches[0] : null;
    }
    const claude = eligible.filter((agent) => agent.id === "claude");
    return claude.length === 1 ? claude[0] : null;
  }

  function pushNotification(event: Extract<HookEvent, { kind: "notification" }>): void {
    if (focusMode === "quiet" || focusMode === "errors") return;
    const agent = notificationAgent(event);
    if (!agent?.active_session) return;
    const now = Date.now();
    const duplicate = notifications.find((item) =>
      item.sessionId === agent.active_session?.id && item.message === event.message && now - item.receivedAt <= 20_000,
    );
    if (duplicate) {
      notifications = notifications.map((item) => item.id === duplicate.id
        ? { ...item, receivedAt: now, count: item.count + 1 }
        : item);
      scheduleNotificationDismiss(duplicate.id, 6_500);
      void syncWindowSize();
      return;
    }
    const id = `notification-${++notificationSequence}`;
    const retained = notifications.slice(-2);
    for (const item of notifications) {
      if (!retained.some((candidate) => candidate.id === item.id)) clearNotificationTimer(item.id);
    }
    notifications = [
      ...retained,
      { id, agentName: agent.name, sessionId: agent.active_session.id, message: event.message, receivedAt: now, count: 1 },
    ];
    scheduleNotificationDismiss(id, 6_500);
    notificationAutoExpanded ||= !expanded;
    expanded = true;
    void syncWindowSize();
  }

  function clearNotificationTimer(id: string): void {
    const timer = notificationTimers.get(id);
    if (timer) window.clearTimeout(timer.handle);
    notificationTimers.delete(id);
  }

  function scheduleNotificationDismiss(id: string, delayMs: number): void {
    clearNotificationTimer(id);
    const startedAt = Date.now();
    const handle = window.setTimeout(() => dismissNotification(id), delayMs);
    notificationTimers.set(id, { handle, remainingMs: delayMs, startedAt, pauseCount: 0 });
  }

  function dismissNotification(id: string): void {
    clearNotificationTimer(id);
    notifications = notifications.filter((item) => item.id !== id);
    if (notifications.length === 0 && approvals.length === 0 && notificationAutoExpanded) {
      expanded = false;
      notificationAutoExpanded = false;
    }
    void syncWindowSize();
  }

  function pauseNotification(id: string, paused: boolean): void {
    const timer = notificationTimers.get(id);
    if (!timer) return;
    if (paused) {
      if (timer.pauseCount === 0) {
        window.clearTimeout(timer.handle);
        timer.remainingMs = Math.max(0, timer.remainingMs - (Date.now() - timer.startedAt));
        timer.handle = 0;
      }
      timer.pauseCount += 1;
    } else {
      timer.pauseCount = Math.max(0, timer.pauseCount - 1);
      if (timer.pauseCount === 0 && timer.handle === 0) {
        scheduleNotificationDismiss(id, Math.max(250, timer.remainingMs));
      }
    }
  }

  function pushApproval(event: Extract<HookEvent, { kind: "approval" }>): void {
    if (approvals.some((item) => item.approval.id === event.approval.id)) return;
    approvals = [...approvals, { approval: event.approval, state: "idle", decision: null, error: null } satisfies ApprovalItem];
    notificationAutoExpanded = false;
    expanded = true;
    void syncWindowSize();
  }

  function handleHookEvent(event: HookEvent): void {
    if (event.kind === "stop") {
      void refreshAll();
    } else if (event.kind === "notification") {
      pushNotification(event);
    } else {
      pushApproval(event);
    }
  }

  async function respondToApproval(id: string, allow: boolean): Promise<void> {
    const current = approvals.find((item) => item.approval.id === id);
    if (!current || current.state === "pending") return;
    approvals = approvals.map((item) => item.approval.id === id
      ? { ...item, state: "pending", decision: allow, error: null }
      : item);
    const release = store.beginInteraction("approval");
    let result: CommandResult;
    try {
      result = await bridge.runCommand({ name: "respond_hook_approval", id, allow });
    } catch (error) {
      result = {
        ok: false,
        code: "command_failed",
        message: error instanceof Error ? error.message : String(error),
        retryable: false,
      };
    }
    if (result.ok) {
      approvals = approvals.filter((item) => item.approval.id !== id);
    } else {
      approvals = approvals.map((item) => item.approval.id === id
        ? { ...item, state: "failed", error: result }
        : item);
    }
    release();
    if (result.ok) void refreshAll();
    void syncWindowSize();
  }

  async function runActionCommand(command: Parameters<AgentIslandBridge["runCommand"]>[0]): Promise<CommandResult> {
    const result = await bridge.runCommand(command);
    if (result.ok && (command.name === "stop_agent" || command.name === "restart_agent")) {
      await store.refresh();
      if (!disposed) void loadDiagnostics();
    }
    return result;
  }

  function setPrivacy(value: boolean): void {
    privacy = value;
    localStorage.setItem("agent-island-privacy", value ? "1" : "0");
  }

  function setFocusMode(value: "off" | "errors" | "quiet"): void {
    focusMode = value;
    localStorage.setItem("agent-island-focus", value);
    if (value === "errors") {
      const errorAgent = store.current().snapshot?.agents.find((agent) =>
        agent.display_status === "error" && !agent.freshness.stale,
      );
      if (errorAgent) store.selectAgent(errorAgent.id);
    }
  }

  function setThemeStyle(value: "graphite" | "pure-black"): void {
    themeStyle = value;
    localStorage.setItem("agent-island-theme-style-v2", value);
    void syncWindowSize();
  }

  function beginLayoutInteraction(): void {
    if (releaseLayoutDrag) return;
    releaseLayoutDrag = store.beginInteraction("drag");
  }

  function endLayoutInteraction(): void {
    const release = releaseLayoutDrag;
    releaseLayoutDrag = null;
    release?.();
  }

  function persistLayout(next: LayoutConfigV1): void {
    layout = next;
    const result = saveLayout(localStorage, next);
    layoutPersistenceFailed = !result.ok;
    if (result.ok) layout = result.value;
    void syncWindowSize();
  }

  async function safeRunCommand(command: Parameters<AgentIslandBridge["runCommand"]>[0]): Promise<CommandResult> {
    try {
      return await bridge.runCommand(command);
    } catch (error) {
      return {
        ok: false,
        code: "command_failed",
        message: error instanceof Error ? error.message : String(error),
        retryable: true,
      };
    }
  }

  async function loadHookStatus(): Promise<void> {
    if (hookState === "pending" || hookEnabled !== null) return;
    hookState = "pending";
    hookError = null;
    const result = await safeRunCommand({ name: "get_hook_status" });
    if (result.ok && typeof result.value === "boolean") {
      hookEnabled = result.value;
      hookState = "succeeded";
    } else {
      hookState = "failed";
      hookError = result.ok
        ? { code: "invalid_hook_status", message: "事件状态返回格式无效", retryable: true }
        : result;
    }
    void syncWindowSize();
  }

  async function toggleHook(): Promise<void> {
    if (hookState === "pending") return;
    if (hookEnabled === null) {
      await loadHookStatus();
      return;
    }
    const next = !hookEnabled;
    hookState = "pending";
    hookError = null;
    const result = await safeRunCommand({ name: "set_hook_enabled", enabled: next });
    if (result.ok) {
      hookEnabled = next;
      hookState = "succeeded";
    } else {
      hookState = "failed";
      hookError = result;
    }
    void syncWindowSize();
  }

  onMount(() => {
    let stopListening: (() => void) | undefined;
    const unsubscribe = store.subscribe((next) => {
      if (focusMode === "errors") {
        const errorAgent = next.snapshot?.agents.find((agent) =>
          agent.display_status === "error" && !agent.freshness.stale,
        );
        if (errorAgent && errorAgent.id !== next.selectedAgentId) {
          store.selectAgent(errorAgent.id);
          return;
        }
      }
      viewState = next;
      void syncWindowSize();
    });
    void bridge.listenHookEvents(handleHookEvent).then((unlisten) => {
      if (disposed) unlisten();
      else stopListening = unlisten;
    }).catch(() => {
      if (disposed) return;
      listenerError = true;
      expanded = true;
      void syncWindowSize();
    });
    void refreshAll();
    const timer = pollIntervalMs > 0
      ? window.setInterval(() => void refreshAll(), pollIntervalMs)
      : undefined;
    return () => {
      disposed = true;
      diagnosticRequest += 1;
      diagnosticRefreshQueued = false;
      resizeRevision += 1;
      stopListening?.();
      for (const timer of notificationTimers.values()) window.clearTimeout(timer.handle);
      notificationTimers.clear();
      endLayoutInteraction();
      unsubscribe();
      if (timer !== undefined) window.clearInterval(timer);
    };
  });
</script>

{#if viewState.snapshot}
  <main
    class={`agent-island status-${viewState.selectedAgent?.display_status ?? "idle"}`}
    class:expanded
    class:privacy
    data-testid="agent-island"
    data-theme-style={themeStyle}
    aria-label="Agent Island"
    bind:this={islandElement}
  >
    <span class="sr-only" data-testid="selected-agent-id">{viewState.selectedAgentId ?? ""}</span>
    <CompactIsland agent={viewState.selectedAgent} {expanded} onToggle={toggleExpanded} />
    {#if expanded}
      <section id="expanded-island" class="adaptive-details" aria-label="Agent 详情">
        <nav class="agent-strip" aria-label="切换 Agent">
          {#each viewState.snapshot.agents as agent (agent.id)}
            <button
              type="button"
              class:active={agent.id === selectedAgent?.id}
              data-agent-id={agent.id}
              aria-pressed={agent.id === selectedAgent?.id}
              onclick={() => store.selectAgent(agent.id)}
            ><span class={`mini-dot status-${agent.display_status}`} aria-hidden="true"></span><span class="sensitive">{agent.name}</span></button>
          {/each}
        </nav>
        {#if viewState.refreshError}<p class="refresh-note" role="status">数据刷新暂时失败，已保留最近状态。</p>{/if}
        {#if selectedAgent && selectedSession}
          <LayoutEditor
            bind:layout
            onLayoutChange={persistLayout}
            beginInteraction={beginLayoutInteraction}
            endInteraction={endLayoutInteraction}
          >
            {#snippet content(placement: CardPlacement)}
              {#if placement.id === "status"}
                <CardBoundary cardName="状态卡片" errorCode="status_card_render_failed">
                  <StatusCard agent={selectedAgent} session={selectedSession} />
                </CardBoundary>
              {:else if placement.id === "usage"}
                <CardBoundary cardName="用量卡片" errorCode="usage_card_render_failed">
                  <UsageCard agent={selectedAgent} refreshFailed={viewState.refreshError !== null} />
                </CardBoundary>
              {:else if placement.id === "session"}
                <CardBoundary cardName="会话卡片" errorCode="session_card_render_failed">
                  <SessionCard session={selectedSession} {privacy} />
                </CardBoundary>
              {:else if placement.id === "log"}
                <CardBoundary cardName="日志卡片" errorCode="log_card_render_failed">
                  <LogCard session={selectedSession} />
                </CardBoundary>
              {:else}
                <article class="stats-card" aria-label="当前 Agent 统计">
                  <span class="stats-eyebrow">当前快照统计</span>
                  <div class="stats-values">
                    <span><strong data-testid="stats-event-count">{selectedAgent.diagnostic?.event_count ?? "—"}</strong>事件</span>
                    <span><strong data-testid="stats-message-count">{selectedAgent.diagnostic?.message_count ?? "—"}</strong>消息</span>
                    <span><strong>{selectedAgent.history_sessions.length + 1}</strong>会话</span>
                  </div>
                </article>
              {/if}
            {/snippet}
          </LayoutEditor>
          {#if layoutPersistenceFailed}
            <p class="layout-persistence-status" data-testid="layout-persistence-status" role="status">布局已应用，但暂时未保存；下次调整时会重试。</p>
          {/if}
        {:else}
          <div class="empty" data-testid="empty-session" role="status"><strong>无法确认当前会话</strong><span>为避免串入历史对话，灵动岛不会显示其他会话内容。</span></div>
        {/if}
      </section>
      <DiagnosticsPanel
        issues={diagnosticIssues}
        status={diagnosticStatus}
        exportDiagnostics={(destination) => diagnosticsBridge.exportDiagnostics(destination)}
        onLayoutChange={() => void syncWindowSize()}
      />
      {#key viewState.selectedAgent?.id}
        <ActionBar
          agent={viewState.selectedAgent}
          runCommand={runActionCommand}
          onLayoutChange={() => void syncWindowSize()}
          {privacy}
          {focusMode}
          {themeStyle}
          {hookEnabled}
          {hookState}
          {hookError}
          onPrivacy={setPrivacy}
          onFocusMode={setFocusMode}
          onThemeStyle={setThemeStyle}
          onHookToggle={() => void toggleHook()}
          onSettingsOpen={() => void loadHookStatus()}
        />
      {/key}
    {/if}
  </main>
  {#if expanded}
    <aside
      class="event-layer"
      class:privacy
      data-testid="event-layer"
      data-theme-style={themeStyle}
      aria-label="紧急事件"
      bind:this={eventLayerElement}
    >
      <ApprovalStack items={approvals} onRespond={(id, allow) => void respondToApproval(id, allow)} />
      {#if listenerError}<p class="listener-error" role="alert">事件监听连接失败，请重启灵动岛后重试。</p>{/if}
      <NotificationStack items={notifications} onDismiss={dismissNotification} onPause={pauseNotification} />
    </aside>
  {/if}
{:else}
  <div class="island-loading" data-theme-style={themeStyle} aria-live="polite" bind:this={islandElement}>正在连接 Agent Island…</div>
{/if}

<style>
  .agent-island, .island-loading {
    box-sizing: border-box;
    width: min(720px, calc(100vw - 24px));
    color: var(--island-text-primary);
    font: 500 14px/1.45 Inter, ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  .agent-island {
    overflow: hidden;
    border: 1px solid var(--island-border-subtle);
    border-radius: var(--radius-island);
    background: var(--island-background);
    box-shadow: var(--island-shadow);
    backdrop-filter: var(--effect-backdrop);
  }
  .agent-island.expanded { max-height: 100vh; overflow-y: auto; scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track); }
  :global(.agent-island.expanded > .compact) { position: sticky; top: 0; z-index: var(--z-sticky); background: var(--surface-sticky); backdrop-filter: var(--effect-backdrop); }
  :global(.agent-island.expanded > .action-bar) { position: sticky; bottom: 0; z-index: var(--z-sticky); background: var(--surface-sticky); backdrop-filter: var(--effect-backdrop); }
  .adaptive-details { min-width: 0; border-top: 1px solid var(--island-border-subtle); padding: 12px; }
  .agent-strip { display: flex; gap: 7px; padding: 0 0 11px; overflow-x: auto; scrollbar-width: none; }
  .agent-strip button { flex: 0 0 auto; display: inline-flex; align-items: center; gap: 7px; padding: 7px 10px; border: 1px solid transparent; border-radius: 999px; color: var(--island-text-secondary); background: transparent; cursor: pointer; }
  .agent-strip button.active { color: var(--island-text-primary); border-color: var(--island-border-subtle); background: var(--island-surface-raised); }
  .agent-strip button:focus-visible { outline: 2px solid var(--island-accent-primary); }
  .mini-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--island-status-idle); }
  .mini-dot.status-working { background: var(--island-status-working); }
  .mini-dot.status-error { background: var(--island-status-error); }
  .mini-dot.status-done { background: var(--island-status-done); }
  .mini-dot.status-waiting { background: var(--island-status-waiting); }
  .mini-dot.status-stopped { background: var(--island-status-stopped); }
  .refresh-note,
  .layout-persistence-status { margin: 0 0 10px; padding: 7px 10px; border-radius: 10px; color: var(--island-refresh-error-text); background: var(--island-refresh-error-surface); font-size: 12px; overflow-wrap: anywhere; }
  .layout-persistence-status { margin: 10px 0 0; }
  .empty { display: grid; gap: 5px; padding: 28px 20px; text-align: center; color: var(--island-text-secondary); }
  .empty strong { color: var(--island-text-primary); }
  .stats-card { min-width: 0; height: 100%; box-sizing: border-box; display: grid; align-content: center; gap: 10px; }
  .stats-eyebrow { color: var(--island-text-secondary); font-size: 10px; letter-spacing: .12em; text-transform: uppercase; }
  .stats-values { min-width: 0; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 7px; }
  .stats-values span { min-width: 0; display: grid; gap: 2px; color: var(--island-text-secondary); font-size: 10px; }
  .stats-values strong { overflow: hidden; color: var(--island-accent-primary); font-size: 18px; text-overflow: ellipsis; }
  :global(.adaptive-details .layout-card .card) { width: 100%; height: 100%; box-sizing: border-box; border: 0; padding: 0; background: transparent; }
  :global(.adaptive-details .layout-card .session-card),
  :global(.adaptive-details .layout-card .log-card) { grid-column: auto; }
  .event-layer { position: fixed; top: 66px; left: 50%; z-index: var(--z-event); width: min(696px, calc(100vw - 48px)); max-height: calc(100dvh - 78px); overflow-y: auto; overscroll-behavior: contain; transform: translateX(-50%); pointer-events: auto; scrollbar-color: var(--island-scrollbar-thumb) transparent; }
  .listener-error { margin: 7px 0 0; padding: 10px 12px; border: 1px solid var(--island-border-subtle); border-radius: 12px; color: var(--island-refresh-error-text); background: var(--island-refresh-error-surface); font-size: 11px; }
  :global(.privacy .sensitive) { filter: blur(5px); user-select: none; pointer-events: none; }
  .island-loading { padding: 12px 18px; border: 1px solid var(--island-border-subtle); border-radius: 999px; background: var(--island-surface); }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  @media (max-width: 420px) { .agent-island, .island-loading { width: calc(100vw - 16px); } .agent-island { border-radius: 20px; } .event-layer { width: calc(100vw - 32px); } .adaptive-details { padding: 10px; } .stats-values { grid-template-columns: minmax(0, 1fr); } }
</style>
