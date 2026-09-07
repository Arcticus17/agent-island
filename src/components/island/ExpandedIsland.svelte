<script lang="ts">
  import type { AgentView, SessionView } from "../../bridge/types";
  import type { SnapshotRefreshError } from "../../stores/agent-store";
  import LogCard from "../cards/LogCard.svelte";
  import SessionCard from "../cards/SessionCard.svelte";
  import StatusCard from "../cards/StatusCard.svelte";
  import UsageCard from "../cards/UsageCard.svelte";
  let { agents, selectedAgent, session, refreshError, onSelectAgent }: { agents: readonly AgentView[]; selectedAgent: AgentView | null; session: SessionView | null; refreshError: SnapshotRefreshError | null; onSelectAgent: (id: string) => void } = $props();
</script>

<section id="expanded-island" class="expanded" aria-label="Agent 详情">
  <nav class="agent-strip" aria-label="切换 Agent">
    {#each agents as agent (agent.id)}
      <button type="button" class:active={agent.id === selectedAgent?.id} data-agent-id={agent.id} aria-pressed={agent.id === selectedAgent?.id} onclick={() => onSelectAgent(agent.id)}><span class={`mini-dot status-${agent.display_status}`} aria-hidden="true"></span>{agent.name}</button>
    {/each}
  </nav>
  {#if refreshError}<p class="refresh-note" role="status">数据刷新暂时失败，已保留最近状态。</p>{/if}
  {#if selectedAgent && session}
    <div class="card-grid" data-testid="card-grid"><StatusCard agent={selectedAgent} {session} /><UsageCard agent={selectedAgent} refreshFailed={refreshError !== null} /><SessionCard {session} /><LogCard {session} /></div>
  {:else}
    <div class="empty" data-testid="empty-session" role="status"><strong>无法确认当前会话</strong><span>为避免串入历史对话，灵动岛不会显示其他会话内容。</span></div>
  {/if}
</section>

<style>
  .expanded { border-top: 1px solid var(--island-border-subtle); padding: 12px; }
  .agent-strip { display: flex; gap: 7px; padding: 0 0 11px; overflow-x: auto; scrollbar-width: none; }
  .agent-strip button { flex: 0 0 auto; display: inline-flex; align-items: center; gap: 7px; padding: 7px 10px; border: 1px solid transparent; border-radius: 999px; color: var(--island-text-secondary); background: transparent; cursor: pointer; }
  .agent-strip button.active { color: var(--island-text-primary); border-color: var(--island-border-subtle); background: var(--island-surface-raised); }
  .agent-strip button:focus-visible { outline: 2px solid var(--island-accent-primary); }
  .mini-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--island-status-idle); }
  .mini-dot.status-working { background: var(--island-status-working); } .mini-dot.status-error { background: var(--island-status-error); } .mini-dot.status-done { background: var(--island-status-done); } .mini-dot.status-waiting { background: var(--island-status-waiting); } .mini-dot.status-stopped { background: var(--island-status-stopped); }
  .card-grid { display: grid; grid-template-columns: minmax(0, .85fr) minmax(0, 1.15fr); gap: 10px; }
  .refresh-note { margin: 0 0 10px; padding: 7px 10px; border-radius: 10px; color: var(--island-refresh-error-text); background: var(--island-refresh-error-surface); font-size: 12px; }
  .empty { display: grid; gap: 5px; padding: 28px 20px; text-align: center; color: var(--island-text-secondary); } .empty strong { color: var(--island-text-primary); }
  @media (max-width: 520px) { .card-grid { grid-template-columns: minmax(0, 1fr); } }
</style>
