<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import "../themes/graphite-glass.css";
  import CompactIsland from "../components/island/CompactIsland.svelte";
  import ExpandedIsland from "../components/island/ExpandedIsland.svelte";
  import ApprovalStack from "../components/approvals/ApprovalStack.svelte";
  import ActionBar from "../components/controls/ActionBar.svelte";
  import NotificationStack from "../components/notifications/NotificationStack.svelte";
  import type { ApprovalItem, CommandFailure, CommandUiState, NotificationItem } from "../components/events/types";
  import { tauriBridge } from "../bridge/tauri";
  import type { AgentIslandBridge, CommandResult, HookEvent } from "../bridge/types";
  import { createAgentStore, type AgentStoreState } from "../stores/agent-store";

  let {
    bridge = tauriBridge,
    pollIntervalMs = 1_500,
    initiallyExpanded = false,
  }: {
    bridge?: AgentIslandBridge;
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
  let hookEnabled = $state<boolean | null>(null);
  let hookState = $state<CommandUiState>("idle");
  let hookError = $state<CommandFailure | null>(null);
  let listenerError = $state(false);
  let resizeRevision = 0;
  let notificationSequence = 0;
  let notificationAutoExpanded = false;
  const notificationTimers = new Map<string, { handle: number; remainingMs: number; startedAt: number; pauseCount: number }>();
  const collapsedWindowHeight = 60;

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
      void store.refresh();
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
    if (result.ok) void store.refresh();
    void syncWindowSize();
  }

  async function runActionCommand(command: Parameters<AgentIslandBridge["runCommand"]>[0]): Promise<CommandResult> {
    const result = await bridge.runCommand(command);
    if (result.ok && (command.name === "stop_agent" || command.name === "restart_agent")) {
      await store.refresh();
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
    let disposed = false;
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
    void store.refresh();
    const timer = pollIntervalMs > 0
      ? window.setInterval(() => void store.refresh(), pollIntervalMs)
      : undefined;
    return () => {
      disposed = true;
      resizeRevision += 1;
      stopListening?.();
      for (const timer of notificationTimers.values()) window.clearTimeout(timer.handle);
      notificationTimers.clear();
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
      <ExpandedIsland
        agents={viewState.snapshot.agents}
        selectedAgent={viewState.selectedAgent}
        session={viewState.selectedAgent?.active_session ?? null}
        refreshError={viewState.refreshError}
        {privacy}
        onSelectAgent={(id) => store.selectAgent(id)}
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
  <div class="island-loading" aria-live="polite" bind:this={islandElement}>正在连接 Agent Island…</div>
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
    border-radius: 24px;
    background: var(--island-background);
    box-shadow: var(--island-shadow);
    backdrop-filter: blur(22px) saturate(125%);
  }
  .agent-island.expanded { max-height: 100vh; overflow-y: auto; scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track); }
  .event-layer { position: fixed; top: 66px; left: 50%; z-index: 20; width: min(696px, calc(100vw - 48px)); max-height: calc(100dvh - 78px); overflow-y: auto; overscroll-behavior: contain; transform: translateX(-50%); pointer-events: auto; scrollbar-color: var(--island-scrollbar-thumb) transparent; }
  .listener-error { margin: 7px 0 0; padding: 10px 12px; border: 1px solid var(--island-border-subtle); border-radius: 12px; color: var(--island-refresh-error-text); background: var(--island-refresh-error-surface); font-size: 11px; }
  :global(.privacy .sensitive) { filter: blur(5px); user-select: none; pointer-events: none; }
  .island-loading { padding: 12px 18px; border: 1px solid var(--island-border-subtle); border-radius: 999px; background: var(--island-surface); }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  @media (max-width: 420px) { .agent-island, .island-loading { width: calc(100vw - 16px); } .agent-island { border-radius: 20px; } .event-layer { width: calc(100vw - 32px); } }
</style>
