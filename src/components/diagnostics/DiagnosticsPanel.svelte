<script lang="ts">
  import { onDestroy } from "svelte";
  import type { DiagnosticView } from "../../bridge/types";

  type ExportState = "idle" | "pending" | "succeeded" | "failed";

  let {
    issues,
    status = "ready",
    exportDiagnostics,
    onLayoutChange = () => {},
  }: {
    issues: readonly DiagnosticView[];
    status?: "loading" | "ready" | "failed";
    exportDiagnostics: (destination: string) => Promise<void>;
    onLayoutChange?: () => void;
  } = $props();

  let destination = $state("");
  let exportState = $state<ExportState>("idle");
  let exportRequest = 0;
  let disposed = false;

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
      <span class="eyebrow">SYSTEM HEALTH</span>
      <h2 id="diagnostics-title">诊断信息</h2>
    </div>
    <span class:clear={status === "ready" && issues.length === 0} class:unavailable={status === "failed"} class="issue-count">
      {status === "loading"
        ? "正在检查"
        : status === "failed"
          ? "暂不可用"
          : issues.length === 0
            ? "状态正常"
            : `${issues.length} 项提示`}
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
  .diagnostics {
    display: grid;
    gap: 11px;
    margin: 0 12px 12px;
    padding: 13px;
    border: 1px solid var(--island-border-subtle);
    border-radius: 16px;
    color: var(--island-text-primary);
    background: var(--island-surface-raised);
  }

  header,
  .issue-heading,
  .export-row {
    display: flex;
    align-items: center;
  }

  header {
    justify-content: space-between;
    gap: 12px;
  }

  .eyebrow {
    display: block;
    margin-bottom: 2px;
    color: var(--island-text-secondary);
    font-size: 9px;
    letter-spacing: .13em;
  }

  h2 {
    margin: 0;
    font-size: 14px;
    line-height: 1.2;
  }

  .issue-count {
    flex: 0 0 auto;
    padding: 4px 8px;
    border-radius: 999px;
    color: var(--island-status-waiting);
    background: color-mix(in srgb, var(--island-status-waiting) 12%, transparent);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .issue-count.clear {
    color: var(--island-status-done);
    background: color-mix(in srgb, var(--island-status-done) 12%, transparent);
  }

  .issue-count.unavailable {
    color: var(--island-refresh-error-text);
    background: var(--island-refresh-error-surface);
  }

  .issue-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    max-height: 190px;
    overflow-y: auto;
    padding-right: 2px;
    overscroll-behavior: contain;
    scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track);
  }

  .issue {
    min-width: 0;
    padding: 10px;
    border: 1px solid var(--island-border-subtle);
    border-radius: 12px;
    background: color-mix(in srgb, var(--island-control-surface) 82%, transparent);
  }

  .issue-heading {
    min-width: 0;
    gap: 7px;
  }

  .adapter {
    flex: 0 0 auto;
    padding: 2px 6px;
    border-radius: 6px;
    color: var(--island-accent-primary);
    background: color-mix(in srgb, var(--island-accent-primary) 12%, transparent);
    font-size: 10px;
    font-weight: 700;
  }

  strong {
    min-width: 0;
    overflow: hidden;
    font: 600 11px/1.3 ui-monospace, "Cascadia Mono", Consolas, monospace;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .freshness {
    flex: 0 0 auto;
    margin-left: auto;
    color: var(--island-status-done);
    font-size: 9px;
  }

  .freshness.stale { color: var(--island-status-waiting); }

  dl {
    display: flex;
    flex-wrap: wrap;
    gap: 5px 12px;
    margin: 8px 0 0;
  }

  dl > div {
    display: inline-flex;
    min-width: 0;
    gap: 4px;
    font-size: 10px;
  }

  dt { color: var(--island-text-secondary); }
  dd {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    margin: 0;
    color: var(--island-text-secondary);
    font-size: 11px;
  }

  .unavailable-copy { color: var(--island-refresh-error-text); }

  .export {
    display: grid;
    gap: 6px;
    padding-top: 10px;
    border-top: 1px solid var(--island-border-subtle);
  }

  label {
    color: var(--island-text-secondary);
    font-size: 10px;
  }

  .export-row { gap: 7px; }

  input,
  button {
    min-height: 34px;
    border: 1px solid var(--island-border-subtle);
    border-radius: 9px;
    color: var(--island-text-primary);
    background: var(--island-control-surface);
    font: inherit;
    font-size: 11px;
  }

  input {
    min-width: 0;
    flex: 1 1 auto;
    padding: 6px 9px;
  }

  input::placeholder { color: var(--island-text-secondary); opacity: .72; }

  button {
    flex: 0 0 auto;
    padding: 6px 11px;
    cursor: pointer;
    transition:
      background var(--duration-normal) var(--motion-spring),
      border-color var(--duration-normal) var(--motion-spring),
      transform var(--duration-fast) var(--motion-spring);
  }

  button:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--island-accent-primary) 42%, transparent);
    background: var(--island-control-hover);
  }

  button:active:not(:disabled) { transform: scale(.97); }
  button:disabled,
  input:disabled { cursor: default; opacity: .48; }
  input:focus-visible,
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }

  .feedback {
    margin: 0;
    color: var(--island-text-secondary);
    font-size: 10px;
  }

  .feedback.success { color: var(--island-status-done); }
  .feedback.failure { color: var(--island-refresh-error-text); }

  @media (max-width: 520px) {
    .issue-list { grid-template-columns: minmax(0, 1fr); }
  }

  @media (max-width: 380px) {
    .export-row { align-items: stretch; flex-direction: column; }
    button { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>
