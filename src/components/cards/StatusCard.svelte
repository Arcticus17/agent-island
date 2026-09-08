<script lang="ts">
  import type { AgentView, DisplayStatus, SessionView } from "../../bridge/types";
  let { agent, session }: { agent: AgentView; session: SessionView } = $props();
  const labels: Record<DisplayStatus, string> = { stopped: "已停止", idle: "空闲", working: "工作中", done: "已完成", error: "错误", waiting: "等待确认" };
</script>

<article class="card status-card">
  <span class="eyebrow">当前状态</span>
  <strong data-testid="expanded-status">{labels[agent.display_status]}</strong>
  <span class="sensitive" data-testid="status-session">{agent.name} · {session.id}</span>
</article>

<style>
  .card { min-width: 0; padding: 13px; border: 1px solid var(--island-border-subtle); border-radius: 16px; background: var(--island-surface-raised); }
  .status-card { display: grid; align-content: center; gap: 4px; }
  .eyebrow { color: var(--island-text-secondary); font-size: 10px; letter-spacing: .12em; text-transform: uppercase; }
  strong { color: var(--island-status-current); font-size: 22px; }
  .status-card > span:last-child { overflow: hidden; color: var(--island-text-secondary); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
</style>
