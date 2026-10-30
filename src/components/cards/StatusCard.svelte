<script lang="ts">
  import type { AgentView, DisplayStatus, SessionView } from "../../bridge/types";
  let { agent, session, inspectingHistory = false }: { agent: AgentView; session: SessionView | null; inspectingHistory?: boolean } = $props();
  const labels: Record<DisplayStatus, string> = { stopped: "已停止", idle: "空闲", working: "工作中", done: "已完成", error: "错误", waiting: "等待确认" };
</script>

<article class="card status-card" data-state={agent.display_status} aria-label="当前状态">
  <div class="line">
    <span class="key">状态</span>
    <span class="val" data-testid="expanded-status">{inspectingHistory ? "本地记录" : labels[agent.display_status]}</span>
  </div>
  <span class="note sensitive" data-testid="status-session">{agent.name} · {session?.id ?? "会话尚未确认"}</span>
</article>

<style>
  .card { min-width: 0; display: grid; gap: 3px; align-content: center; }
  .line { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
  .key { flex: 0 0 auto; width: 30px; color: var(--ink-3); font-size: var(--fs-2xs); letter-spacing: var(--tracking-label); }
  .val { min-width: 0; color: var(--state-idle); font-size: 19px; font-weight: 650; letter-spacing: .01em; }
  .status-card[data-state="working"] .val { color: var(--state-working); }
  .status-card[data-state="done"] .val { color: var(--state-done); }
  .status-card[data-state="waiting"] .val { color: var(--state-waiting); }
  .status-card[data-state="error"] .val { color: var(--state-error); }
  .status-card[data-state="stopped"] .val { color: var(--state-stopped); }
  .note { overflow: hidden; color: var(--ink-3); font-size: var(--fs-2xs); text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 420px) { .key { width: 26px; } }
</style>
