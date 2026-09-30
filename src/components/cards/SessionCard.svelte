<script lang="ts">
  import type { SessionView } from "../../bridge/types";
  let { session, privacy }: { session: SessionView; privacy: boolean } = $props();
</script>

<article class="card session-card">
  <span class="eyebrow">{session.lifecycle === "Historical" ? "本地会话记录" : "活动会话"}</span>
  <strong class="sensitive" data-testid="active-session-id">{session.id}</strong>
  <span class="path sensitive" data-testid="session-path" title={privacy ? undefined : (session.cwd ?? "")}>{session.cwd ?? "未提供工作目录"}</span>
  {#if session.current_file}<span class="file sensitive" title={privacy ? undefined : session.current_file}>{session.current_file}</span>{/if}
</article>

<style>
  .card { min-width: 0; display: grid; gap: 5px; padding: 13px; border: 1px solid var(--island-border-subtle); border-radius: 16px; background: var(--island-surface-raised); }
  .session-card { grid-column: 1 / -1; }
  .eyebrow { color: var(--island-text-secondary); font-size: 10px; letter-spacing: .12em; text-transform: uppercase; }
  strong, .path, .file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } strong { font-size: 13px; }
  .path { color: var(--island-text-primary); font-size: 12px; } .file { color: var(--island-text-secondary); font-size: 11px; }
</style>
