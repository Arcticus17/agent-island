<script lang="ts">
  import type { AgentView, DisplayStatus } from "../../bridge/types";
  let { agent, expanded, onToggle }: { agent: AgentView | null; expanded: boolean; onToggle: () => void } = $props();
  const labels: Record<DisplayStatus, string> = { stopped: "已停止", idle: "空闲", working: "工作中", done: "已完成", error: "错误", waiting: "等待确认" };
</script>

<button class="compact" type="button" data-testid="compact-toggle" aria-expanded={expanded} aria-controls="expanded-island" onclick={onToggle}>
  <span class="orb" aria-hidden="true"></span>
  <span class="identity"><strong>{agent?.name ?? "暂无 Agent"}</strong><span class="session">{agent?.active_session?.name ?? "当前会话未确认"}</span></span>
  <span class="status" role="status" data-testid="compact-status">{labels[agent?.display_status ?? "idle"]}</span>
  <span class="chevron" aria-hidden="true">{expanded ? "⌃" : "⌄"}</span>
</button>

<style>
  .compact { width: 100%; min-height: 58px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 11px; padding: 10px 14px; border: 0; color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .compact:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: -3px; }
  .orb { width: 12px; height: 12px; border-radius: 999px; background: var(--island-status-current); box-shadow: var(--island-status-glow); }
  .identity { min-width: 0; display: grid; gap: 1px; }
  strong, .session { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { font-size: 13px; letter-spacing: .01em; }
  .session { color: var(--island-text-secondary); font-size: 11px; }
  .status { color: var(--island-status-current); font-size: 12px; font-weight: 700; }
  .chevron { color: var(--island-text-secondary); font-size: 15px; }
  @media (max-width: 300px) { .compact { grid-template-columns: auto minmax(0, 1fr) auto; } .status { display: none; } }
</style>
