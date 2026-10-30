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
      <span class="meta"><span class="session sensitive">{item.sessionId.slice(0, 8)}</span><button type="button" aria-label="关闭通知" onclick={() => onDismiss(item.id)}><svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" /></svg></button></span>
    </article>
  {/each}
</div>

<style>
  /* 通知：与板同高的窄条，左侧一个状态方标；悬停暂停计时（行为不变） */
  .stack { display: grid; gap: 6px; margin-top: 6px; }
  .notification {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    padding: 8px 10px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-event);
    color: var(--ink);
    background: var(--ground-raised);
    pointer-events: auto;
  }
  .dot { width: 5px; height: 5px; border-radius: 1px; background: var(--state-waiting); }
  .content { min-width: 0; display: grid; gap: 1px; }
  strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-xs); font-weight: 650; }
  .content span { overflow-wrap: anywhere; }
  .content span, .session { color: var(--ink-2); font-size: var(--fs-2xs); }
  .meta { display: flex; align-items: center; gap: 6px; }
  button {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--ink-3);
    background: transparent;
    font-size: 13px;
    line-height: 1;
    cursor: pointer;
  }
  button:hover { color: var(--ink); background: var(--field); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
