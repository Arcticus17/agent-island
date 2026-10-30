<script lang="ts">
  import { onDestroy } from "svelte";
  import type { AgentCommand, AgentView, CommandResult, SessionView } from "../../bridge/types";
  import type { CommandFailure, CommandUiState } from "../events/types";
  import type { Appearance } from "../../themes/theme-style";
  import SettingsPopover from "./SettingsPopover.svelte";

  type ActionKey = "overview" | "directory" | "terminal" | "focus" | "restart" | "stop";

  let {
    agent,
    session = agent?.active_session ?? null,
    inspectingHistory = false,
    runCommand,
    onLayoutChange,
    privacy,
    focusMode,
    appearance,
    iconStyle,
    agentMode,
    hookEnabled,
    hookState,
    hookError,
    onPrivacy,
    onFocusMode,
    onAppearance,
    budget = 0,
    onBudget,
    pinnedAgentName = null,
    onTogglePin,
    onIconStyle,
    onAgentMode,
    onHookToggle,
    onSettingsOpen,
    onAdjustWidth,
    onStartResize,
  }: {
    agent: AgentView | null;
    session?: SessionView | null;
    inspectingHistory?: boolean;
    runCommand: (command: AgentCommand) => Promise<CommandResult>;
    onLayoutChange: () => void;
    privacy: boolean;
    focusMode: "off" | "errors" | "quiet";
    appearance: Appearance;
    iconStyle: "dot" | "pixel";
    agentMode: "all" | "running";
    hookEnabled: boolean | null;
    hookState: CommandUiState;
    hookError: CommandFailure | null;
    onPrivacy: (value: boolean) => void;
    onFocusMode: (value: "off" | "errors" | "quiet") => void;
    onAppearance: (value: Appearance) => void;
    budget?: number;
    onBudget?: (value: number) => void;
    pinnedAgentName?: string | null;
    onTogglePin?: () => void;
    onIconStyle: (value: "dot" | "pixel") => void;
    onAgentMode: (value: "all" | "running") => void;
    onHookToggle: () => void;
    onSettingsOpen: () => void;
    onAdjustWidth?: (delta: number) => void;
    onStartResize?: () => void;
  } = $props();

  let states = $state<Record<ActionKey, CommandUiState>>({ overview: "idle", directory: "idle", terminal: "idle", focus: "idle", restart: "idle", stop: "idle" });
  let failure = $state<CommandFailure | null>(null);
  let failedAction = $state<{ key: ActionKey; command: AgentCommand } | null>(null);
  let confirmTimer: number | undefined;

  function normalizedFailure(error: unknown): CommandFailure {
    return { code: "command_failed", message: error instanceof Error ? error.message : String(error), retryable: false };
  }

  async function execute(key: ActionKey, command: AgentCommand): Promise<void> {
    if (states[key] === "pending") return;
    if (key === "stop" && confirmTimer !== undefined) window.clearTimeout(confirmTimer);
    states[key] = "pending";
    failure = null;
    failedAction = null;
    onLayoutChange();
    let result: CommandResult;
    try {
      result = await runCommand(command);
    } catch (error) {
      result = { ok: false, ...normalizedFailure(error) };
    }
    if (result.ok) {
      states[key] = "succeeded";
    } else {
      states[key] = "failed";
      failure = result;
      failedAction = { key, command };
    }
    onLayoutChange();
  }

  function stop(): void {
    if (inspectingHistory || !agent || agent.state.process !== "running" || states.stop === "pending") return;
    if (states.stop !== "confirming") {
      states.stop = "confirming";
      failure = null;
      if (confirmTimer !== undefined) window.clearTimeout(confirmTimer);
      confirmTimer = window.setTimeout(() => {
        if (states.stop === "confirming") states.stop = "idle";
      }, 3_000);
      return;
    }
    const command: AgentCommand = { name: "stop_agent", agentName: agent.name };
    void execute("stop", command);
  }

  function agentCommand(key: "focus" | "restart", name: "focus_agent_terminal" | "restart_agent"): void {
    if (!agent || inspectingHistory) return;
    void execute(key, { name, agentName: agent.name });
  }

  /**
   * "为什么现在不能打开"——把原因说清楚，而不是等用户点了再弹一句笼统的错误。
   * 三种情况完全不同：会话没确认 / 会话没有 cwd / 选中的历史记录已不在快照里。
   */
  const directoryReason = $derived(
    session
      ? session.cwd
        ? null
        : "这条会话没有记录工作目录（日志里缺少 cwd）"
      : inspectingHistory
        ? "所选历史记录已不在当前快照中，请重新选择"
        : "当前会话未确认：先在「查看会话」里选一条记录",
  );
  const terminalReason = $derived(
    agent && session ? null : inspectingHistory ? "所选历史记录已不可用" : "当前会话未确认，无法确定终端位置",
  );

  function openDirectory(): void {
    const path = session?.cwd;
    if (path) void execute("directory", { name: "open_path", path });
    else unavailable("directory", directoryReason ?? "尚未确认当前会话的工作目录");
  }

  function openTerminal(): void {
    if (agent && session) {
      void execute("terminal", { name: "open_session_terminal", agentName: agent.name, sessionId: session.id });
    } else unavailable("terminal", terminalReason ?? "尚未确认当前会话");
  }

  function unavailable(key: ActionKey, message: string): void {
    states[key] = "failed";
    failedAction = null;
    failure = { code: "session_unavailable", message, retryable: false };
    onLayoutChange();
  }

  function actionLabel(key: ActionKey, idle: string, pending: string, succeeded: string): string {
    return states[key] === "pending" ? pending : states[key] === "succeeded" ? succeeded : states[key] === "failed" ? `${idle}失败` : idle;
  }

  function retryFailedAction(): void {
    const action = failedAction;
    if (!action) return;
    // `$state` recursively proxies stored objects. Recreate the command so the
    // bridge receives a plain serializable payload on retry.
    void execute(action.key, { ...action.command } as AgentCommand);
  }

  onDestroy(() => {
    if (confirmTimer !== undefined) window.clearTimeout(confirmTimer);
  });
</script>

<footer class="action-bar" aria-label="Agent 操作">
  {#if failure}
    <div class="feedback" data-testid="action-error" role="alert"><span class="sensitive"><strong>{failure.code}</strong> · {failure.message}</span>{#if failure.retryable && failedAction}<button type="button" data-testid="action-retry" onclick={retryFailedAction}>重试</button>{/if}</div>
  {/if}
  <div class="action-row">
    <div class="actions">
      <button type="button" class="primary" data-testid="action-overview" disabled={states.overview === "pending"} onclick={() => void execute("overview", { name: "open_overview" })}>{actionLabel("overview", "总览", "打开中…", "已打开")}</button>
      <button type="button" data-testid="action-directory" title={directoryReason ?? "打开会话工作目录"} disabled={states.directory === "pending"} onclick={openDirectory}>{actionLabel("directory", "目录", "打开中…", "已打开")}</button>
      <button type="button" data-testid="action-terminal" title={terminalReason ?? "在这个会话的目录里打开终端"} disabled={states.terminal === "pending"} onclick={openTerminal}>{actionLabel("terminal", "终端", "打开中…", "已打开")}</button>
      <button type="button" data-testid="action-focus" title={inspectingHistory ? "本地记录无法定位实时窗口" : agent?.state.process === "running" ? "切回 Agent 所在窗口" : "Agent 未运行，无法跳回"} disabled={inspectingHistory || agent?.state.process !== "running" || states.focus === "pending"} onclick={() => agentCommand("focus", "focus_agent_terminal")}>{actionLabel("focus", "跳回", "跳转中…", "已跳回")}</button>
    </div>
    <div class="destructive">
      <button type="button" data-testid="action-restart" title="使用记录的启动方式重新打开 Agent；不会停止现有任务" disabled={inspectingHistory || agent?.can_restart !== true || states.restart === "pending"} onclick={() => agentCommand("restart", "restart_agent")}>{actionLabel("restart", "重启", "启动中…", "已请求启动")}</button>
      <button class="danger" class:confirming={states.stop === "confirming"} type="button" data-testid="action-stop" data-action-state={states.stop} disabled={inspectingHistory || !agent || agent.state.process !== "running" || states.stop === "pending"} onclick={stop}>
        {states.stop === "confirming" ? "确认停止" : states.stop === "pending" ? "停止中…" : states.stop === "succeeded" ? "已停止" : "停止"}
      </button>
    </div>
    <SettingsPopover {privacy} {focusMode} {appearance} {iconStyle} {agentMode} {hookEnabled} {hookState} {hookError} {onPrivacy} {onFocusMode} {onAppearance} {onIconStyle} {onAgentMode} {onHookToggle} {budget} {onBudget} {pinnedAgentName} {onTogglePin} onOpen={onSettingsOpen} {onLayoutChange} {onAdjustWidth} {onStartResize} />
  </div>
</footer>

<style>
  .action-bar {
    display: grid;
    gap: 8px;
    padding: 9px 14px 11px;
    border-top: 1px solid var(--rule);
    background: var(--sticky);
    box-sizing: border-box;
    position: sticky;
    bottom: 0;
    z-index: var(--z-sticky);
  }
  .action-row { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 6px; min-width: 0; }
  .actions, .destructive { min-width: 0; display: flex; align-items: center; gap: 2px; }
  .actions { overflow-x: auto; scrollbar-width: none; }
  .actions::-webkit-scrollbar { display: none; }
  .destructive { flex: 0 0 auto; margin-left: auto; }

  button {
    flex: 0 0 auto;
    min-height: 28px;
    padding: 4px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    color: var(--ink-2);
    background: transparent;
    font: 600 var(--fs-xs)/1 var(--font-ui);
    cursor: pointer;
    transition: background var(--duration-fast) ease-out, color var(--duration-fast) ease-out;
  }
  button:hover:not(:disabled) { color: var(--ink); background: var(--field); }
  button:active:not(:disabled) { transform: translateY(.5px); }
  button:disabled { opacity: .35; cursor: not-allowed; }

  /* 只有"总览"是实心主按钮：它是这排里唯一"打开另一块屏幕"的动作 */
  button.primary { color: var(--ground); background: var(--ink); font-weight: 650; }
  button.primary:hover:not(:disabled) { color: var(--ground); background: var(--ink); opacity: .88; }

  button.danger { color: var(--state-error); }
  button.danger:hover:not(:disabled) { color: var(--state-error); background: color-mix(in srgb, var(--state-error) 12%, transparent); }
  button.danger.confirming { color: var(--danger-ink); background: var(--danger); }
  button.danger.confirming:hover:not(:disabled) { color: var(--danger-ink); background: var(--danger); }

  .feedback {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    max-height: 120px;
    overflow: auto;
    overflow-wrap: anywhere;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    color: var(--alarm-ink);
    background: var(--alarm);
    font-size: var(--fs-2xs);
  }
  .feedback button { color: var(--alarm-ink); border-color: color-mix(in srgb, var(--alarm-ink) 45%, transparent); }
  .feedback button:hover:not(:disabled) { color: var(--alarm-ink); background: color-mix(in srgb, var(--alarm-ink) 12%, transparent); }

  @media (max-width: 420px) {
    .action-bar { padding-inline: 10px; }
    button { min-height: 34px; padding-inline: 8px; }
  }
</style>
