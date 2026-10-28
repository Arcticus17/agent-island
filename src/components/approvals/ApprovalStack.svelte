<script lang="ts">
  import type { ApprovalItem } from "../events/types";
  import type { ApprovalScope } from "../../bridge/types";
  import { suggestRulePattern } from "../../approvals/rules";

  let {
    items,
    onRespond,
    onBatchAllowLowRisk,
    onRemember,
  }: {
    items: readonly ApprovalItem[];
    onRespond: (id: string, allow: boolean, scope?: ApprovalScope, reason?: string) => void;
    /** 只在有多条低风险审批时显示；由 App 逐条发送。 */
    onBatchAllowLowRisk?: () => void;
    /** 把这条命令加进允许清单（可选：宿主不支持规则时不给按钮） */
    onRemember?: (tool: string, pattern: string) => void;
  } = $props();

  let scopeFor = $state<Record<string, ApprovalScope>>({});
  let reasonOpen = $state<string | null>(null);
  let reasonText = $state<Record<string, string>>({});

  const lowRiskCount = $derived(items.filter((item) => item.approval.risk === "low" && item.state !== "pending").length);

  function scopeOf(id: string): ApprovalScope {
    return scopeFor[id] ?? "once";
  }

  function setScope(id: string, value: string): void {
    scopeFor = { ...scopeFor, [id]: value as ApprovalScope };
  }

  function toggleReason(id: string): void {
    reasonOpen = reasonOpen === id ? null : id;
  }

  function reasonOf(id: string): string {
    return (reasonText[id] ?? "").trim();
  }

  function deny(item: ApprovalItem): void {
    onRespond(item.approval.id, false, undefined, reasonOf(item.approval.id) || undefined);
  }

  function allow(item: ApprovalItem): void {
    onRespond(item.approval.id, true, scopeOf(item.approval.id));
  }

  const riskLabels: Record<string, string> = { low: "低风险", medium: "中风险", high: "高风险" };

  function ruleKey(item: ApprovalItem): string {
    return item.approval.rule_key || item.approval.command;
  }
</script>

<div class="stack approvals" data-testid="approval-stack" aria-label="待处理审批" aria-live="assertive">
  {#if items.length > 1}
    <div class="batch" data-testid="approval-batch">
      <span>{items.length} 条待审批</span>
      {#if lowRiskCount > 1 && onBatchAllowLowRisk}
        <button type="button" data-testid="approval-batch-allow" onclick={onBatchAllowLowRisk}>
          批准 {lowRiskCount} 条低风险
        </button>
      {/if}
    </div>
  {/if}

  {#each items as item (item.approval.id)}
    <article class="approval" data-approval-id={item.approval.id} aria-busy={item.state === "pending"}>
      <div class="heading">
        <span class="badge">需要审批</span>
        <span class="tool">{item.agentName ? `${item.agentName} · ` : ""}{item.approval.tool || "未知工具"}</span>
        {#if item.approval.risk}
          <span class="risk" data-risk={item.approval.risk} data-testid="approval-risk">{riskLabels[item.approval.risk] ?? item.approval.risk}</span>
        {/if}
      </div>

      <code class="command sensitive">{item.approval.command || "未提供命令"}</code>

      <span class="context sensitive">
        {#if item.approval.repo}<span title={item.approval.repo}>{item.approval.repo}</span>{/if}
        {#if item.approval.branch}<span> · {item.approval.branch}</span>{/if}
        {#if item.approval.cwd}<span> · {item.approval.cwd}</span>{/if}
        <span> · {item.approval.session.slice(0, 8) || "无会话"}</span>
      </span>

      {#if item.approval.risk_reason}
        <p class="reason-line" data-testid="approval-risk-reason">{item.approval.risk_reason}</p>
      {/if}

      {#if item.approval.diff}
        {@const diff = item.approval.diff}
        <div class="diff" data-testid="approval-diff">
          <div class="diff-head">
            <code class="sensitive">{diff.file}</code>
            <span class="diff-stat"><b>+{diff.added}</b> <i>−{diff.removed}</i></span>
          </div>
          <div class="diff-body sensitive">
            {#each diff.lines as line}
              <div class:add={line.kind === "add"} class:del={line.kind === "del"} class:ctx={line.kind === "ctx"}>
                <span class="mark">{line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}</span><code>{line.text}</code>
              </div>
            {/each}
          </div>
          {#if diff.truncated}<div class="diff-more">仅显示前 {diff.lines.length} 行</div>{/if}
        </div>
      {/if}

      {#if reasonOpen === item.approval.id}
        <textarea
          class="reason-input sensitive"
          data-testid="approval-reason"
          rows="2"
          placeholder="告诉它应该怎么做，例如：先在 staging 验证，不要动 production"
          value={reasonText[item.approval.id] ?? ""}
          oninput={(event) => { reasonText = { ...reasonText, [item.approval.id]: event.currentTarget.value }; }}
        ></textarea>
      {/if}

      {#if item.error}
        <p class="error sensitive" role="alert"><strong>{item.error.code}</strong> · {item.error.message}</p>
      {/if}

      <div class="actions">
        <button type="button" class="deny" data-testid="approval-deny" disabled={item.state === "pending" || (item.state === "failed" && !item.error?.retryable)} onclick={() => deny(item)}>
          {item.state === "pending" && item.decision === false ? "拒绝中…" : item.state === "failed" && item.decision === false && item.error?.retryable ? "重试拒绝" : "拒绝"}
        </button>
        <button type="button" class="reason-toggle" data-testid="approval-reason-toggle" onclick={() => toggleReason(item.approval.id)}>
          {reasonOpen === item.approval.id ? "收起理由" : "拒绝并说明…"}
        </button>
        {#if onRemember && ruleKey(item)}
          <button
            class="remember"
            type="button"
            data-testid="approval-remember"
            title={`以后同类命令自动允许：${suggestRulePattern(ruleKey(item))}`}
            onclick={() => onRemember(item.approval.tool, suggestRulePattern(ruleKey(item)))}
          >记住这条</button>
        {/if}
        <label class="scope">
          <span>范围</span>
          <select
            data-testid="approval-scope"
            aria-label="授权范围"
            value={scopeOf(item.approval.id)}
            onchange={(event) => setScope(item.approval.id, event.currentTarget.value)}
          >
            <option value="once">仅本次</option>
            <option value="ten_minutes">10 分钟</option>
            <option value="session">本会话</option>
          </select>
        </label>
        <button class="allow" type="button" data-testid="approval-allow" disabled={item.state === "pending" || (item.state === "failed" && !item.error?.retryable)} onclick={() => allow(item)}>
          {item.state === "pending" && item.decision === true ? "允许中…" : item.state === "failed" && item.decision === true && item.error?.retryable ? "重试允许" : "允许"}
        </button>
      </div>
    </article>
  {/each}
</div>

<style>
  .stack { display: grid; gap: 8px; }
  .batch { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 7px 10px; border: 1px solid var(--island-event-border); border-radius: 11px; color: var(--island-text-secondary); background: var(--island-event-surface); box-shadow: var(--island-event-shadow); font-size: 10px; pointer-events: auto; }
  .batch button { min-height: 26px; padding: 2px 9px; border: 1px solid var(--island-border-subtle); border-radius: 7px; color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 10px; cursor: pointer; }
  .approval { display: grid; gap: 8px; padding: 12px; border: 1px solid var(--island-event-border); border-radius: 15px; color: var(--island-text-primary); background: var(--island-event-surface); box-shadow: var(--island-event-shadow); pointer-events: auto; }
  .heading { display: flex; align-items: center; gap: 8px; }
  .badge { color: var(--island-status-waiting); font-size: 11px; font-weight: 800; }
  .tool { min-width: 0; color: var(--island-text-secondary); font-size: 10px; overflow-wrap: anywhere; }
  .risk { margin-left: auto; flex: 0 0 auto; padding: 2px 7px; border: 1px solid var(--island-border-subtle); border-radius: 999px; color: var(--island-text-secondary); font-size: 9.5px; font-weight: 650; }
  .risk[data-risk="high"] { color: var(--island-status-error); border-color: color-mix(in srgb, var(--island-status-error) 34%, transparent); background: color-mix(in srgb, var(--island-status-error) 10%, transparent); }
  .risk[data-risk="medium"] { color: var(--island-status-waiting); border-color: color-mix(in srgb, var(--island-status-waiting) 34%, transparent); background: color-mix(in srgb, var(--island-status-waiting) 10%, transparent); }
  .risk[data-risk="low"] { color: var(--island-status-done); border-color: color-mix(in srgb, var(--island-status-done) 30%, transparent); }
  .command { overflow: hidden; padding: 8px; border-radius: 9px; color: var(--island-log-text); background: var(--island-log-surface); font: 500 11px/1.45 ui-monospace, "Cascadia Code", monospace; overflow-wrap: anywhere; }
  .context { min-width: 0; color: var(--island-text-tertiary); font-size: 9.5px; overflow-wrap: anywhere; }
  .reason-line { margin: 0; color: var(--island-text-secondary); font-size: 10px; line-height: 1.5; }
  .diff { overflow: hidden; border: 1px solid var(--island-border-subtle); border-radius: 9px; background: var(--island-log-surface); }
  .diff-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 5px 8px; border-bottom: 1px solid var(--island-border-subtle); color: var(--island-text-tertiary); font-size: 9px; }
  .diff-head code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .diff-stat { flex: 0 0 auto; font-variant-numeric: tabular-nums; }
  .diff-stat b { color: var(--island-status-done); font-weight: 650; }
  .diff-stat i { color: var(--island-status-error); font-style: normal; }
  .diff-body { max-height: 168px; overflow: auto; padding: 3px 0; font-family: ui-monospace, "Cascadia Code", monospace; font-size: 9px; line-height: 1.55; }
  .diff-body > div { display: grid; grid-template-columns: 14px minmax(0, 1fr); gap: 4px; padding: 0 8px; white-space: pre; }
  .diff-body .mark { color: var(--island-text-tertiary); user-select: none; }
  .diff-body .add { color: var(--island-status-done); background: color-mix(in srgb, var(--island-status-done) 8%, transparent); }
  .diff-body .del { color: var(--island-status-error); background: color-mix(in srgb, var(--island-status-error) 8%, transparent); }
  .diff-body .ctx { color: var(--island-text-secondary); }
  .diff-more { padding: 4px 8px; border-top: 1px solid var(--island-border-subtle); color: var(--island-text-tertiary); font-size: 9px; }
  .reason-input { width: 100%; min-height: 48px; padding: 7px 8px; resize: vertical; border: 1px solid var(--island-border-subtle); border-radius: 8px; color: var(--island-text-primary); background: var(--island-control-surface); font: 10px/1.5 var(--island-font, ui-sans-serif); }
  .reason-input:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 1px; }
  .error { margin: 0; color: var(--island-refresh-error-text); font-size: 11px; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .actions button { min-height: 32px; padding: 4px 10px; border: 1px solid var(--island-border-subtle); border-radius: 9px; color: var(--island-text-primary); background: var(--island-control-surface); cursor: pointer; font-size: 10.5px; }
  .actions button.allow { border-color: var(--island-accent-primary); color: var(--island-accent-primary); }
  .actions button.deny { color: var(--island-status-error); }
  .actions .remember { border-style: dashed; color: var(--island-text-secondary); }
  .actions .reason-toggle { color: var(--island-text-secondary); }
  .scope { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--island-text-tertiary); font-size: 9px; }
  .scope select { max-width: 88px; min-height: 28px; padding: 2px 4px; border: 1px solid var(--island-border-subtle); border-radius: 7px; color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 9px; }
  .scope select option { color: var(--island-menu-text); background: var(--island-menu-surface); }
  button:disabled { opacity: .55; cursor: wait; }
  button:focus-visible, select:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
</style>
