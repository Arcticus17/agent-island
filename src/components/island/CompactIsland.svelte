<script lang="ts">
  import type { AgentView, DisplayStatus } from "../../bridge/types";
  import PixelAgent from "../brand/PixelAgent.svelte";

  let {
    agent,
    agents = [],
    expanded,
    onToggle,
    iconStyle = "pixel",
    pendingApprovals = false,
    inboxCount = 0,
    binding = "inferred",
  }: {
    agent: AgentView | null;
    /** 所有可见 Agent：折叠态把它们的吉祥物并排放在刘海里 */
    agents?: readonly AgentView[];
    expanded: boolean;
    onToggle: () => void;
    /** "pixel" 为像素吉祥物（默认）；"dot" 为圆点语言 */
    iconStyle?: "dot" | "pixel";
    /** 有审批在等：胶囊也要显性提示，不能只靠展开面板 */
    pendingApprovals?: boolean;
    /** 收件箱里未解决的事件数（通知/完成/审批） */
    inboxCount?: number;
    /** 会话身份来源：launched（岛内终端）/ hooked（hooks）/ inferred（推断） */
    binding?: "launched" | "hooked" | "inferred" | "manual";
  } = $props();

  const labels: Record<DisplayStatus, string> = { stopped: "已停止", idle: "空闲", working: "工作中", done: "已完成", error: "错误", waiting: "等待确认" };

  const status = $derived(agent?.display_status ?? "idle");
  const pixel = $derived(iconStyle === "pixel");
  /** 告警态：状态块被状态色占满；其它时候颜色只出现在字和一个小方标上。 */
  const tone = $derived(status === "error" ? "danger" : pendingApprovals || inboxCount > 0 || status === "waiting" ? "alert" : "calm");
  const word = $derived(pendingApprovals ? "待确认" : inboxCount > 0 ? "待处理" : labels[status]);
  const visibleAgents = $derived(agents.length > 0 ? agents.slice(0, 3) : []);

  /**
   * 右侧计数：待你处理的件数优先；没有待办时回退到额度百分比，
   * 再没有就回退到本会话结构化事件条数（至少让"活着"可见）。
   */
  const badge = $derived.by(() => {
    if (pendingApprovals) return { text: "!", kind: "pending" as const, title: "有审批等待你确认", percent: null };
    if (inboxCount > 0) return { text: inboxCount > 9 ? "9+" : `${inboxCount}`, kind: "inbox" as const, title: `${inboxCount} 件待处理`, percent: null };
    if (status === "waiting") return { text: "?", kind: "waiting" as const, title: "等待你确认", percent: null };
    const percent = agent?.usage?.used_percent;
    const events = agent?.active_session?.records?.length ?? 0;
    if (typeof percent === "number") return { text: `${Math.round(percent)}%`, kind: "usage" as const, title: `额度已用 ${Math.round(percent)}%`, percent };
    if (events > 0) return { text: events > 99 ? "99+" : `${events}`, kind: "events" as const, title: `本会话 ${events} 条结构化事件`, percent: null };
    return null;
  });

  const bindingLabel: Record<"launched" | "hooked" | "inferred" | "manual", string> = {
    launched: "岛内启动",
    hooked: "hooks",
    inferred: "",
    manual: "已绑定",
  };

  const ariaLabel = $derived([
    agent?.name ?? "暂无 Agent",
    word,
    badge?.kind === "usage" ? `额度 ${badge.text}` : "",
    expanded ? "收起详情" : "展开详情",
  ].filter(Boolean).join(" · "));
</script>

<button
  class="pill"
  class:expanded
  data-testid="compact-toggle"
  data-state={status}
  data-tone={tone}
  aria-expanded={expanded}
  aria-controls="expanded-island"
  aria-label={ariaLabel}
  title={agent ? `${agent.name} · ${word}` : "Agent Island"}
  onclick={onToggle}
  type="button"
>
  <span class="mascots" aria-hidden="true">
    {#if pixel && visibleAgents.length > 0}
      {#each visibleAgents as entry (entry.id)}
        <span class="mascot" class:current={entry.id === agent?.id}><PixelAgent agentName={entry.name} size={expanded ? 18 : 15} status={entry.display_status} /></span>
      {/each}
    {:else if pixel}
      <span class="mascot"><PixelAgent agentName={agent?.name ?? null} size={expanded ? 18 : 15} {status} /></span>
    {:else}
      <span class="orb"></span>
    {/if}
  </span>

  {#if !expanded}
    <span class="mark" aria-hidden="true">
      {#if status === "error"}
        <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3.2 3.2l5.6 5.6M8.8 3.2l-5.6 5.6" /></svg>
      {:else if status === "waiting" || tone === "alert"}
        <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 2.4v4.4M6 9.2v.4" /></svg>
      {:else if status === "done"}
        <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 6.4l2.4 2.4 4.4-5" /></svg>
      {:else if status === "working"}
        <i class="pulse"></i>
      {/if}
    </span>

    {#if badge}
      <span class="count" data-testid="compact-badge" data-kind={badge.kind} data-pending={badge.kind === "pending" ? "true" : undefined} data-inbox={badge.kind === "inbox" ? "true" : undefined} role="img" aria-label={badge.title} title={badge.title}>{badge.text}</span>
    {/if}

    <span class="sr-only" data-testid="compact-status" role="status">{word}</span>
    {#if binding !== "inferred"}
      <span class="sr-only" data-testid="agent-binding" data-binding={binding}>{bindingLabel[binding]}</span>
    {/if}
  {:else}
    <span class="identity sensitive">
      <strong>{agent?.name ?? "暂无 Agent"}</strong>
      <span class="session">{agent?.active_session?.name ?? "当前会话未确认"}</span>
    </span>

    {#if badge}
      <span class="readout" data-testid="compact-badge" data-kind={badge.kind} data-pending={badge.kind === "pending" ? "true" : undefined} data-inbox={badge.kind === "inbox" ? "true" : undefined} role="img" aria-label={badge.title} title={badge.title}>
        <span class="readout-value">{badge.text}</span>
        {#if badge.percent !== null}<span class="gauge" aria-hidden="true"><i style={`width:${Math.max(2, Math.min(100, badge.percent))}%`}></i></span>{/if}
      </span>
    {/if}

    {#if binding !== "inferred"}
      <span class="binding" data-testid="agent-binding" data-binding={binding} title={binding === "hooked" ? "会话身份由 hooks 自报" : "该 Agent 由岛内终端启动，身份可确认"}>{bindingLabel[binding]}</span>
    {/if}

    <span class="state" role="status" data-testid="compact-status">{word}</span>

    <span class="chevron" aria-hidden="true">
      <svg viewBox="0 0 12 12" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 7.5 L6 4 L9.5 7.5" /></svg>
    </span>
  {/if}
</button>

<style>
  /*
   * 折叠态是"刘海"：只有吉祥物、状态记号和一个计数，宽度由内容决定。
   * 它贴在工作区顶边，像系统挖出来的一块，而不是浮在桌面上的卡片。
   */
  .pill {
    width: fit-content;
    min-height: 40px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 11px 0 10px;
    border: 0;
    border-radius: 0 0 14px 14px;
    color: var(--ink);
    background: transparent;
    text-align: left;
    cursor: pointer;
    transition: color var(--duration-slow) var(--motion-out);
  }
  .pill:focus-visible { outline: 2px solid var(--ink); outline-offset: -3px; }

  .mascots { flex: 0 0 auto; display: inline-flex; align-items: center; }
  .mascot { display: grid; place-items: center; width: 15px; height: 18px; }
  .mascot + .mascot { margin-left: -3px; }
  .mascot:not(.current) { opacity: .55; }
  .orb { width: 8px; height: 8px; border-radius: 50%; background: var(--state-working); }
  .pill[data-state="idle"] .orb { background: var(--state-idle); }
  .pill[data-state="stopped"] .orb { background: var(--state-stopped); }
  .pill[data-state="done"] .orb { background: var(--state-done); }
  .pill[data-state="error"] .orb { background: var(--state-error); }

  .mark { display: grid; place-items: center; width: 10px; height: 12px; color: var(--ink-3); }
  .pill[data-state="working"] .mark { color: var(--state-working); }
  .pill[data-state="done"] .mark { color: var(--state-done); }
  .pill[data-state="waiting"] .mark,
  .pill[data-tone="alert"] .mark { color: var(--state-waiting); }
  .pill[data-state="error"] .mark { color: var(--state-error); }
  .pulse { width: 6px; height: 6px; border-radius: 1px; background: currentColor; animation: notch-pulse 2.2s var(--motion-out) infinite; }
  @keyframes notch-pulse { 50% { opacity: .3; } }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } }

  .count {
    flex: 0 0 auto;
    min-width: 16px;
    font: 600 11px/1 var(--font-readout);
    font-variant-numeric: tabular-nums;
    letter-spacing: .02em;
    text-align: right;
    color: var(--ink-2);
  }
  .count[data-kind="pending"],
  .count[data-kind="inbox"],
  .count[data-kind="waiting"] {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 18px;
    padding: 0 5px;
    border-radius: 4px;
    color: var(--alarm-ink);
    background: var(--alarm);
    font-weight: 700;
  }
  .pill[data-state="error"] .count[data-kind="pending"],
  .pill[data-state="error"] .count[data-kind="inbox"] { color: var(--danger-ink); background: var(--danger); }
  .count[data-kind="events"] { color: var(--ink-3); }

  /* ---- 展开态：同一条胶囊长成面板的抬头 ---- */
  .pill.expanded {
    width: auto;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 54px;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto auto auto;
    gap: var(--space-4);
    padding: 0 var(--space-5);
    border-radius: 0;
  }
  .pill.expanded .mascot { width: 18px; height: 20px; }
  .pill.expanded .mascot + .mascot { margin-left: -4px; opacity: .7; }

  .identity { min-width: 0; display: grid; gap: 1px; }
  strong, .session { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { font-size: var(--fs-md); font-weight: 650; }
  .session { color: var(--ink-3); font-size: var(--fs-2xs); }

  .readout {
    flex: 0 0 auto;
    display: grid;
    justify-items: end;
    gap: 3px;
    color: var(--ink-2);
    font: 600 11px/1 var(--font-readout);
    font-variant-numeric: tabular-nums;
  }
  .readout[data-kind="pending"],
  .readout[data-kind="inbox"],
  .readout[data-kind="waiting"] { color: var(--state-waiting); font-weight: 700; }
  .readout .gauge { display: block; width: 34px; height: 3px; border-radius: 2px; background: var(--rule); overflow: hidden; }
  .readout .gauge i { display: block; height: 100%; border-radius: inherit; background: currentColor; }

  .binding {
    flex: 0 0 auto;
    padding: 2px 6px;
    border: 1px solid var(--rule-strong);
    border-radius: 4px;
    color: var(--ink-2);
    font: 600 9px/1.2 var(--font-readout);
    letter-spacing: .06em;
    text-transform: uppercase;
  }

  .state { flex: 0 0 auto; display: inline-flex; align-items: center; gap: 6px; color: var(--state-idle); font-size: var(--fs-sm); font-weight: 650; }
  .state::before { content: ""; width: 6px; height: 6px; border-radius: 1px; background: currentColor; }
  .pill[data-state="working"] .state { color: var(--state-working); }
  .pill[data-state="done"] .state { color: var(--state-done); }
  .pill[data-state="waiting"] .state { color: var(--state-waiting); }
  .pill[data-state="error"] .state { color: var(--state-error); }
  .pill[data-state="stopped"] .state { color: var(--state-stopped); }

  .chevron { flex: 0 0 auto; display: grid; place-items: center; width: 20px; height: 20px; color: var(--ink-3); }
  .chevron svg { transition: transform var(--duration-normal) var(--motion-out); }
  .pill.expanded .chevron svg { transform: rotate(180deg); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }

  @media (max-width: 300px) {
    .pill.expanded { grid-template-columns: auto minmax(0, 1fr) auto; }
    .pill.expanded .state { display: none; }
  }
</style>
