<script lang="ts">
  import type { InboxItem } from "../../bridge/types";

  interface Digest {
    minutes: number;
    total: number;
    approvals: number;
    done: number;
    notifications: number;
  }

  let {
    items,
    digest = null,
    onDismissDigest,
    onResolve,
    onSnooze,
    onOpenApproval,
  }: {
    items: readonly InboxItem[];
    digest?: Digest | null;
    onDismissDigest: () => void;
    onResolve: (id: string) => void;
    onSnooze: (id: string, minutes: number) => void;
    onOpenApproval: (id: string) => void;
  } = $props();

  const visible = $derived(items.filter((item) => item.state !== "resolved"));
  const shown = $derived(visible.slice(0, 6));
  const hiddenCount = $derived(visible.length - shown.length);
  const actionable = $derived(visible.filter((item) => item.kind !== "approval"));

  const icons: Record<string, string> = {
    approval: "⚠",
    notification: "!",
    done: "✓",
    question: "?",
    system: "·",
  };

  function ageLabel(atMs: number): string {
    const seconds = Math.max(0, Math.floor((Date.now() - atMs) / 1000));
    if (seconds < 60) return "刚刚";
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h`;
    return `${Math.floor(seconds / 86_400)}d`;
  }

  function snoozeLabel(item: InboxItem): string {
    if (item.state !== "snoozed" || !item.snooze_until_ms) return "";
    const date = new Date(item.snooze_until_ms);
    return `稍后 ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  }

  function openApproval(item: InboxItem): void {
    if (item.approval_id) onOpenApproval(item.approval_id);
  }
</script>

{#if visible.length > 0 || digest}
  <section class="inbox" data-testid="inbox-panel" aria-label="待你处理">
    {#if digest}
      <div class="digest" data-testid="inbox-digest">
        <div class="digest-copy">
          <strong>你离开的 {digest.minutes} 分钟</strong>
          <span>
            {digest.total} 件待处理
            {#if digest.approvals > 0} · {digest.approvals} 个审批{/if}
            {#if digest.done > 0} · {digest.done} 个完成{/if}
            {#if digest.notifications > 0} · {digest.notifications} 条通知{/if}
          </span>
        </div>
        <button type="button" data-testid="inbox-digest-dismiss" onclick={onDismissDigest}>收起摘要</button>
      </div>
    {/if}

    <div class="heading">
      <span class="label"><i class="accent"></i>待你处理<span class="count" data-testid="inbox-count">{visible.length}</span></span>
      {#if actionable.length > 0}
        <button type="button" class="link" data-testid="inbox-resolve-all" onclick={() => actionable.forEach((item) => onResolve(item.id))}>全部已读</button>
      {/if}
    </div>

    <div class="list">
      {#each shown as item (item.id)}
        <article
          class:hot={item.severity === "high" || item.severity === "critical"}
          data-inbox-id={item.id}
          data-inbox-kind={item.kind}
        >
          <span class="icon" data-kind={item.kind} aria-hidden="true">{icons[item.kind] ?? "·"}</span>
          <span class="content">
            <strong class="sensitive">{item.title}</strong>
            {#if item.body}<span class="body sensitive">{item.body}</span>{/if}
            <span class="meta">
              {item.agent_name || item.agent_id || "Agent"}
              {#if item.session_id} · <span class="sensitive">{item.session_id.slice(0, 8)}</span>{/if}
              · {ageLabel(item.created_at_ms)}
              {#if snoozeLabel(item)} · {snoozeLabel(item)}{/if}
            </span>
          </span>
          <span class="actions">
            {#if item.kind === "approval"}
              <button type="button" data-testid="inbox-open-approval" onclick={() => openApproval(item)}>查看</button>
            {:else}
              <button type="button" data-testid="inbox-snooze" onclick={() => onSnooze(item.id, 60)}>稍后</button>
              <button type="button" data-testid="inbox-resolve" onclick={() => onResolve(item.id)}>知道了</button>
            {/if}
          </span>
        </article>
      {/each}
      {#if hiddenCount > 0}
        <p class="more" data-testid="inbox-more">还有 {hiddenCount} 条待处理，先处理上面这些。</p>
      {/if}
    </div>
  </section>
{/if}

<style>
  .inbox { display: grid; gap: 8px; margin: 0 0 10px; padding: 0 0 10px; border-bottom: 1px solid var(--island-border-subtle); }
  .digest { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 10px 12px; border: 1px solid var(--island-border-subtle); border-radius: var(--island-radius-card); background: color-mix(in srgb, var(--island-accent-primary) 10%, var(--island-surface-card)); }
  .digest-copy { min-width: 0; display: grid; gap: 2px; }
  .digest-copy strong { font-size: 12px; color: var(--island-text-primary); }
  .digest-copy span { color: var(--island-text-secondary); font-size: 10px; }
  .digest button { flex: 0 0 auto; min-height: 26px; padding: 2px 9px; border: 1px solid var(--island-border-subtle); border-radius: var(--island-radius-control); color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 10px; cursor: pointer; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 0 2px; }
  .label { display: flex; align-items: center; gap: 7px; color: var(--island-text-secondary); font-size: 11px; font-weight: 650; letter-spacing: .3px; }
  .accent { width: 2px; height: 11px; border-radius: 2px; background: var(--island-accent-primary); }
  .count { min-width: 17px; height: 17px; padding: 0 5px; display: inline-grid; place-items: center; border-radius: 999px; color: var(--island-status-waiting); background: color-mix(in srgb, var(--island-status-waiting) 14%, transparent); font-size: 9.5px; font-variant-numeric: tabular-nums; }
  .link { border: 0; color: var(--island-text-tertiary); background: transparent; font-size: 10px; cursor: pointer; }
  .link:hover { color: var(--island-text-secondary); }
  .list { display: grid; gap: 6px; }
  .more { margin: 0; padding: 2px 4px; color: var(--island-text-tertiary); font-size: 9.5px; }
  article { display: grid; grid-template-columns: 24px minmax(0, 1fr) auto; align-items: center; gap: 8px; padding: 8px 9px; border: 1px solid var(--island-border-subtle); border-radius: 12px; background: var(--island-surface-card); }
  article.hot { border-color: color-mix(in srgb, var(--island-status-waiting) 38%, transparent); background: color-mix(in srgb, var(--island-status-waiting) 8%, var(--island-surface-card)); }
  .icon { width: 24px; height: 24px; display: grid; place-items: center; border-radius: 7px; color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 11px; }
  article.hot .icon, .icon[data-kind="approval"] { color: var(--island-status-waiting); background: color-mix(in srgb, var(--island-status-waiting) 14%, transparent); }
  .icon[data-kind="done"] { color: var(--island-status-done); }
  .content { min-width: 0; display: grid; gap: 1px; }
  strong, .body { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { color: var(--island-text-primary); font-size: 11px; }
  .body { color: var(--island-text-secondary); font-size: 10px; }
  .meta { color: var(--island-text-tertiary); font-size: 9px; }
  .actions { display: flex; gap: 5px; }
  .actions button { min-height: 26px; padding: 2px 8px; border: 1px solid var(--island-border-subtle); border-radius: 7px; color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 9.5px; cursor: pointer; }
  .actions button:hover { color: var(--island-text-primary); background: var(--island-control-hover); }
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
</style>
