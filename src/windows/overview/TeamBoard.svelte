<script lang="ts">
  import { onMount } from "svelte";
  import type {
    AgentIslandBridge,
    TeamCapability,
    TeamPlanTask,
    TeamRun,
    TeamRunPhase,
    TeamTask,
    TeamTaskState,
  } from "../../bridge/types";

  let { bridge }: { bridge: AgentIslandBridge } = $props();

  let capabilities: TeamCapability[] = $state([]);
  let runs: TeamRun[] = $state([]);
  let run: TeamRun | null = $state(null);
  let goal = $state("");
  let workspace = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let retryArmed = $state(false);
  let planTasks: TeamPlanTask[] = $state([]);
  let planForRunId: string | null = null;
  let disposed = false;
  let pollTimer: number | undefined;
  let unlisten: (() => void) | null = null;

  const phaseText: Record<TeamRunPhase, string> = {
    planning: "主 Agent 正在拆解",
    awaiting_plan: "计划已生成，等你确认",
    running: "执行中",
    summarizing: "正在汇总",
    succeeded: "已完成",
    failed: "失败",
    cancelled: "已取消",
  };

  const taskText: Record<TeamTaskState, string> = {
    queued: "排队",
    running: "执行中",
    waiting_approval: "等待审批",
    succeeded: "完成",
    failed: "失败",
    cancelled: "取消",
    skipped: "跳过",
  };

  const activePhases: TeamRunPhase[] = ["planning", "running", "summarizing"];

  function isTerminal(phase: TeamRunPhase): boolean {
    return phase === "succeeded" || phase === "failed" || phase === "cancelled";
  }

  function syncPlan(next: TeamRun): void {
    if (next.phase !== "awaiting_plan") {
      if (next.phase !== "planning") planForRunId = null;
      return;
    }
    if (planForRunId === next.id && planTasks.length > 0) return;
    planForRunId = next.id;
    planTasks = next.tasks.map((task) => ({
      id: task.id,
      title: task.title,
      assignee: task.assignee,
      prompt: task.prompt,
      depends_on: [...task.dependsOn],
      acceptance: task.acceptance,
    }));
  }

  function applyRun(next: TeamRun | null): void {
    if (!next) return;
    run = next;
    workspace = workspace || next.workspace;
    syncPlan(next);
    if (activePhases.includes(next.phase)) {
      startPolling();
    } else {
      stopPolling();
    }
  }

  async function refresh(): Promise<void> {
    if (!run || !bridge.getTeamRun) return;
    try {
      const next = await bridge.getTeamRun(run.id);
      if (disposed) return;
      if (next) applyRun(next);
    } catch {
      // 轮询失败不覆盖已有界面；下一轮会再试。
    }
  }

  function startPolling(): void {
    if (pollTimer !== undefined) return;
    pollTimer = window.setInterval(() => void refresh(), 1500);
  }

  function stopPolling(): void {
    if (pollTimer !== undefined) {
      window.clearInterval(pollTimer);
      pollTimer = undefined;
    }
  }

  async function loadRuns(): Promise<void> {
    if (!bridge.listTeamRuns) return;
    try {
      runs = await bridge.listTeamRuns();
      if (!run && runs.length > 0) {
        const latest = runs.find((item) => !isTerminal(item.phase)) ?? runs[0];
        applyRun(latest);
      }
    } catch {
      // 历史记录读不到时仍然可以新建任务。
    }
  }

  async function generate(): Promise<void> {
    if (busy) return;
    if (!bridge.startTeamPlan) {
      error = "这个构建没有团队任务接口";
      return;
    }
    error = null;
    busy = true;
    try {
      const next = await bridge.startTeamPlan(goal.trim(), workspace.trim());
      planTasks = [];
      planForRunId = null;
      applyRun(next);
      await loadRuns();
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  async function confirm(): Promise<void> {
    if (busy || !run || !bridge.confirmTeamPlan) return;
    error = null;
    busy = true;
    try {
      const next = await bridge.confirmTeamPlan(run.id, planTasks);
      applyRun(next);
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  async function cancel(): Promise<void> {
    if (!run || !bridge.cancelTeamRun) return;
    error = null;
    retryArmed = false;
    try {
      applyRun(await bridge.cancelTeamRun(run.id));
    } catch (reason) {
      error = String(reason);
    }
  }

  async function retry(): Promise<void> {
    if (!run || !bridge.retryTeamRun) return;
    if (!retryArmed) {
      retryArmed = true;
      return;
    }
    error = null;
    busy = true;
    try {
      applyRun(await bridge.retryTeamRun(run.id));
      retryArmed = false;
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  function hasUnfinishedTasks(value: TeamRun): boolean {
    return value.tasks.some((task) => task.state !== "succeeded");
  }

  async function selectRun(id: string): Promise<void> {
    if (!bridge.getTeamRun) return;
    error = null;
    planForRunId = null;
    planTasks = [];
    retryArmed = false;
    try {
      applyRun(await bridge.getTeamRun(id));
    } catch (reason) {
      error = String(reason);
    }
  }

  function reset(): void {
    stopPolling();
    run = null;
    planTasks = [];
    planForRunId = null;
    goal = "";
    error = null;
    retryArmed = false;
  }

  function removeTask(id: string): void {
    if (planTasks.length <= 1) return;
    planTasks = planTasks.filter((task) => task.id !== id);
  }

  function addTask(): void {
    const id = `task-${planTasks.length + 1}`;
    planTasks = [
      ...planTasks,
      {
        id,
        title: "新任务",
        assignee: capabilities.find((item) => item.available)?.id ?? "claude",
        prompt: "",
        depends_on: [],
        acceptance: "",
      },
    ];
  }

  function taskCost(tasks: TeamTask[]): string {
    const total = tasks.reduce((sum, task) => sum + (task.costUsd ?? 0), 0);
    return total > 0 ? `$${total.toFixed(3)}` : "";
  }

  function taskElapsed(task: TeamTask): string {
    if (!task.startedAtMs) return "";
    const end = task.endedAtMs ?? Date.now();
    const seconds = Math.max(0, Math.round((end - task.startedAtMs) / 1000));
    return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m${seconds % 60}s`;
  }

  onMount(() => {
    disposed = false;
    void (async () => {
      if (bridge.teamCapabilities) {
        try {
          capabilities = await bridge.teamCapabilities();
        } catch {
          capabilities = [];
        }
      }
      await loadRuns();
      if (bridge.listenTeamRuns) {
        try {
          unlisten = await bridge.listenTeamRuns((next) => {
            if (!run || next.id === run.id) applyRun(next);
            void loadRuns();
          });
        } catch {
          unlisten = null;
        }
      }
    })();
    return () => {
      disposed = true;
      stopPolling();
      unlisten?.();
    };
  });
</script>

<div class="team-board" data-testid="team-board">
  <header class="board-head">
    <div>
      <span class="eyebrow">TEAM</span>
      <h2>团队任务</h2>
    </div>
    {#if run}
      <button type="button" class="ghost" onclick={reset} disabled={busy}>新建</button>
    {/if}
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if !run}
    <section class="composer-panel" aria-label="新建团队任务">
      <label class="field">
        <span>目标</span>
        <textarea
          data-testid="team-goal"
          rows="3"
          bind:value={goal}
          placeholder="例如：给设置页加一个键盘快捷键设置，并补测试"
        ></textarea>
      </label>
      <label class="field">
        <span>工作区</span>
        <input
          data-testid="team-workspace"
          bind:value={workspace}
          placeholder="C:\\path\\to\\project"
        />
      </label>
      <div class="capabilities">
        {#each capabilities as capability (capability.id)}
          <span class="capability" class:ready={capability.available} title={capability.detail}>
            <span class="dot"></span>{capability.name}{capability.available ? " · 可派发" : " · 不可用"}
          </span>
        {/each}
      </div>
      <button
        class="primary"
        type="button"
        data-testid="team-generate"
        disabled={busy || !goal.trim() || !workspace.trim()}
        onclick={() => void generate()}
      >{busy ? "正在请主 Agent 拆解…" : "生成计划"}</button>
      <p class="hint">主 Agent 只出计划；你确认后，岛才会派发次级 Agent 执行。</p>
    </section>
  {:else}
    <section class="run-panel" aria-label="团队任务状态">
      <div class="run-head">
        <span class={`phase phase-${run.phase}`}>{phaseText[run.phase]}</span>
        <span class="meta">{run.workspace}</span>
      </div>
      <p class="goal">{run.goal}</p>
      {#if run.error}<p class="error" role="alert">{run.error}</p>{/if}
      {#if activePhases.includes(run.phase)}
        <button class="danger ghost" type="button" onclick={() => void cancel()}>取消运行</button>
      {:else if (run.phase === "failed" || run.phase === "cancelled") && hasUnfinishedTasks(run)}
        <div class="retry-row">
          <button class="primary" type="button" disabled={busy} onclick={() => void retry()}>
            {retryArmed ? "确认重试未完成任务" : "重试未完成任务"}
          </button>
          {#if retryArmed}<span class="hint">会再次执行失败/跳过的任务，已完成的不会重跑。</span>{/if}
        </div>
      {/if}
    </section>

    {#if run.phase === "planning"}
      <p class="notice" role="status">主 Agent 正在读工作区并拆解目标，通常需要 10–30 秒。</p>
    {:else if run.phase === "awaiting_plan"}
      <section class="plan" aria-label="计划预览">
        {#if run.planSummary}<p class="plan-summary">{run.planSummary}</p>{/if}
        {#each planTasks as task, index (task.id)}
          <article class="plan-task">
            <div class="task-head">
              <strong>{index + 1}. {task.title || "未命名任务"}</strong>
              <button class="ghost tiny" type="button" onclick={() => removeTask(task.id)} disabled={planTasks.length <= 1}>删除</button>
            </div>
            <div class="task-fields">
              <label class="field compact">
                <span>标题</span>
                <input bind:value={task.title} />
              </label>
              <label class="field compact">
                <span>负责人</span>
                <select bind:value={task.assignee}>
                  {#each capabilities.filter((item) => item.available) as capability (capability.id)}
                    <option value={capability.id}>{capability.name}</option>
                  {/each}
                </select>
              </label>
            </div>
            <label class="field compact">
              <span>任务说明</span>
              <textarea rows="3" bind:value={task.prompt} placeholder="要改什么、验收标准是什么"></textarea>
            </label>
            <label class="field compact">
              <span>验收标准</span>
              <input bind:value={task.acceptance} />
            </label>
            {#if task.depends_on.length > 0}
              <p class="depends">依赖：{task.depends_on.join("、")}</p>
            {/if}
          </article>
        {/each}
        <div class="plan-actions">
          <button class="ghost" type="button" onclick={addTask}>加一个任务</button>
          <button class="primary" type="button" disabled={busy} onclick={() => void confirm()}>
            {busy ? "正在开始…" : "确认并执行"}
          </button>
        </div>
      </section>
    {:else}
      <section class="tasks" aria-label="任务列表">
        {#each run.tasks as task (task.id)}
          <article class={`task state-${task.state}`}>
            <div class="task-line">
              <span class="dot" aria-hidden="true"></span>
              <strong>{task.title}</strong>
              <span class="state">{taskText[task.state]}</span>
              <span class="meta">{taskElapsed(task)}</span>
            </div>
            {#if task.error}<p class="task-error">{task.error}</p>{/if}
            {#if task.state === "waiting_approval"}
              <p class="waiting" role="status">
                等待在小岛上确认：<code>{task.pendingApproval ?? "危险命令"}</code>
              </p>
            {/if}
            {#if task.log.length > 0}
              <details>
                <summary>过程（{task.log.length} 行）</summary>
                <pre>{task.log.join("\n")}</pre>
              </details>
            {/if}
            {#if task.result}
              <details open={task.state === "succeeded"}>
                <summary>结果</summary>
                <pre>{task.result}</pre>
              </details>
            {/if}
          </article>
        {/each}
        {#if taskCost(run.tasks)}<p class="meta cost">本次花费 {taskCost(run.tasks)}</p>{/if}
      </section>
    {/if}

    {#if run.summary}
      <section class="summary" aria-label="主 Agent 汇总">
        <h3>主 Agent 汇总</h3>
        <pre>{run.summary}</pre>
      </section>
    {/if}
  {/if}

  {#if runs.length > 0}
    <section class="history" aria-label="最近团队任务">
      <h3>最近</h3>
      {#each runs.slice(0, 8) as item (item.id)}
        <button
          type="button"
          class={`history-row ${run?.id === item.id ? "selected" : ""}`}
          onclick={() => void selectRun(item.id)}
        >
          <span class={`dot phase-${item.phase}`}></span>
          <span class="history-goal">{item.goal}</span>
          <span class="meta">{phaseText[item.phase]}</span>
        </button>
      {/each}
    </section>
  {/if}
</div>

<style>
  .team-board { height: 100%; overflow: auto; display: flex; flex-direction: column; gap: 10px; padding: 14px; }
  .board-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h2, h3, p { margin: 0; }
  h2 { font-size: 15px; }
  h3 { font-size: 12px; color: var(--text-secondary); font-weight: 600; }
  .eyebrow { display: block; color: var(--text-secondary); font-size: 9px; letter-spacing: 0.14em; }
  .composer-panel, .run-panel, .plan, .tasks, .summary, .history {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-card, 10px);
    background: var(--surface-card);
    padding: 11px;
  }
  .composer-panel { display: flex; flex-direction: column; gap: 9px; }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 10px; color: var(--text-secondary); }
  .field.compact { gap: 3px; }
  textarea, input, select {
    width: 100%;
    min-height: 32px;
    padding: 7px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-control, 7px);
    color: var(--text-primary);
    background: var(--control-surface);
    font: inherit;
    resize: vertical;
  }
  textarea { min-height: 64px; }
  button {
    min-height: 30px;
    padding: 5px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-control, 7px);
    color: var(--text-primary);
    background: var(--control-surface);
    font: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) { background: var(--control-hover); }
  button:disabled { opacity: 0.4; cursor: default; }
  button.primary { color: var(--alarm-ink); background: var(--alarm); border-color: transparent; font-weight: 600; }
  button.danger { color: var(--danger-ink); background: var(--danger); border-color: transparent; }
  button.ghost { background: transparent; }
  button.tiny { min-height: 24px; padding: 2px 7px; font-size: 10px; }
  .capabilities { display: flex; flex-wrap: wrap; gap: 6px; }
  .capability { display: inline-flex; align-items: center; gap: 5px; padding: 3px 7px; border-radius: 999px; border: 1px solid var(--border-subtle); color: var(--text-secondary); font-size: 10px; }
  .capability.ready { color: var(--status-done); border-color: color-mix(in srgb, var(--status-done) 40%, transparent); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-secondary); flex: 0 0 auto; }
  .capability.ready .dot { background: var(--status-done); }
  .hint, .notice, .meta, .depends { color: var(--text-secondary); font-size: 10px; }
  .run-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .phase { font-size: 11px; font-weight: 600; }
  .phase-planning, .phase-running, .phase-summarizing { color: var(--status-working); }
  .phase-awaiting_plan { color: var(--status-waiting); }
  .phase-succeeded { color: var(--status-done); }
  .phase-failed { color: var(--status-error); }
  .phase-cancelled { color: var(--text-secondary); }
  .goal { margin-top: 6px; font-size: 12px; line-height: 1.5; }
  .retry-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .error { padding: 7px 9px; border-radius: 7px; color: var(--danger-ink); background: var(--danger); font-size: 11px; }
  .plan-summary { color: var(--text-secondary); font-size: 11px; line-height: 1.5; margin-bottom: 8px; }
  .plan-task { padding: 9px 0; border-top: 1px solid var(--border-subtle); display: flex; flex-direction: column; gap: 6px; }
  .plan-task:first-of-type { border-top: 0; padding-top: 0; }
  .task-head, .task-line, .plan-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .task-fields { display: grid; grid-template-columns: 1fr 110px; gap: 7px; }
  .plan-actions { margin-top: 8px; }
  .tasks { display: flex; flex-direction: column; gap: 0; }
  .task { padding: 9px 0; border-top: 1px solid var(--border-subtle); }
  .task:first-child { border-top: 0; padding-top: 0; }
  .task .dot { background: var(--text-secondary); }
  .task.state-running .dot { background: var(--status-working); }
  .task.state-waiting_approval .dot { background: var(--status-waiting); }
  .task.state-succeeded .dot { background: var(--status-done); }
  .task.state-failed .dot { background: var(--status-error); }
  .task.state-skipped .dot, .task.state-cancelled .dot { background: var(--text-secondary); }
  .task-line strong { flex: 1; min-width: 0; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state { color: var(--text-secondary); font-size: 10px; }
  .task-error { margin-top: 4px; color: var(--status-error); font-size: 10px; }
  .waiting { margin-top: 5px; padding: 6px 8px; border-radius: 7px; color: var(--text-primary); background: color-mix(in srgb, var(--status-waiting) 16%, transparent); font-size: 10px; line-height: 1.45; }
  .waiting code { font: 10px/1.4 var(--font-mono, Consolas, monospace); word-break: break-all; }
  details { margin-top: 5px; }
  summary { color: var(--text-secondary); font-size: 10px; cursor: pointer; }
  pre { margin: 5px 0 0; max-height: 180px; overflow: auto; white-space: pre-wrap; word-break: break-word; font: 10px/1.5 var(--font-mono, Consolas, monospace); color: var(--text-secondary); }
  .cost { margin-top: 6px; text-align: right; }
  .summary pre { color: var(--text-primary); font-family: inherit; font-size: 11.5px; }
  .history { display: flex; flex-direction: column; gap: 4px; }
  .history-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; width: 100%; text-align: left; background: transparent; border-color: transparent; }
  .history-row.selected { background: var(--surface-raised); border-color: var(--border-subtle); }
  .history-goal { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
  .history .dot.phase-succeeded { background: var(--status-done); }
  .history .dot.phase-failed { background: var(--status-error); }
  .history .dot.phase-running { background: var(--status-working); }
  .history .dot.phase-awaiting_plan { background: var(--status-waiting); }
</style>
