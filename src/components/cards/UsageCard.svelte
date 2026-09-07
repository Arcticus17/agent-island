<script lang="ts">
  import type { AgentView, UsageInfo } from "../../bridge/types";
  let { agent, refreshFailed }: { agent: AgentView; refreshFailed: boolean } = $props();

  function formatTokens(value: number): string {
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
    if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
    return String(value);
  }

  function usageDetails(usage: UsageInfo): string {
    const details: string[] = [];
    if (usage.tokens_output > 0) details.push(`输出 ${formatTokens(usage.tokens_output)}`);
    if (usage.cost_usd !== null) details.push(`$${usage.cost_usd.toFixed(2)}`);
    if (usage.used_percent !== null) details.push(`额度 ${usage.used_percent.toFixed(1)}%`);
    if (usage.resets_at_secs !== null) {
      const remainingSeconds = Math.max(0, usage.resets_at_secs - Date.now() / 1_000);
      const resetIn = remainingSeconds < 3_600
        ? `${Math.max(1, Math.floor(remainingSeconds / 60))}分钟`
        : remainingSeconds < 86_400
          ? `${Math.floor(remainingSeconds / 3_600)}小时`
          : `${Math.floor(remainingSeconds / 86_400)}天`;
      details.push(`${resetIn}后重置`);
    }
    if (usage.unlimited === true) details.push("无限额度");
    else if (usage.credits !== null) details.push(`余额 $${usage.credits.toFixed(2)}`);
    return details.join(" · ");
  }

  const usage = $derived(agent.usage);
  const detail = $derived(usage ? usageDetails(usage) : "");
  const freshness = $derived(
    usage === null
      ? refreshFailed
        ? "刷新失败，暂无缓存用量"
        : "暂无用量数据"
      : refreshFailed
        ? "保留上次用量"
        : usage.stale || agent.freshness.stale
          ? "用量可能已过期"
          : "用量已同步",
  );
</script>

<article class="card">
  <span class="eyebrow">用量</span>
  {#if usage}
    <div class="metric"><strong data-testid="usage-total">{formatTokens(usage.tokens_total)}</strong><span>tokens</span></div>
    {#if usage.used_percent !== null}
      <div class="meter" aria-label={`额度已使用 ${usage.used_percent.toFixed(1)}%`}><span style={`width: ${Math.min(100, Math.max(0, usage.used_percent))}%`}></span></div>
    {/if}
    <span class="detail" data-testid="usage-detail">{detail || "暂无更多用量明细"}</span>
  {:else}
    <strong class="unavailable">用量尚未提供</strong>
  {/if}
  <span class="freshness" data-testid="usage-freshness">{freshness}</span>
</article>

<style>
  .card { min-width: 0; display: grid; gap: 7px; padding: 13px; border: 1px solid var(--island-border-subtle); border-radius: 16px; background: var(--island-surface-raised); }
  .eyebrow { color: var(--island-text-secondary); font-size: 10px; letter-spacing: .12em; text-transform: uppercase; }
  .metric { display: flex; align-items: baseline; gap: 7px; } .metric strong { color: var(--island-accent-primary); font-size: 22px; }
  .metric span, .detail, .freshness { color: var(--island-text-secondary); font-size: 11px; }
  .detail { overflow-wrap: anywhere; }
  .freshness { justify-self: end; }
  .unavailable { color: var(--island-text-primary); font-size: 13px; }
  .meter { height: 4px; overflow: hidden; border-radius: 999px; background: var(--island-border-subtle); }
  .meter span { display: block; height: 100%; border-radius: inherit; background: var(--island-status-current); }
</style>
