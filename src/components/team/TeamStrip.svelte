<script lang="ts">
  import { onMount } from "svelte";
  import type { AgentIslandBridge, TeamRun, TeamRunPhase } from "../../bridge/types";

  let { bridge }: { bridge: AgentIslandBridge } = $props();

  let run: TeamRun | null = $state(null);
  let disposed = false;
  let pollTimer: number | undefined;
  let hideTimer: number | undefined;
  let unlisten: (() => void) | null = null;

  const activePhases: TeamRunPhase[] = ["planning", "awaiting_plan", "running", "summarizing"];

  const phaseText: Record<TeamRunPhase, string> = {
    planning: "拆解中",
    awaiting_plan: "待确认",
    running: "执行中",
    summarizing: "汇总中",
    succeeded: "已完成",
    failed: "失败",
    cancelled: "已取消",
  };

  function isActive(phase: TeamRunPhase): boolean {
    return activePhases.includes(phase);
  }

  function apply(next: TeamRun | null): void {
    if (!next) return;
    run = next;
    if (isActive(next.phase)) {
      if (hideTimer !== undefined) {
        window.clearTimeout(hideTimer);
        hideTimer = undefined;
      }
      startPolling();
    } else {
      stopPolling();
      if (hideTimer !== undefined) window.clearTimeout(hideTimer);
      hideTimer = window.setTimeout(() => {
        if (!disposed && run && !isActive(run.phase)) run = null;
      }, 12_000);
    }
  }

  async function load(): Promise<void> {
    if (!bridge.listTeamRuns) return;
    try {
      const runs = await bridge.listTeamRuns();
      if (disposed) return;
      const active = runs.find((item) => isActive(item.phase));
      if (active) {
        apply(active);
        return;
      }
      const latest = runs[0];
      if (latest && Date.now() - latest.updatedAtMs < 12_000) apply(latest);
    } catch {
      // 团队接口不可用时整条隐藏，不影响主岛。
    }
  }

  async function refresh(): Promise<void> {
    if (!run || !bridge.getTeamRun) return;
    try {
      const next = await bridge.getTeamRun(run.id);
      if (!disposed && next) apply(next);
    } catch {
      // 下一轮再试。
    }
  }

  function startPolling(): void {
    if (pollTimer !== undefined) return;
    pollTimer = window.setInterval(() => void refresh(), 2500);
  }

  function stopPolling(): void {
    if (pollTimer !== undefined) {
      window.clearInterval(pollTimer);
      pollTimer = undefined;
    }
  }

  function open(): void {
    const request = bridge.openTeam?.();
    if (request) void request.catch(() => {});
  }

  function pendingCount(value: TeamRun): number {
    return value.tasks.filter((task) => task.state === "waiting_approval").length;
  }

  function doneCount(value: TeamRun): number {
    return value.tasks.filter((task) => task.state === "succeeded").length;
  }

  onMount(() => {
    disposed = false;
    void (async () => {
      await load();
      if (bridge.listenTeamRuns) {
        try {
          unlisten = await bridge.listenTeamRuns((next) => {
            if (disposed) return;
            apply(next);
          });
        } catch {
          unlisten = null;
        }
      }
    })();
    return () => {
      disposed = true;
      stopPolling();
      if (hideTimer !== undefined) window.clearTimeout(hideTimer);
      unlisten?.();
    };
  });
</script>

{#if run}
  {@const waiting = pendingCount(run)}
  <button
    type="button"
    class={`team-strip phase-${run.phase}`}
    data-testid="team-strip"
    aria-label={`团队任务：${phaseText[run.phase]}，${doneCount(run)}/${run.tasks.length} 完成${waiting > 0 ? `，${waiting} 个等待审批` : ""}。打开总览`}
    onclick={open}
  >
    <span class="tag">TEAM</span>
    <span class="lead">{run.mainAgentName} 主</span>
    <span class="phase">{phaseText[run.phase]}</span>
    <span class="progress">{doneCount(run)}/{run.tasks.length}</span>
    {#if waiting > 0}
      <span class="waiting">{waiting} 待审批</span>
    {/if}
    <span class="chevron" aria-hidden="true">›</span>
  </button>
{/if}

<style>
  .team-strip {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    min-height: 32px;
    padding: 5px 10px;
    border: 0;
    border-bottom: 1px solid var(--rule, color-mix(in srgb, currentColor 10%, transparent));
    border-radius: 0;
    color: var(--ink, currentColor);
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .team-strip:hover { background: var(--field, color-mix(in srgb, currentColor 6%, transparent)); }
  .team-strip:focus-visible { outline: 2px solid var(--ink, currentColor); outline-offset: -2px; }
  .tag {
    padding: 1px 5px;
    border: 1px solid var(--rule-strong, color-mix(in srgb, currentColor 20%, transparent));
    border-radius: 3px;
    color: var(--ink-3, currentColor);
    font: 600 8px/1.4 var(--font-readout, inherit);
    letter-spacing: 0.08em;
  }
  .lead { font-size: 11px; font-weight: 650; white-space: nowrap; }
  .phase { color: var(--ink-2, currentColor); font-size: 10px; }
  .progress { margin-left: auto; color: var(--ink-2, currentColor); font: 600 10px/1 var(--font-readout, inherit); font-variant-numeric: tabular-nums; }
  .waiting {
    padding: 2px 6px;
    border-radius: 3px;
    color: var(--alarm-ink);
    background: var(--alarm);
    font-size: 9px;
    font-weight: 700;
    white-space: nowrap;
  }
  .chevron { color: var(--ink-3, currentColor); font-size: 14px; line-height: 1; }
  .phase-running .phase, .phase-summarizing .phase { color: var(--state-working, currentColor); }
  .phase-awaiting_plan .phase { color: var(--state-waiting, currentColor); }
  .phase-succeeded .phase { color: var(--state-done, currentColor); }
  .phase-failed .phase { color: var(--state-error, currentColor); }
  @media (prefers-reduced-motion: reduce) {
    .team-strip { transition: none; }
  }
</style>
