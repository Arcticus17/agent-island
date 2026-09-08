<script lang="ts">
  import type { ApprovalItem } from "../events/types";

  let {
    items,
    onRespond,
  }: {
    items: readonly ApprovalItem[];
    onRespond: (id: string, allow: boolean) => void;
  } = $props();
</script>

<div class="stack approvals" data-testid="approval-stack" aria-label="待处理审批" aria-live="assertive">
  {#each items as item (item.approval.id)}
    <article class="approval" data-approval-id={item.approval.id} aria-busy={item.state === "pending"}>
      <div class="heading">
        <span class="badge">需要审批</span>
        <span class="tool">{item.approval.tool || "未知工具"}</span>
      </div>
      <code class="sensitive">{item.approval.command || "未提供命令"}</code>
      <span class="context sensitive">{item.approval.cwd || "未提供目录"} · {item.approval.session.slice(0, 8) || "无会话"}</span>
      {#if item.error}
        <p class="error sensitive" role="alert"><strong>{item.error.code}</strong> · {item.error.message}</p>
      {/if}
      <div class="actions">
        <button type="button" data-testid="approval-deny" disabled={item.state === "pending" || (item.state === "failed" && !item.error?.retryable)} onclick={() => onRespond(item.approval.id, false)}>
          {item.state === "pending" && item.decision === false ? "拒绝中…" : item.state === "failed" && item.decision === false && item.error?.retryable ? "重试拒绝" : "拒绝"}
        </button>
        <button class="allow" type="button" data-testid="approval-allow" disabled={item.state === "pending" || (item.state === "failed" && !item.error?.retryable)} onclick={() => onRespond(item.approval.id, true)}>
          {item.state === "pending" && item.decision === true ? "允许中…" : item.state === "failed" && item.decision === true && item.error?.retryable ? "重试允许" : "允许"}
        </button>
      </div>
    </article>
  {/each}
</div>

<style>
  .stack { display: grid; gap: 8px; }
  .approval { display: grid; gap: 8px; padding: 12px; border: 1px solid var(--island-event-border); border-radius: 15px; color: var(--island-text-primary); background: var(--island-event-surface); box-shadow: var(--island-event-shadow); pointer-events: auto; }
  .heading, .actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .badge { color: var(--island-status-waiting); font-size: 11px; font-weight: 800; }
  .tool, .context { min-width: 0; color: var(--island-text-secondary); font-size: 10px; overflow-wrap: anywhere; }
  code { overflow: hidden; padding: 8px; border-radius: 9px; color: var(--island-log-text); background: var(--island-log-surface); font: 500 11px/1.45 ui-monospace, "Cascadia Code", monospace; overflow-wrap: anywhere; }
  .error { margin: 0; color: var(--island-refresh-error-text); font-size: 11px; }
  button { min-height: 36px; padding: 5px 11px; border: 1px solid var(--island-border-subtle); border-radius: 9px; color: var(--island-text-primary); background: var(--island-control-surface); cursor: pointer; }
  button.allow { border-color: var(--island-accent-primary); color: var(--island-accent-primary); }
  button:disabled { opacity: .55; cursor: wait; }
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
</style>
