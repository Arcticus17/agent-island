<script lang="ts">
  import type { AgentView, UsageInfo } from "../../bridge/types";
  import { formatUsd } from "../../usage/spend";
  let {
    agent,
    refreshFailed,
    todaySpend = 0,
    budget = 0,
  }: {
    agent: AgentView;
    refreshFailed: boolean;
    /** 今日花费（岛在线期间累计，所有 Agent 合计） */
    todaySpend?: number;
    /** 每日预算，0 表示没设 */
    budget?: number;
  } = $props();

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
  const spendLabel = $derived(todaySpend > 0 || budget > 0
    ? `今日 ${formatUsd(todaySpend)}${budget > 0 ? ` / 预算 ${formatUsd(budget)}` : ""}`
    : "");
  const spendState = $derived(budget > 0 ? (todaySpend > budget ? "over" : todaySpend >= budget * 0.8 ? "near" : "ok") : "off");
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

<article class="card usage-card" aria-label="用量">
  <div class="line">
    <span class="key">用量</span>
    {#if usage}
      <span class="val" data-testid="usage-total">{formatTokens(usage.tokens_total)}</span>
      <span class="unit">tokens</span>
      {#if usage.used_percent !== null}
        <span class="meter" aria-label={`额度已使用 ${usage.used_percent.toFixed(1)}%`}><i style={`width: ${Math.min(100, Math.max(0, usage.used_percent))}%`}></i></span>
      {/if}
    {:else}
      <span class="val unavailable">用量尚未提供</span>
    {/if}
  </div>
  {#if usage}<span class="detail" data-testid="usage-detail">{detail || "暂无更多用量明细"}</span>{/if}
  <div class="foot">
    {#if spendLabel}<span class="spend" data-testid="usage-spend" data-budget-state={spendState} title="岛在线期间累计的增量，关闭应用的时间不计">{spendLabel}</span>{/if}
    <span class="freshness" data-testid="usage-freshness">{freshness}</span>
  </div>
</article>

<style>
  .card { min-width: 0; display: grid; gap: 4px; align-content: center; }
  .line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .key { flex: 0 0 auto; width: 30px; color: var(--ink-3); font-size: var(--fs-2xs); letter-spacing: var(--tracking-label); }
  .val { color: var(--ink); font: 600 20px/1.1 var(--font-readout); font-variant-numeric: tabular-nums; letter-spacing: -.01em; }
  .val.unavailable { font: 600 var(--fs-sm)/1.4 var(--font-ui); color: var(--ink-2); }
  .unit { color: var(--ink-3); font-size: var(--fs-2xs); }
  .meter { flex: 1 1 auto; min-width: 24px; height: 3px; margin-left: 2px; border-radius: 2px; background: var(--rule); overflow: hidden; }
  .meter i { display: block; height: 100%; background: var(--state-working); border-radius: inherit; }
  .detail {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--fs-2xs);
    line-height: 1.5;
  }
  .foot { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; min-width: 0; }
  .spend { color: var(--ink-2); font-size: var(--fs-2xs); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .spend[data-budget-state="near"] { color: var(--state-waiting); }
  .spend[data-budget-state="over"] { color: var(--state-error); font-weight: 700; }
  .freshness { color: var(--ink-3); font-size: var(--fs-2xs); white-space: nowrap; }
</style>
