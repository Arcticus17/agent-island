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
    VerificationRisk,
  } from "../../bridge/types";

  let { bridge }: { bridge: AgentIslandBridge } = $props();

  let capabilities: TeamCapability[] = $state([]);
  let runs: TeamRun[] = $state([]);
  let run: TeamRun | null = $state(null);
  let goal = $state("");
  let workspace = $state("");
  let budget = $state("2.00");
  let parallelism = $state("2");
  let mainAgent = $state("");
  let followupOpen = $state(false);
  let followupInstruction = $state("");
  let verificationRisks = $state<VerificationRisk[]>([]);
  let riskyConfirmOpen = $state(false);
  let followupBudget = $state("");
  let followupParallelism = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let retryArmed = $state(false);
  let diffs = $state<Record<string, string>>({});
  let diffLoading = $state<string | null>(null);
  let mergeArmed = $state<string | null>(null);
  let discardArmed = $state<string | null>(null);
  let taskBusy = $state<string | null>(null);
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
      verify_command: task.verifyCommand ?? "",
    }));
  }

  function applyRun(next: TeamRun | null): void {
    if (!next) return;
    if (run && run.id !== next.id) {
      diffs = {};
      mergeArmed = null;
      discardArmed = null;
      followupOpen = false;
      followupInstruction = "";
      riskyConfirmOpen = false;
      verificationRisks = [];
    }
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
      const parsedBudget = budget.trim() === "" ? null : Number(budget);
      if (parsedBudget !== null && (!Number.isFinite(parsedBudget) || parsedBudget <= 0)) {
        error = "预算必须是大于 0 的 USD 数字";
        busy = false;
        return;
      }
      const parsedParallelism = Number(parallelism);
      if (!Number.isInteger(parsedParallelism) || parsedParallelism < 1 || parsedParallelism > 4) {
        error = "并发必须是 1–4 的整数";
        busy = false;
        return;
      }
      const next = await bridge.startTeamPlan(
        goal.trim(),
        workspace.trim(),
        parsedBudget,
        parsedParallelism,
        mainAgent || undefined,
      );
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

  async function confirm(allowRisky = false): Promise<void> {
    if (busy || !run || !bridge.confirmTeamPlan) return;
    error = null;
    if (!allowRisky && bridge.checkTeamVerification) {
      busy = true;
      try {
        const risks = await bridge.checkTeamVerification(planTasks);
        if (risks.length > 0) {
          verificationRisks = risks;
          riskyConfirmOpen = true;
          busy = false;
          return;
        }
      } catch (reason) {
        error = String(reason);
        busy = false;
        return;
      }
    }
    busy = true;
    try {
      const next = await bridge.confirmTeamPlan(run.id, planTasks, allowRisky);
      riskyConfirmOpen = false;
      verificationRisks = [];
      applyRun(next);
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  function closeRisks(): void {
    riskyConfirmOpen = false;
    verificationRisks = [];
  }

  async function confirmRisks(): Promise<void> {
    await confirm(true);
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

  function hasUnresolvedMerges(value: TeamRun): boolean {
    return value.tasks.some(
      (task) =>
        (task.mergeState === "pending" ||
          task.mergeState === "conflict" ||
          task.changedFiles.length > 0) &&
        task.mergeState !== "merged" &&
        task.mergeState !== "discarded",
    );
  }

  function planTaskName(id: string): string {
    return planTasks.find((task) => task.id === id)?.title || id;
  }

  function taskTitle(value: TeamRun | null, id: string): string {
    return value?.tasks.find((task) => task.id === id)?.title || id;
  }

  function dependencyBlocker(task: TeamTask, value: TeamRun): string | null {
    for (const dependencyId of task.dependsOn) {
      const dependency = value.tasks.find((candidate) => candidate.id === dependencyId);
      if (!dependency) return `依赖任务不存在：${dependencyId}`;
      if (dependency.state !== "succeeded") return `依赖任务还没成功：${dependency.title}`;
      if (dependency.mergeState === "discarded") {
        return `依赖任务已丢弃，无法合并：${dependency.title}`;
      }
      if (dependency.changedFiles.length > 0 && dependency.mergeState !== "merged") {
        return `先合并依赖任务：${dependency.title}`;
      }
    }
    return null;
  }

  function dependentTasks(task: TeamTask, value: TeamRun): TeamTask[] {
    const ids = new Set<string>([task.id]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const candidate of value.tasks) {
        if (ids.has(candidate.id)) continue;
        if (candidate.dependsOn.some((dependency) => ids.has(dependency))) {
          ids.add(candidate.id);
          changed = true;
        }
      }
    }
    return value.tasks.filter((candidate) => candidate.id !== task.id && ids.has(candidate.id));
  }

  function discardLabel(task: TeamTask, value: TeamRun): string {
    const count = dependentTasks(task, value).length;
    return count > 0 ? `确认丢弃（含 ${count} 个后续任务）` : "确认丢弃改动";
  }

  function openFollowup(): void {
    if (!run) return;
    followupInstruction = "";
    followupBudget = run.budgetUsd != null ? run.budgetUsd.toFixed(2) : "";
    followupParallelism = String(run.concurrency);
    followupOpen = true;
  }

  async function continueRun(): Promise<void> {
    if (!run || !bridge.continueTeamRun || busy) return;
    const instruction = followupInstruction.trim();
    if (!instruction) {
      error = "请写一句下一轮要做什么";
      return;
    }
    let parsedBudget: number | null = null;
    if (followupBudget.trim() !== "") {
      parsedBudget = Number(followupBudget);
      if (!Number.isFinite(parsedBudget) || parsedBudget <= 0) {
        error = "下一轮预算必须是大于 0 的 USD 数字";
        return;
      }
    }
    let parsedParallelism: number | null = null;
    if (followupParallelism.trim() !== "") {
      parsedParallelism = Number(followupParallelism);
      if (!Number.isInteger(parsedParallelism) || parsedParallelism < 1 || parsedParallelism > 4) {
        error = "并发必须是 1–4 的整数";
        return;
      }
    }
    error = null;
    busy = true;
    try {
      const next = await bridge.continueTeamRun(run.id, instruction, parsedBudget, parsedParallelism ?? undefined);
      followupOpen = false;
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

  function shortCommit(value: string | null): string {
    return value ? value.slice(0, 7) : "";
  }

  async function ensureDiff(task: TeamTask): Promise<void> {
    if (!run) return;
    if (!bridge.teamTaskDiff) {
      diffs = { ...diffs, [task.id]: "当前构建不支持查看 diff" };
      return;
    }
    if (diffs[task.id] !== undefined) return;
    diffLoading = task.id;
    try {
      diffs = { ...diffs, [task.id]: await bridge.teamTaskDiff(run.id, task.id) };
    } catch (reason) {
      diffs = { ...diffs, [task.id]: `读取 diff 失败：${String(reason)}` };
    } finally {
      if (diffLoading === task.id) diffLoading = null;
    }
  }

  async function mergeTask(task: TeamTask): Promise<void> {
    if (!run || !bridge.mergeTeamTask) return;
    if (mergeArmed !== task.id) {
      mergeArmed = task.id;
      discardArmed = null;
      return;
    }
    taskBusy = task.id;
    error = null;
    try {
      applyRun(await bridge.mergeTeamTask(run.id, task.id));
      mergeArmed = null;
    } catch (reason) {
      error = String(reason);
      mergeArmed = null;
      await refresh();
    } finally {
      taskBusy = null;
    }
  }

  async function discardTask(task: TeamTask): Promise<void> {
    if (!run || !bridge.discardTeamTask) return;
    if (discardArmed !== task.id) {
      discardArmed = task.id;
      mergeArmed = null;
      return;
    }
    taskBusy = task.id;
    error = null;
    try {
      applyRun(await bridge.discardTeamTask(run.id, task.id));
      discardArmed = null;
    } catch (reason) {
      error = String(reason);
      discardArmed = null;
      await refresh();
    } finally {
      taskBusy = null;
    }
  }

  async function selectRun(id: string): Promise<void> {
    if (!bridge.getTeamRun) return;
    error = null;
    planForRunId = null;
    planTasks = [];
    retryArmed = false;
    diffs = {};
    mergeArmed = null;
    discardArmed = null;
    followupOpen = false;
    followupInstruction = "";
    riskyConfirmOpen = false;
    verificationRisks = [];
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
    budget = "2.00";
    parallelism = "2";
    diffs = {};
    mergeArmed = null;
    discardArmed = null;
    followupOpen = false;
    followupInstruction = "";
    riskyConfirmOpen = false;
    verificationRisks = [];
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
        verify_command: "",
      },
    ];
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
          if (!mainAgent) {
            mainAgent = capabilities.find((item) => item.available && item.canPlan)?.id ?? "";
          }
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
      <label class="field compact">
        <span>主 Agent</span>
        <select data-testid="team-main-agent" bind:value={mainAgent}>
          {#each capabilities.filter((item) => item.available && item.canPlan) as capability (capability.id)}
            <option value={capability.id}>{capability.name}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        <span>预算（USD）</span>
        <input
          data-testid="team-budget"
          inputmode="decimal"
          bind:value={budget}
          placeholder="2.00"
        />
      </label>
      <label class="field">
        <span>并发（1–4）</span>
        <input
          data-testid="team-concurrency"
          inputmode="numeric"
          bind:value={parallelism}
          placeholder="2"
        />
      </label>
      <p class="hint">预算只按本机 Claude 返回的成本累计；超出后停止派发，不会继续花钱。并发只用于互不依赖的任务。主 Agent 负责规划，次级 worker 按任务单独选择。</p>
      <div class="capabilities">
        {#each capabilities as capability (capability.id)}
          <span class="capability" class:ready={capability.available} title={capability.detail}>
            <span class="dot"></span>{capability.name}{capability.available ? (capability.canPlan ? " · 可规划/执行" : " · 可派发") : " · 不可用"}
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
      <p class="hint">主 Agent 只出计划；需要 git 仓库（至少一次提交）。确认后每个任务在独立 worktree 执行，主工作区不会被直接修改。</p>
    </section>
  {:else}
    <section class="run-panel" aria-label="团队任务状态">
      <div class="run-head">
        <span class={`phase phase-${run.phase}`}>{phaseText[run.phase]}</span>
        <span class="meta">{run.workspace}</span>
      </div>
      <p class="goal">{run.goal}</p>
      <p class="budget-line">
        第 {run.round} 轮 · 预算 {run.budgetUsd != null ? `$${run.budgetUsd.toFixed(2)}` : "不设上限"}
        · 已用 ${run.spentUsd.toFixed(2)}
        · 并发 {run.concurrency}
      </p>
      {#if run.worktreeMode}
        <p class="worktree-note">独立 worktree：{run.baseBranch ?? "HEAD"}@{shortCommit(run.baseCommit)}；主工作区不会被直接修改。</p>
      {/if}
      {#if run.workspaceWarning}<p class="warning" role="status">{run.workspaceWarning}</p>{/if}
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
            <label class="field compact">
              <span>验收命令（可选）</span>
              <input
                bind:value={task.verify_command}
                placeholder="例：npm test -- xxx；留空不跑"
              />
            </label>
            {#if task.verify_command.trim()}
              <p class="hint">会在任务 worktree 内执行，拥有当前用户权限；退出码 0 才算通过，失败不允许合并。</p>
            {/if}
            {#if task.depends_on.length > 0}
              <p class="depends">依赖：{task.depends_on.map(planTaskName).join(" → ")}</p>
            {/if}
            {#if task.verify_command.trim()}
              <p class="depends">验收：<code>{task.verify_command}</code></p>
            {/if}
          </article>
        {/each}
        <div class="plan-actions">
          <button class="ghost" type="button" onclick={addTask}>加一个任务</button>
          <button class="primary" type="button" disabled={busy} onclick={() => void confirm()}>
            {busy ? "正在开始…" : "确认并执行"}
          </button>
        </div>
        {#if riskyConfirmOpen}
          <section class="risk-panel" aria-label="高风险验收命令">
            <h3>验收命令需要再次确认</h3>
            <p class="hint">
              这些命令会以当前用户权限在任务 worktree 内执行，不是沙箱；确认后才会开始派发。
            </p>
            <ul>
              {#each verificationRisks as risk (risk.taskId)}
                <li>
                  <strong>{risk.taskTitle}</strong>
                  <code>{risk.command}</code>
                  <span>{risk.reason}</span>
                </li>
              {/each}
            </ul>
            <div class="plan-actions">
              <button class="ghost" type="button" onclick={closeRisks}>返回修改</button>
              <button class="primary" type="button" disabled={busy} onclick={() => void confirmRisks()}>
                {busy ? "正在开始…" : "仍然执行"}
              </button>
            </div>
          </section>
        {/if}
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
            {#if task.dependsOn.length > 0}
              <p class="depends">依赖：{task.dependsOn.map((id) => taskTitle(run, id)).join(" → ")}</p>
            {/if}
            {#if task.state === "waiting_approval"}
              <p class="waiting" role="status">
                等待在小岛上确认：<code>{task.pendingApproval ?? "危险命令"}</code>
              </p>
            {/if}
            {#if task.branch || task.mergeState !== "none"}
              <div class="worktree-row">
                {#if task.branch}<code class="branch">{task.branch}</code>{/if}
                {#if task.changedFiles.length > 0}<span class="file-count">{task.changedFiles.length} 个文件</span>{/if}
                {#if task.mergeState === "pending"}<span class="merge-state pending">待合并</span>
                {:else if task.mergeState === "merged"}<span class="merge-state merged">已合并</span>
                {:else if task.mergeState === "discarded"}<span class="merge-state discarded">已丢弃</span>
                {:else if task.mergeState === "conflict"}<span class="merge-state conflict">合并冲突</span>{/if}
              </div>
            {/if}
            {#if task.diffStat}<p class="diff-stat">{task.diffStat}</p>{/if}
            {#if task.verification}
              <div class="verify-row">
                <span class={`verify-badge ${task.verification.passed ? "passed" : "failed"}`}>
                  {task.verification.timedOut ? "验收超时" : task.verification.passed ? "验收通过" : "验收失败"}
                </span>
                <span class="meta">
                  退出码 {task.verification.exitCode ?? "—"} · {task.verification.durationMs}ms
                </span>
              </div>
              <details>
                <summary>验收命令与输出</summary>
                <code class="verify-command">{task.verification.command}</code>
                <pre>{task.verification.output || "（无输出）"}</pre>
              </details>
            {:else if task.verifyCommand}
              <p class="hint">验收命令：<code>{task.verifyCommand}</code></p>
            {/if}
            {#if task.changedFiles.length > 0 && task.branch}
              <details ontoggle={(event) => { if (event.currentTarget.open) void ensureDiff(task); }}>
                <summary>查看改动（{task.changedFiles.length} 个文件）</summary>
                {#if diffLoading === task.id}<p class="hint">正在读取 diff…</p>
                {:else if diffs[task.id] !== undefined}<pre>{diffs[task.id]}</pre>
                {:else}<p class="hint">展开后读取 diff</p>{/if}
              </details>
            {/if}
            {#if task.mergeError}<p class="task-error">{task.mergeError}</p>{/if}
            {#if task.mergeState === "pending" && task.changedFiles.length > 0}
              <div class="merge-actions">
                {#if task.state === "succeeded" && !activePhases.includes(run.phase) && dependencyBlocker(task, run)}
                  <span class="hint">{dependencyBlocker(task, run)}</span>
                {:else if task.state === "succeeded" && !activePhases.includes(run.phase)}
                  <button class="primary" type="button" disabled={taskBusy === task.id} onclick={() => void mergeTask(task)}>
                    {mergeArmed === task.id ? "确认合并到主工作区" : "合并"}
                  </button>
                {:else if task.state === "succeeded"}
                  <span class="hint">任务已完成；等这一轮结束后可合并。</span>
                {:else}
                  <span class="hint">任务未成功；改动保留在分支，可查看或丢弃。</span>
                {/if}
                {#if dependentTasks(task, run).length > 0}
                  <span class="hint">丢弃会级联 {dependentTasks(task, run).length} 个后续任务</span>
                {/if}
                <button class="ghost" type="button" disabled={taskBusy === task.id} onclick={() => void discardTask(task)}>
                  {discardArmed === task.id ? discardLabel(task, run) : "丢弃改动"}
                </button>
              </div>
            {:else if task.worktreePath && task.mergeState === "none" && task.state !== "running" && task.state !== "waiting_approval"}
              <div class="merge-actions">
                <span class="hint">这个任务没有可合并的改动；可以清理 worktree。</span>
                <button class="ghost" type="button" disabled={taskBusy === task.id} onclick={() => void discardTask(task)}>
                  {discardArmed === task.id
                    ? (dependentTasks(task, run).length > 0
                      ? `确认清理（含 ${dependentTasks(task, run).length} 个后续任务）`
                      : "确认清理 worktree")
                    : "清理 worktree"}
                </button>
              </div>
            {:else if task.mergeState === "conflict"}
              <div class="merge-actions">
                <span class="hint">合并冲突已 abort；分支和 worktree 保留，可手动处理或丢弃。</span>
                <button class="ghost" type="button" disabled={taskBusy === task.id} onclick={() => void discardTask(task)}>
                  {discardArmed === task.id ? discardLabel(task, run) : "丢弃改动"}
                </button>
              </div>
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
        {#if run.spentUsd > 0}<p class="meta cost">本次花费 ${run.spentUsd.toFixed(3)}</p>{/if}
      </section>
    {/if}

    {#if run.summary}
      <section class="summary" aria-label="主 Agent 汇总">
        <h3>主 Agent 汇总</h3>
        <pre>{run.summary}</pre>
      </section>
    {/if}

    {#if isTerminal(run.phase)}
      {#if hasUnresolvedMerges(run)}
        <p class="hint followup-hint">先合并或丢弃上一轮的改动，再来一轮。</p>
      {:else if followupOpen}
        <section class="followup-panel" aria-label="下一轮规划">
          <h3>再来一轮</h3>
          <label class="field">
            <span>下一轮要做什么</span>
            <textarea
              data-testid="team-followup-instruction"
              rows="2"
              bind:value={followupInstruction}
              placeholder="例如：补齐上一轮汇总里没验证的测试，然后更新文档"
            ></textarea>
          </label>
          <div class="followup-fields">
            <label class="field compact">
              <span>预算（留空继承）</span>
              <input data-testid="team-followup-budget" inputmode="decimal" bind:value={followupBudget} />
            </label>
            <label class="field compact">
              <span>并发（1–4）</span>
              <input data-testid="team-followup-concurrency" inputmode="numeric" bind:value={followupParallelism} />
            </label>
          </div>
          <div class="plan-actions">
            <button class="ghost" type="button" onclick={() => { followupOpen = false; }}>取消</button>
            <button
              class="primary"
              type="button"
              disabled={busy || !followupInstruction.trim()}
              onclick={() => void continueRun()}
            >{busy ? "正在请主 Agent 规划…" : "继续规划"}</button>
          </div>
          <p class="hint">会把上一轮的目标、汇总和任务结果带给主 Agent；新计划仍需要你确认。</p>
        </section>
      {:else if bridge.continueTeamRun}
        <section class="followup-cta">
          <button class="ghost" type="button" data-testid="team-followup" onclick={openFollowup}>再来一轮</button>
          <span class="hint">基于上一轮结果继续规划；已合并的改动会作为新起点。</span>
        </section>
      {/if}
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
  .budget-line { margin-top: 4px; color: var(--text-secondary); font-size: 10px; font-variant-numeric: tabular-nums; }
  .worktree-note { margin-top: 4px; color: var(--text-secondary); font-size: 10px; line-height: 1.45; }
  .warning { margin-top: 6px; padding: 6px 8px; border-radius: 7px; color: var(--text-primary); background: color-mix(in srgb, var(--status-waiting) 16%, transparent); font-size: 10px; line-height: 1.45; }
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
  .worktree-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  .branch { color: var(--text-secondary); font: 9px/1.4 var(--font-mono, Consolas, monospace); overflow-wrap: anywhere; }
  .file-count { color: var(--text-secondary); font-size: 9px; }
  .merge-state { padding: 1px 5px; border-radius: 3px; font-size: 9px; font-weight: 600; }
  .merge-state.pending { color: var(--status-waiting); background: color-mix(in srgb, var(--status-waiting) 16%, transparent); }
  .merge-state.merged { color: var(--status-done); background: color-mix(in srgb, var(--status-done) 16%, transparent); }
  .merge-state.discarded { color: var(--text-secondary); background: var(--field); }
  .merge-state.conflict { color: var(--status-error); background: color-mix(in srgb, var(--status-error) 16%, transparent); }
  .diff-stat { margin-top: 3px; color: var(--text-secondary); font: 9px/1.45 var(--font-mono, Consolas, monospace); white-space: pre-wrap; }
  .merge-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; margin-top: 6px; }
  .waiting { margin-top: 5px; padding: 6px 8px; border-radius: 7px; color: var(--text-primary); background: color-mix(in srgb, var(--status-waiting) 16%, transparent); font-size: 10px; line-height: 1.45; }
  .waiting code { font: 10px/1.4 var(--font-mono, Consolas, monospace); word-break: break-all; }
  details { margin-top: 5px; }
  summary { color: var(--text-secondary); font-size: 10px; cursor: pointer; }
  pre { margin: 5px 0 0; max-height: 180px; overflow: auto; white-space: pre-wrap; word-break: break-word; font: 10px/1.5 var(--font-mono, Consolas, monospace); color: var(--text-secondary); }
  .cost { margin-top: 6px; text-align: right; }
  .summary pre { color: var(--text-primary); font-family: inherit; font-size: 11.5px; }
  .followup-panel { border: 1px solid var(--border-subtle); border-radius: var(--radius-card, 10px); background: var(--surface-card); padding: 11px; display: flex; flex-direction: column; gap: 8px; }
  .followup-fields { display: grid; grid-template-columns: 1fr 1fr; gap: 7px; }
  .followup-cta { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .followup-hint { padding: 6px 8px; border-radius: 7px; background: var(--field); }
  .verify-row { display: flex; align-items: center; gap: 7px; margin-top: 5px; }
  .verify-badge { padding: 2px 6px; border-radius: 5px; font-size: 10px; font-weight: 600; }
  .verify-badge.passed { color: var(--status-done); background: color-mix(in srgb, var(--status-done) 14%, transparent); }
  .verify-badge.failed { color: var(--danger-ink); background: var(--danger); }
  .verify-command { display: block; margin-top: 4px; padding: 5px 7px; border-radius: 5px; background: var(--field); font-size: 10px; white-space: pre-wrap; word-break: break-word; }
  .risk-panel { margin-top: 9px; padding: 10px; border: 1px solid var(--danger-ink); border-radius: var(--radius-card, 10px); background: var(--danger); display: flex; flex-direction: column; gap: 7px; }
  .risk-panel h3 { margin: 0; color: var(--danger-ink); font-size: 11.5px; }
  .risk-panel ul { margin: 0; padding-left: 16px; display: flex; flex-direction: column; gap: 5px; }
  .risk-panel li { color: var(--text-primary); font-size: 10.5px; line-height: 1.45; }
  .risk-panel code { display: inline-block; margin: 2px 0; padding: 2px 5px; border-radius: 4px; background: var(--field); word-break: break-word; }
  .risk-panel li span { display: block; color: var(--danger-ink); }
  .history { display: flex; flex-direction: column; gap: 4px; }
  .history-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; width: 100%; text-align: left; background: transparent; border-color: transparent; }
  .history-row.selected { background: var(--surface-raised); border-color: var(--border-subtle); }
  .history-goal { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
  .history .dot.phase-succeeded { background: var(--status-done); }
  .history .dot.phase-failed { background: var(--status-error); }
  .history .dot.phase-running { background: var(--status-working); }
  .history .dot.phase-awaiting_plan { background: var(--status-waiting); }
</style>
