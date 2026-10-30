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
  const approvalsWaiting = $derived(visible.filter((item) => item.kind === "approval").length);

  const kindLabels: Record<string, string> = {
    approval: "审批",
    notification: "通知",
    done: "完成",
    question: "问题",
    system: "系统",
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
  <section class="inbox" data-testid="inbox-panel" aria-label="待你处理" data-alarm={approvalsWaiting > 0 ? "true" : "false"}>
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
        <button type="button" class="link" data-testid="inbox-digest-dismiss" onclick={onDismissDigest}>收起摘要</button>
      </div>
    {/if}

    <div class="heading">
      <span class="title">待你处理<span class="count" data-testid="inbox-count">{visible.length}</span></span>
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
          <span class="kind" data-kind={item.kind}>{kindLabels[item.kind] ?? "系统"}</span>
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
              <button type="button" class="action" data-testid="inbox-open-approval" onclick={() => openApproval(item)}>查看</button>
            {:else}
              <button type="button" class="action" data-testid="inbox-snooze" onclick={() => onSnooze(item.id, 60)}>稍后</button>
              <button type="button" class="action" data-testid="inbox-resolve" onclick={() => onResolve(item.id)}>知道了</button>
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
  /*
   * 「待你处理」是整块板里唯一会被整片点亮的区域（有审批时）。
   * 行与行之间只有发丝线，动作贴在行尾，不需要再包一层卡片。
   */
  .inbox { display: grid; gap: 0; margin: 0 0 8px; border: 1px solid var(--rule); border-radius: var(--radius-event); overflow: hidden; }
  .inbox[data-alarm="true"] { border-color: var(--alarm); }

  .digest {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    background: var(--alarm);
    color: var(--alarm-ink);
  }
  .digest-copy { min-width: 0; display: grid; gap: 1px; }
  .digest-copy strong { font-size: var(--fs-xs); font-weight: 650; }
  .digest-copy span { color: color-mix(in srgb, var(--alarm-ink) 78%, transparent); font-size: var(--fs-2xs); }
  .digest .link { color: var(--alarm-ink); }
  .digest .link:hover { color: var(--alarm-ink); text-decoration-color: currentColor; }

  .heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 7px 10px; }
  .title { display: inline-flex; align-items: center; gap: 6px; color: var(--ink-2); font-size: var(--fs-xs); font-weight: 650; }
  .inbox[data-alarm="true"] .title { color: var(--state-waiting); }
  .count {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    display: inline-grid;
    place-items: center;
    border-radius: 3px;
    color: var(--alarm-ink);
    background: var(--alarm);
    font: 600 10px/1 var(--font-readout);
    font-variant-numeric: tabular-nums;
  }
  .link { padding: 0; border: 0; color: var(--ink-3); background: none; font-size: var(--fs-2xs); text-decoration: underline; text-underline-offset: 2px; text-decoration-color: var(--rule-strong); cursor: pointer; }
  .link:hover { color: var(--ink); text-decoration-color: var(--ink-2); }

  .list { display: grid; gap: 0; }
  article {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    padding: 7px 10px;
    border-top: 1px solid var(--rule);
  }
  article.hot { background: color-mix(in srgb, var(--alarm) 8%, transparent); }
  .kind { color: var(--ink-3); font: 600 9px/1.5 var(--font-readout); letter-spacing: .08em; text-transform: uppercase; }
  .kind[data-kind="approval"] { color: var(--state-waiting); }
  .kind[data-kind="done"] { color: var(--state-done); }
  .kind[data-kind="question"] { color: var(--state-working); }
  .content { min-width: 0; display: grid; gap: 1px; }
  strong, .body { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { color: var(--ink); font-size: var(--fs-xs); font-weight: 600; }
  .body { color: var(--ink-2); font-size: var(--fs-2xs); }
  .meta { color: var(--ink-3); font: 400 var(--fs-2xs)/1.5 var(--font-mono); }
  .actions { display: flex; gap: 4px; }
  .action {
    min-height: 22px;
    padding: 2px 7px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .action:hover { color: var(--ink); border-color: var(--rule-strong); background: var(--field); }
  .more { margin: 0; padding: 6px 10px; border-top: 1px solid var(--rule); color: var(--ink-3); font-size: var(--fs-2xs); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
