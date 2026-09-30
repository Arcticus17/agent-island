<script lang="ts">
  import { onDestroy } from "svelte";
  import type { AgentCommand, AgentView, CommandResult, SessionView } from "../../bridge/types";
  import type { CommandFailure, CommandUiState } from "../events/types";
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
    themeStyle,
    hookEnabled,
    hookState,
    hookError,
    onPrivacy,
    onFocusMode,
    onThemeStyle,
    onHookToggle,
    onSettingsOpen,
  }: {
    agent: AgentView | null;
    session?: SessionView | null;
    inspectingHistory?: boolean;
    runCommand: (command: AgentCommand) => Promise<CommandResult>;
    onLayoutChange: () => void;
    privacy: boolean;
    focusMode: "off" | "errors" | "quiet";
    themeStyle: "graphite" | "pure-black";
    hookEnabled: boolean | null;
    hookState: CommandUiState;
    hookError: CommandFailure | null;
    onPrivacy: (value: boolean) => void;
    onFocusMode: (value: "off" | "errors" | "quiet") => void;
    onThemeStyle: (value: "graphite" | "pure-black") => void;
    onHookToggle: () => void;
    onSettingsOpen: () => void;
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

  function openDirectory(): void {
    const path = session?.cwd;
    if (path) void execute("directory", { name: "open_path", path });
    else unavailable("directory", "尚未确认当前会话的工作目录，不能打开其他历史会话的目录。");
  }

  function openTerminal(): void {
    if (agent && session) {
      void execute("terminal", { name: "open_session_terminal", agentName: agent.name, sessionId: session.id });
    } else unavailable("terminal", "尚未确认当前会话，无法确定终端应打开的位置。请先在总览中确认目标会话。");
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
    <button type="button" data-testid="action-overview" disabled={states.overview === "pending"} onclick={() => void execute("overview", { name: "open_overview" })}>{actionLabel("overview", "总览", "打开中…", "已打开")}</button>
    <button type="button" data-testid="action-directory" disabled={states.directory === "pending"} onclick={openDirectory}>{actionLabel("directory", "目录", "打开中…", "已打开")}</button>
    <button type="button" data-testid="action-terminal" disabled={states.terminal === "pending"} onclick={openTerminal}>{actionLabel("terminal", "终端", "打开中…", "已打开")}</button>
    <button type="button" data-testid="action-focus" title={inspectingHistory ? "本地记录无法定位实时窗口" : agent?.state.process === "running" ? "切回 Agent 所在窗口" : "Agent 未运行，无法跳回"} disabled={inspectingHistory || agent?.state.process !== "running" || states.focus === "pending"} onclick={() => agentCommand("focus", "focus_agent_terminal")}>{actionLabel("focus", "跳回", "跳转中…", "已跳回")}</button>
    <button type="button" data-testid="action-restart" title="使用记录的启动方式重新打开 Agent；不会停止现有任务" disabled={inspectingHistory || agent?.can_restart !== true || states.restart === "pending"} onclick={() => agentCommand("restart", "restart_agent")}>{actionLabel("restart", "重启", "启动中…", "已请求启动")}</button>
    <button class="danger" type="button" data-testid="action-stop" data-action-state={states.stop} disabled={inspectingHistory || !agent || agent.state.process !== "running" || states.stop === "pending"} onclick={stop}>
      {states.stop === "confirming" ? "确认停止" : states.stop === "pending" ? "停止中…" : states.stop === "succeeded" ? "已停止" : "停止"}
    </button>
  </div>
  <SettingsPopover {privacy} {focusMode} {themeStyle} {hookEnabled} {hookState} {hookError} {onPrivacy} {onFocusMode} {onThemeStyle} {onHookToggle} onOpen={onSettingsOpen} {onLayoutChange} />
  </div>
</footer>

<style>
  .action-bar { display: grid; gap: 8px; padding: 10px 12px 12px; border-top: 1px solid var(--island-border-subtle); max-height: calc(100dvh - 66px); overflow-y: auto; box-sizing: border-box; }
  .action-row { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; }
  .actions { min-width: 0; display: flex; flex-wrap: wrap; gap: 6px; }
  button { min-height: 30px; padding: 5px 10px; border: 1px solid var(--island-border-subtle); border-radius: 9px; color: var(--island-text-secondary); background: var(--island-control-surface); cursor: pointer; }
  button:hover:not(:disabled) { color: var(--island-text-primary); background: var(--island-control-hover); }
  button.danger { color: var(--island-status-error); }
  button:disabled { opacity: .5; cursor: not-allowed; }
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
  .feedback { display: flex; justify-content: space-between; gap: 8px; max-height: 120px; overflow: auto; overflow-wrap: anywhere; padding: 8px 10px; border-radius: 10px; color: var(--island-refresh-error-text); background: var(--island-refresh-error-surface); font-size: 11px; }
  .feedback button { color: inherit; }
  @media (max-width: 420px) { .action-bar { align-items: flex-start; } .actions { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); flex: 1; } button { min-height: 38px; padding-inline: 7px; } }
</style>
