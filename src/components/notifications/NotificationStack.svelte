<script lang="ts">
  import type { NotificationItem } from "../events/types";
  let { items, onDismiss, onPause }: { items: readonly NotificationItem[]; onDismiss: (id: string) => void; onPause: (id: string, paused: boolean) => void } = $props();
</script>

<div class="stack notifications" data-testid="notification-stack" aria-live="polite" aria-label="Agent 通知">
  {#each items as item (item.id)}
    <article
      class="notification"
      data-notification-id={item.id}
      onmouseenter={() => onPause(item.id, true)}
      onmouseleave={() => onPause(item.id, false)}
      onfocusin={() => onPause(item.id, true)}
      onfocusout={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onPause(item.id, false); }}
    >
      <span class="dot" aria-hidden="true"></span>
      <span class="content sensitive"><strong>{item.agentName}{item.count > 1 ? ` ×${item.count}` : ""}</strong><span>{item.message}</span></span>
      <span class="meta"><span class="session sensitive">{item.sessionId.slice(0, 8)}</span><button type="button" aria-label="关闭通知" onclick={() => onDismiss(item.id)}>×</button></span>
    </article>
  {/each}
</div>

<style>
  .stack { display: grid; gap: 7px; margin-top: 7px; }
  .notification { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 9px; padding: 10px 12px; border: 1px solid var(--island-border-subtle); border-radius: 13px; color: var(--island-text-primary); background: var(--island-notification-surface); box-shadow: var(--island-event-shadow); pointer-events: auto; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--island-status-waiting); }
  .content { min-width: 0; display: grid; gap: 1px; }
  strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .content span { overflow-wrap: anywhere; }
  strong { font-size: 11px; } .content span, .session { color: var(--island-text-secondary); font-size: 10px; }
  .meta { display: flex; align-items: center; gap: 6px; }
  button { width: 28px; height: 28px; padding: 0; border: 1px solid var(--island-border-subtle); border-radius: 8px; color: var(--island-text-secondary); background: var(--island-control-surface); cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
</style>
