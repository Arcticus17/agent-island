<script lang="ts">
  import { onDestroy } from "svelte";
  import type { AgentCommand, AgentView, CommandResult, SessionView } from "../../bridge/types";
  import type { CommandFailure } from "../events/types";

  let {
    agent,
    session,
    runCommand,
    onLayoutChange,
    disabledReason = null,
    taskCapable = false,
  }: {
    agent: AgentView | null;
    session: SessionView | null;
    runCommand: (command: AgentCommand) => Promise<CommandResult>;
    onLayoutChange?: () => void;
    /** 例如正在查看历史记录 / 隐私遮罩时的说明 */
    disabledReason?: string | null;
    /** 没有会话也能发（岛内任务模式：`codex exec --json`） */
    taskCapable?: boolean;
  } = $props();

  type SendState = "idle" | "pending" | "streaming" | "done" | "failed";

  let draft = $state("");
  let sendState = $state<SendState>("idle");
  let lines = $state<string[]>([]);
  let failure = $state<CommandFailure | null>(null);
  let taskId = $state<string | null>(null);
  let timer: number | undefined;

  const busy = $derived(sendState === "pending" || sendState === "streaming");
  const canSend = $derived(
    Boolean(agent && (session || taskCapable) && draft.trim().length > 0 && !busy && !disabledReason),
  );
  const placeholder = $derived(
    disabledReason
      ?? (session
        ? "给当前会话发一条指令…"
        : taskCapable
          ? "让 Codex 跑一个任务（岛内任务模式，事件来自 --json）…"
          : "当前会话未确认，无法发送指令"),
  );

  function clearTimer(): void {
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timer = undefined;
    }
  }

  function schedulePoll(): void {
    clearTimer();
    timer = window.setTimeout(() => void poll(), 800);
  }

  async function poll(): Promise<void> {
    const current = taskId;
    if (!current) return;
    let result: CommandResult;
    try {
      result = await runCommand({ name: "get_send_output", taskId: current });
    } catch (error) {
      failure = { code: "send_output_failed", message: error instanceof Error ? error.message : String(error), retryable: false };
      sendState = "failed";
      return;
    }
    if (!result.ok) {
      failure = result;
      sendState = "failed";
      return;
    }
    const value = result.value as { done?: unknown; lines?: unknown; failed?: unknown } | undefined;
    if (!value || typeof value.done !== "boolean" || !Array.isArray(value.lines)) {
      failure = { code: "invalid_send_output", message: "发送回执格式无效", retryable: false };
      sendState = "failed";
      return;
    }
    lines = value.lines.filter((line): line is string => typeof line === "string");
    sendState = value.done ? (value.failed === true ? "failed" : "done") : "streaming";
    if (!value.done) schedulePoll();
    else clearTimer();
    onLayoutChange?.();
  }

  async function send(): Promise<void> {
    const prompt = draft.trim();
    if (!prompt || !agent || (!session && !taskCapable) || busy || disabledReason) return;
    clearTimer();
    failure = null;
    lines = [];
    taskId = null;
    sendState = "pending";
    let result: CommandResult;
    try {
      result = session
        ? await runCommand({ name: "send_to_session", agentName: agent.name, sessionId: session.id, prompt })
        : await runCommand({ name: "run_task", agentName: agent.name, prompt });
    } catch (error) {
      failure = { code: "send_failed", message: error instanceof Error ? error.message : String(error), retryable: true };
      sendState = "failed";
      return;
    }
    if (!result.ok) {
      failure = result;
      sendState = "failed";
      return;
    }
    if (typeof result.value !== "string" || result.value.length === 0) {
      failure = { code: "invalid_send_task", message: "发送任务未返回标识", retryable: true };
      sendState = "failed";
      return;
    }
    taskId = result.value;
    draft = "";
    sendState = "streaming";
    onLayoutChange?.();
    schedulePoll();
  }

  function reset(): void {
    clearTimer();
    sendState = "idle";
    lines = [];
    failure = null;
    taskId = null;
    onLayoutChange?.();
  }

  onDestroy(clearTimer);
</script>

<section class="composer" data-testid="prompt-composer" aria-label="给当前 Agent 发送指令">
  <form
    class="row"
    onsubmit={(event) => {
      event.preventDefault();
      void send();
    }}
  >
    <input
      data-testid="prompt-input"
      aria-label="给当前会话发送指令"
      placeholder={placeholder}
      value={draft}
      disabled={Boolean(disabledReason) || busy}
      oninput={(event) => { draft = event.currentTarget.value; }}
    />
    <button data-testid="prompt-send" type="submit" disabled={!canSend}>
      {sendState === "pending" ? "发送中…" : "发送"}
    </button>
  </form>

  {#if failure}
    <p class="error" role="alert"><strong>{failure.code}</strong> · {failure.message}</p>
  {/if}

  {#if lines.length > 0 || busy || sendState === "done"}
    <div class="output" data-testid="prompt-output" aria-live="polite">
      <div class="output-head">
        <span>{sendState === "done" ? "已结束" : sendState === "failed" ? "失败" : "返回中…"}</span>
        {#if !busy}<button type="button" class="ghost" data-testid="prompt-clear" onclick={reset}>清空</button>{/if}
      </div>
      {#if lines.length > 0}
        <pre class="lines sensitive">{lines.slice(-8).join("\n")}</pre>
      {:else}
        <p class="waiting">等待 Agent 输出…</p>
      {/if}
    </div>
  {/if}
</section>

<style>
  /* 指令输入：一条贴在会话纸带下面的命令行，不是又一个表单卡片 */
  .composer { display: grid; gap: 6px; min-width: 0; margin: 6px 0 8px; }
  .row { display: flex; gap: 6px; align-items: center; min-width: 0; }
  input {
    flex: 1 1 0;
    min-width: 0;
    min-height: 30px;
    padding: 5px 9px;
    border: 1px solid var(--rule);
    border-radius: 6px;
    color: var(--ink);
    background: var(--field);
    font: 400 var(--fs-2xs)/1.4 var(--font-ui);
  }
  input::placeholder { color: var(--ink-3); }
  input:focus-visible { outline: 1px solid var(--ink-2); outline-offset: 1px; }
  input:disabled { opacity: .55; cursor: not-allowed; }
  button {
    flex: 0 0 auto;
    min-height: 30px;
    padding: 4px 12px;
    border: 1px solid var(--rule);
    border-radius: 6px;
    color: var(--ink-3);
    background: transparent;
    font: 650 var(--fs-2xs)/1 var(--font-ui);
    cursor: pointer;
  }
  button:not(:disabled) { color: var(--ground); background: var(--ink); border-color: transparent; }
  button:not(:disabled):hover { opacity: .88; }
  button:disabled { cursor: not-allowed; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .ghost { min-height: 22px; padding: 2px 7px; border: 1px solid var(--rule); color: var(--ink-2); background: transparent; font-weight: 600; }
  .ghost:hover { color: var(--ink); border-color: var(--rule-strong); }
  .output { display: grid; gap: 4px; padding-top: 6px; border-top: 1px solid var(--rule); }
  .output-head { display: flex; align-items: center; justify-content: space-between; color: var(--ink-3); font-size: var(--fs-2xs); }
  .lines {
    margin: 0;
    max-height: 112px;
    overflow: auto;
    padding: 6px 8px;
    border-radius: 5px;
    color: var(--ink-log);
    background: var(--ground-sunken);
    font: 400 var(--fs-2xs)/1.6 var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .waiting { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); }
  .error { margin: 0; color: var(--state-error); font-size: var(--fs-2xs); overflow-wrap: anywhere; }
</style>






