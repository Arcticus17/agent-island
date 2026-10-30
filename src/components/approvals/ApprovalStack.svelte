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
        <span title={item.approval.session}> · {item.approval.session.slice(0, 8) || "无会话"}</span>
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
  /*
   * 审批卡是"告警"本身：整条抬头是实心琥珀，命令与范围是深色面板上的白字，
   * 允许是实心主按钮、拒绝是幽灵按钮。危险动作不靠颜色喊，靠层级。
   */
  .stack { display: grid; gap: 8px; }

  .batch {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-event);
    color: var(--ink-2);
    background: var(--ground-raised);
    font-size: var(--fs-2xs);
    pointer-events: auto;
  }
  .batch button {
    min-height: 24px;
    padding: 2px 8px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .batch button:hover { color: var(--ink); border-color: var(--rule-strong); }

  .approval {
    display: grid;
    gap: 8px;
    padding: 0 0 10px;
    overflow: hidden;
    border: 1px solid var(--alarm);
    border-radius: var(--radius-event);
    color: var(--ink);
    background: var(--ground-raised);
    pointer-events: auto;
  }

  .heading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    background: var(--alarm);
    color: var(--alarm-ink);
  }
  .badge { color: var(--alarm-ink); font-size: var(--fs-xs); font-weight: 700; }
  .tool { min-width: 0; color: color-mix(in srgb, var(--alarm-ink) 78%, transparent); font-size: var(--fs-2xs); overflow-wrap: anywhere; }
  .risk {
    margin-left: auto;
    flex: 0 0 auto;
    padding: 1px 6px;
    border: 1px solid color-mix(in srgb, var(--alarm-ink) 40%, transparent);
    border-radius: 3px;
    color: var(--alarm-ink);
    font-size: 9px;
    font-weight: 650;
  }

  .command,
  .context,
  .reason-line,
  .diff,
  .error,
  .actions,
  .reason-input { margin-inline: 10px; }

  .command {
    overflow: hidden;
    padding: 7px 9px;
    border-radius: 6px;
    color: var(--ink-log);
    background: var(--ground-sunken);
    font: 400 var(--fs-2xs)/1.6 var(--font-mono);
    overflow-wrap: anywhere;
  }
  .context { min-width: 0; color: var(--ink-3); font: 400 var(--fs-2xs)/1.5 var(--font-mono); overflow-wrap: anywhere; }
  .reason-line { margin-block: 0; color: var(--ink-2); font-size: var(--fs-2xs); line-height: 1.6; }

  .diff { overflow: hidden; border: 1px solid var(--rule); border-radius: 6px; background: var(--ground-sunken); }
  .diff-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 5px 8px; border-bottom: 1px solid var(--rule); color: var(--ink-3); font: 400 9.5px/1.5 var(--font-mono); }
  .diff-head code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .diff-stat { flex: 0 0 auto; font-variant-numeric: tabular-nums; }
  .diff-stat b { color: var(--state-done); font-weight: 650; }
  .diff-stat i { color: var(--state-error); font-style: normal; }
  .diff-body { max-height: 168px; overflow: auto; padding: 3px 0; font: 400 9.5px/1.6 var(--font-mono); }
  .diff-body > div { display: grid; grid-template-columns: 12px minmax(0, 1fr); gap: 5px; padding: 0 8px; white-space: pre; }
  .diff-body .mark { color: var(--ink-3); user-select: none; }
  .diff-body .add { color: var(--state-done); background: color-mix(in srgb, var(--state-done) 9%, transparent); }
  .diff-body .del { color: var(--state-error); background: color-mix(in srgb, var(--state-error) 9%, transparent); }
  .diff-body .ctx { color: var(--ink-2); }
  .diff-more { padding: 4px 8px; border-top: 1px solid var(--rule); color: var(--ink-3); font-size: 9.5px; }

  .reason-input {
    width: calc(100% - 20px);
    min-height: 48px;
    padding: 6px 8px;
    resize: vertical;
    border: 1px solid var(--rule);
    border-radius: 5px;
    color: var(--ink);
    background: var(--field);
    font: 400 var(--fs-2xs)/1.6 var(--font-ui);
  }
  .reason-input::placeholder { color: var(--ink-3); }
  .reason-input:focus-visible { outline: 2px solid var(--ink); outline-offset: 1px; }

  .error { margin-block: 0; color: var(--state-error); font-size: var(--fs-2xs); overflow-wrap: anywhere; }

  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; }
  .actions button {
    min-height: 28px;
    padding: 3px 9px;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--ink-2);
    background: transparent;
    font: 600 var(--fs-2xs)/1 var(--font-ui);
    cursor: pointer;
  }
  .actions button:hover:not(:disabled) { color: var(--ink); background: var(--field); }
  .actions button.allow { color: var(--ground); background: var(--ink); font-weight: 650; }
  .actions button.allow:hover:not(:disabled) { color: var(--ground); background: var(--ink); opacity: .88; }
  .actions button.deny { color: var(--state-error); }
  .actions button.deny:hover:not(:disabled) { color: var(--state-error); background: color-mix(in srgb, var(--state-error) 12%, transparent); }
  .actions .remember { border: 0; color: var(--ink-3); }
  .actions .reason-toggle { color: var(--ink-3); }
  .actions .remember:hover:not(:disabled),
  .actions .reason-toggle:hover:not(:disabled) { color: var(--ink); background: var(--field); }

  .scope { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--ink-3); font-size: 9.5px; }
  .scope select {
    max-width: 92px;
    min-height: 26px;
    padding: 2px 4px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-2);
    background: var(--field);
    font-size: 9.5px;
  }
  .scope select option { color: var(--island-menu-text); background: var(--island-menu-surface); }

  button:disabled { opacity: .55; cursor: wait; }
  button:focus-visible, select:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
