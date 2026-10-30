<script lang="ts">
  import { onDestroy } from "svelte";
  import type { DiagnosticView, SourceHealth } from "../../bridge/types";

  type ExportState = "idle" | "pending" | "succeeded" | "failed";

  let {
    issues,
    sources = [],
    status = "ready",
    exportDiagnostics,
    onLayoutChange = () => {},
  }: {
    issues: readonly DiagnosticView[];
    sources?: readonly SourceHealth[];
    status?: "loading" | "ready" | "failed";
    exportDiagnostics: (destination: string) => Promise<void>;
    onLayoutChange?: () => void;
  } = $props();

  let destination = $state("");
  let exportState = $state<ExportState>("idle");
  let exportRequest = 0;
  let disposed = false;

  let sourcesHealthy = $derived(sources.every((entry) => entry.status === "ok"));

  function sourceLabel(source: string): string {
    if (source === "claude_usage") return "Claude 用量";
    if (source === "codex_usage") return "Codex 用量";
    return source;
  }

  function sourceStatusLabel(status: string): string {
    switch (status) {
      case "ok":
        return "正常";
      case "no_records":
        return "未找到记录";
      case "no_usage_events":
        return "未解析到用量事件";
      case "unavailable":
        return "不可用";
      default:
        return status;
    }
  }

  onDestroy(() => {
    disposed = true;
    exportRequest += 1;
  });

  async function exportReport(): Promise<void> {
    if (exportState === "pending" || destination.trim().length === 0) return;

    const request = ++exportRequest;
    const requestedDestination = destination;
    exportState = "pending";
    onLayoutChange();

    try {
      await exportDiagnostics(requestedDestination);
      if (disposed || request !== exportRequest) return;
      exportState = "succeeded";
    } catch {
      if (disposed || request !== exportRequest) return;
      exportState = "failed";
    }
    onLayoutChange();
  }

  function resetFeedback(): void {
    if (exportState !== "pending" && exportState !== "idle") {
      exportState = "idle";
      onLayoutChange();
    }
  }
</script>

<section class="diagnostics" aria-labelledby="diagnostics-title">
  <header>
    <div>
      <h2 id="diagnostics-title">诊断信息</h2>
    </div>
    <span class:clear={status === "ready" && issues.length === 0 && sourcesHealthy} class:unavailable={status === "failed"} class="issue-count">
      {status === "loading"
        ? "正在检查"
        : status === "failed"
          ? "暂不可用"
          : issues.length > 0
            ? `${issues.length} 项提示`
            : sourcesHealthy
              ? "状态正常"
              : "数据源需注意"}
    </span>
  </header>

  {#if status === "failed"}
    <p class="empty unavailable-copy" data-testid="diagnostics-state" role="alert">
      诊断暂不可用 · diagnostics_fetch_failed
    </p>
  {:else if status === "loading"}
    <p class="empty" data-testid="diagnostics-state" role="status">正在读取诊断信息…</p>
  {:else if issues.length > 0}
    <div class="issue-list" aria-label="诊断问题列表">
      {#each issues as issue, index (`${issue.adapter}:${issue.code}:${index}`)}
        <article class="issue" data-testid="diagnostic-issue">
          <div class="issue-heading">
            <span class="adapter">{issue.adapter}</span>
            <strong>{issue.code}</strong>
            <span class:stale={issue.freshness.stale} class="freshness">
              {issue.freshness.stale ? "数据已过期" : "当前数据"}
            </span>
          </div>
          <dl>
            <div><dt>事件类型</dt><dd>{issue.event_type ?? "未识别"}</dd></div>
            <div><dt>事件</dt><dd>{issue.event_count}</dd></div>
            <div><dt>消息</dt><dd>{issue.message_count}</dd></div>
            {#if issue.skipped_lines !== undefined}
              <div><dt>跳过行</dt><dd>{issue.skipped_lines}</dd></div>
            {/if}
            {#if issue.candidate_count !== undefined}
              <div><dt>候选会话</dt><dd>{issue.candidate_count}</dd></div>
            {/if}
          </dl>
        </article>
      {/each}
    </div>
  {:else}
    <p class="empty">当前没有可报告的适配器问题。</p>
  {/if}

  {#if sources.length > 0}
    <div class="source-list" aria-label="数据源健康">
      {#each sources as entry (entry.source)}
        <div class="source" data-testid="diagnostics-source">
          <div class="source-heading">
            <strong>{sourceLabel(entry.source)}</strong>
            <span class="source-records">{entry.records} 个记录文件</span>
            <span class:warn={entry.status !== "ok"} class="source-status">{sourceStatusLabel(entry.status)}</span>
          </div>
          {#if entry.status === "no_usage_events"}
            <p class="source-hint">有记录但未解析到用量事件：可能暂无近期活动，也可能是记录格式已变化。</p>
          {:else if entry.status === "unavailable"}
            <p class="source-hint">无法读取该数据源，用量卡可能为空。</p>
          {:else if entry.status === "no_records"}
            <p class="source-hint">数据目录存在但没有会话记录，用量卡保持空。</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <form class="export" onsubmit={(event) => { event.preventDefault(); void exportReport(); }}>
    <label for="diagnostics-destination">导出脱敏报告</label>
    <div class="export-row">
      <input
        id="diagnostics-destination"
        class="sensitive"
        data-testid="diagnostics-export-path"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder="例如 D:\exports\diagnostics.json"
        bind:value={destination}
        oninput={resetFeedback}
        disabled={exportState === "pending"}
      />
      <button
        data-testid="diagnostics-export"
        type="submit"
        disabled={exportState === "pending"}
      >
        {exportState === "pending" ? "正在导出…" : "导出 JSON"}
      </button>
    </div>
    {#if exportState === "succeeded"}
      <p class="feedback success" data-testid="diagnostics-export-feedback" role="status">
        导出成功
      </p>
    {:else if exportState === "failed"}
      <p class="feedback failure" data-testid="diagnostics-export-feedback" role="alert">
        export_failed · 无法导出诊断报告
      </p>
    {:else if exportState === "pending"}
      <p class="feedback" data-testid="diagnostics-export-feedback" role="status">
        正在生成脱敏报告…
      </p>
    {/if}
  </form>
</section>

<style>
  /* 诊断：系统区里的一段读数，不套卡片；问题与数据源都是发丝分隔的行 */
  .diagnostics { display: grid; gap: 8px; padding: 0 0 10px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  header > div { min-width: 0; display: grid; gap: 2px; }
  h2 { margin: 0; color: var(--ink-2); font-size: var(--fs-xs); font-weight: 650; }
  .issue-count {
    flex: 0 0 auto;
    padding: 1px 6px;
    border-radius: 3px;
    color: var(--ink-3);
    background: var(--field);
    font: 600 var(--fs-2xs)/1.5 var(--font-readout);
    white-space: nowrap;
  }
  .issue-count.clear { color: var(--state-done); background: color-mix(in srgb, var(--state-done) 12%, transparent); }
  .issue-count.unavailable { color: var(--state-error); background: color-mix(in srgb, var(--state-error) 12%, transparent); }

  .issue-list, .source-list { display: grid; gap: 0; }
  .issue, .source { display: grid; gap: 4px; padding: 7px 0; border-top: 1px solid var(--rule); }
  .issue-heading, .source-heading { display: flex; align-items: baseline; flex-wrap: wrap; gap: 8px; min-width: 0; }
  .adapter {
    padding: 0 4px;
    border-radius: 3px;
    color: var(--ink-3);
    background: var(--field);
    font: 600 9px/1.6 var(--font-readout);
    letter-spacing: .04em;
    text-transform: uppercase;
  }
  .issue-heading strong { color: var(--ink-2); font: 500 var(--fs-2xs)/1.5 var(--font-mono); overflow-wrap: anywhere; }
  .freshness { color: var(--ink-3); font-size: var(--fs-2xs); }
  .freshness.stale { color: var(--state-waiting); }

  dl { margin: 0; display: flex; flex-wrap: wrap; gap: 4px 14px; }
  dl div { display: flex; align-items: baseline; gap: 4px; }
  dt { color: var(--ink-3); font-size: var(--fs-2xs); }
  dd { margin: 0; color: var(--ink-2); font: 500 var(--fs-2xs)/1.5 var(--font-readout); font-variant-numeric: tabular-nums; }

  .empty { margin: 0; padding: 4px 0; color: var(--ink-3); font-size: var(--fs-2xs); }
  .unavailable-copy { color: var(--state-error); }

  .source-heading strong { color: var(--ink-2); font-size: var(--fs-2xs); font-weight: 650; }
  .source-records, .source-status { color: var(--ink-3); font-size: var(--fs-2xs); }
  .source-status { margin-left: auto; }
  .source-status.warn { color: var(--state-waiting); }
  .source-hint { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); line-height: 1.6; }

  .export { display: grid; gap: 5px; padding-top: 8px; border-top: 1px solid var(--rule); }
  .export label { color: var(--ink-3); font-size: var(--fs-2xs); }
  .export-row { display: flex; align-items: center; gap: 6px; }
  .export-row input {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 26px;
    padding: 3px 7px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink);
    background: var(--field);
    font: 400 var(--fs-2xs)/1.5 var(--font-mono);
  }
  .export-row input::placeholder { color: var(--ink-3); }
  .export-row input:focus-visible { outline: 2px solid var(--ink); outline-offset: 1px; }
  .export-row button {
    flex: 0 0 auto;
    min-height: 26px;
    padding: 3px 9px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .export-row button:hover:not(:disabled) { color: var(--ink); border-color: var(--rule-strong); }
  .export-row button:disabled { opacity: .5; cursor: progress; }
  .feedback { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); overflow-wrap: anywhere; }
  .feedback.success { color: var(--state-done); }
  .feedback.failure { color: var(--state-error); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }

  @media (max-width: 420px) {
    .export-row { align-items: stretch; flex-direction: column; }
  }
</style>
