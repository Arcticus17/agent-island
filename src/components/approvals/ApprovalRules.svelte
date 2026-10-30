<script lang="ts">
  import type { AgentIslandBridge } from "../../bridge/types";

  let { bridge }: { bridge: AgentIslandBridge } = $props();

  type Rule = { id: string; tool: string; pattern: string };
  type AuditEntry = { at_ms: number; id: string; tool: string; command: string; rule: string; decision: string };

  const TOOL_OPTIONS = ["任意工具", "Bash", "Edit", "Write", "Read"];

  let rules = $state<Rule[]>([]);
  let audit = $state<AuditEntry[]>([]);
  let tool = $state(TOOL_OPTIONS[0]);
  let pattern = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let loaded = $state(false);

  const supported = $derived(!!bridge.listApprovalRules && !!bridge.addApprovalRule);

  async function load(): Promise<void> {
    if (!supported || loaded) return;
    loaded = true;
    try {
      rules = (await bridge.listApprovalRules!()) ?? [];
      audit = (await bridge.listApprovalAudit?.(5)) ?? [];
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function add(): Promise<void> {
    if (!bridge.addApprovalRule || busy) return;
    const value = pattern.trim();
    if (!value) {
      error = "命令模式不能为空";
      return;
    }
    busy = true;
    error = null;
    try {
      rules = await bridge.addApprovalRule(tool === TOOL_OPTIONS[0] ? "" : tool, value);
      pattern = "";
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  async function remove(id: string): Promise<void> {
    if (!bridge.removeApprovalRule || busy) return;
    busy = true;
    try {
      rules = await bridge.removeApprovalRule(id);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  function timeLabel(atMs: number): string {
    const date = new Date(atMs);
    return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  }
</script>

<details class="approval-rules" data-testid="approval-rules-drawer" ontoggle={(event) => { if (event.currentTarget.open) void load(); }}>
  <summary>
    <span>审批规则</span>
    {#if rules.length > 0}
      <span class="badge" data-testid="approval-rule-count">{rules.length} 条</span>
    {:else}
      <span class="muted">自动放行清单</span>
    {/if}
  </summary>
  <div class="body">
    <p class="hint">命中规则的工具调用会直接允许、不再弹卡片，但都会记进审计（最近 200 条）。</p>
    {#if !supported}
      <p class="error" role="alert">当前宿主不支持审批规则</p>
    {:else}
      {#if rules.length > 0}
        <ul class="rules" data-testid="approval-rule-list">
          {#each rules as rule (rule.id)}
            <li>
              <code data-testid="approval-rule">{rule.tool || "任意工具"} · {rule.pattern}</code>
              <button type="button" data-testid="approval-rule-remove" data-rule-id={rule.id} aria-label={`删除规则 ${rule.pattern}`} onclick={() => void remove(rule.id)}>删除</button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="adder">
        <select bind:value={tool} aria-label="规则适用的工具">
          {#each TOOL_OPTIONS as option (option)}<option value={option}>{option}</option>{/each}
        </select>
        <input bind:value={pattern} data-testid="approval-rule-input" placeholder="命令包含或 * 通配，如 npm *" aria-label="命令模式" />
        <button type="button" data-testid="approval-rule-add" disabled={busy} onclick={() => void add()}>添加</button>
      </div>
      {#if audit.length > 0}
        <div class="audit" data-testid="approval-audit">
          <span class="audit-title">最近自动放行</span>
          {#each audit as entry (entry.id)}
            <p><span class="when">{timeLabel(entry.at_ms)}</span><code>{entry.command}</code></p>
          {/each}
        </div>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    {/if}
  </div>
</details>

<style>
  /* 审批规则：自动放行清单，系统区里的一条可展开行 */
  .approval-rules { border-top: 1px solid var(--rule); }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    list-style: none;
    cursor: pointer;
    color: var(--ink-3);
    font-size: var(--fs-2xs);
  }
  summary::-webkit-details-marker { display: none; }
  summary::after {
    content: "";
    width: 7px;
    height: 7px;
    margin-left: auto;
    border-right: 1px solid currentColor;
    border-bottom: 1px solid currentColor;
    transform: rotate(45deg) translateY(-2px);
    transition: transform var(--duration-fast) ease-out;
  }
  .approval-rules[open] > summary::after { transform: rotate(225deg) translateY(-1px); }
  summary:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .badge {
    padding: 1px 6px;
    border-radius: 3px;
    color: var(--ink);
    background: var(--field);
    font: 600 var(--fs-2xs)/1.5 var(--font-readout);
    font-variant-numeric: tabular-nums;
  }
  .muted { color: var(--ink-3); font-size: var(--fs-2xs); }
  .body { display: grid; gap: 8px; padding: 0 0 10px; }
  .hint { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); line-height: 1.6; }
  .rules { margin: 0; padding: 0; display: grid; gap: 0; list-style: none; }
  .rules li { display: flex; align-items: center; gap: 8px; border-top: 1px solid var(--rule); }
  .rules code {
    flex: 1 1 auto;
    min-width: 0;
    padding: 5px 0;
    border: 0;
    background: transparent;
    color: var(--ink-2);
    font: 400 var(--fs-2xs)/1.5 var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rules button, .adder button {
    flex: 0 0 auto;
    min-height: 24px;
    padding: 2px 8px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .rules button:hover, .adder button:hover:not(:disabled) { color: var(--ink); border-color: var(--rule-strong); }
  .adder { display: flex; gap: 6px; align-items: center; }
  .adder select, .adder input {
    min-height: 26px;
    min-width: 0;
    padding: 3px 6px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    background: var(--field);
    color: var(--ink);
    font-size: var(--fs-2xs);
  }
  .adder select { flex: 0 0 auto; max-width: 96px; }
  .adder input { flex: 1 1 auto; }
  .adder input::placeholder { color: var(--ink-3); }
  .audit { display: grid; gap: 2px; }
  .audit-title { color: var(--ink-3); font-size: var(--fs-2xs); }
  .audit p { margin: 0; display: flex; gap: 6px; align-items: baseline; font-size: var(--fs-2xs); color: var(--ink-3); }
  .audit code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-2); }
  .when { flex: 0 0 auto; color: var(--ink-3); font-variant-numeric: tabular-nums; }
  .error { margin: 0; color: var(--state-error); font-size: var(--fs-2xs); }
  button:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
