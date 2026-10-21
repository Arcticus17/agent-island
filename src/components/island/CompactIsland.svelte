<script lang="ts">
  import type { AgentView, DisplayStatus } from "../../bridge/types";
  import PixelAgent from "../brand/PixelAgent.svelte";

  let {
    agent,
    expanded,
    onToggle,
    iconStyle = "pixel",
  }: {
    agent: AgentView | null;
    expanded: boolean;
    onToggle: () => void;
    /** "pixel" 为像素吉祥物 + 数值徽标（默认）；"dot" 为原有的圆点语言 */
    iconStyle?: "dot" | "pixel";
  } = $props();

  const labels: Record<DisplayStatus, string> = { stopped: "已停止", idle: "空闲", working: "工作中", done: "已完成", error: "错误", waiting: "等待确认" };

  const status = $derived(agent?.display_status ?? "idle");
  const pixel = $derived(iconStyle === "pixel");

  /**
   * 右翼数值徽标：
   * 1) 优先额度百分比（Claude 5 小时窗口 / Codex 额度，来自 usage.used_percent）；
   * 2) 没有用量数据时回退到本会话结构化事件条数（至少让"活着"可见）。
   * 工具调用数需要后端补一个跨扫描的增量计数（当前日志只做头尾采样，直接数会来回跳），此处预留位置。
   */
  const badge = $derived.by(() => {
    const percent = agent?.usage?.used_percent;
    const events = agent?.active_session?.records?.length ?? 0;
    const eventNote = events > 0 ? ` · 本会话 ${events} 条事件` : "";
    if (typeof percent === "number") return { text: `${Math.round(percent)}%`, title: `额度已用 ${Math.round(percent)}%${eventNote}` };
    if (events > 0) return { text: events > 99 ? "99+" : `${events}`, title: `本会话 ${events} 条结构化事件` };
    return null;
  });
</script>

<button class="compact" class:pixel type="button" data-testid="compact-toggle" aria-expanded={expanded} aria-controls="expanded-island" onclick={onToggle}>
  {#if pixel}
    <span class="avatar" class:attention={status === "waiting"} aria-hidden="true"><PixelAgent agentName={agent?.name ?? null} size={20} {status} /></span>
  {:else}
    <span class="orb" aria-hidden="true"></span>
  {/if}
  <span class="identity sensitive"><strong>{agent?.name ?? "暂无 Agent"}</strong><span class="session">{agent?.active_session?.name ?? "当前会话未确认"}</span></span>
  {#if pixel}
    {#if status === "waiting"}
      <span class="badge attention" data-testid="compact-badge" role="img" aria-label="等待你确认" title="等待你确认">?</span>
    {:else if badge}
      <span class="badge" data-testid="compact-badge" role="img" aria-label={badge.title} title={badge.title}>{badge.text}</span>
    {/if}
  {/if}
  <span class="status" role="status" data-testid="compact-status">{labels[status]}</span>
  <span class="chevron" aria-hidden="true">{expanded ? "⌃" : "⌄"}</span>
</button>

<style>
  .compact { width: 100%; min-height: 58px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 11px; padding: 10px 14px; border: 0; color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .compact:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: -3px; }
  .orb { width: 12px; height: 12px; border-radius: 999px; background: var(--island-status-current); box-shadow: var(--island-status-glow); }
  .identity { min-width: 0; display: grid; gap: 1px; }
  strong, .session { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { font-size: 13px; letter-spacing: .01em; }
  .session { color: var(--island-text-secondary); font-size: 11px; }
  .status { color: var(--island-status-current); font-size: 12px; font-weight: 700; }
  .chevron { color: var(--island-text-secondary); font-size: 15px; }

  /* 像素模式：换成 flex，好让徽标按内容出现/消失，不占死一列 */
  .compact.pixel { display: flex; gap: 9px; }
  .compact.pixel .identity { flex: 1 1 auto; }
  .avatar { flex: 0 0 auto; display: grid; place-items: center; width: 22px; height: 22px; }
  .avatar.attention { animation: attention-bob 1.8s ease-in-out infinite; }
  .badge { flex: 0 0 auto; min-width: 20px; padding: 1px 5px; border: 1px solid var(--island-border-subtle); border-radius: 999px; background: var(--island-surface-raised); color: var(--island-text-tertiary); font-size: 10.5px; font-weight: 700; line-height: 15px; font-variant-numeric: tabular-nums; text-align: center; }
  .badge.attention { color: var(--island-status-waiting); border-color: color-mix(in srgb, var(--island-status-waiting) 45%, transparent); background: color-mix(in srgb, var(--island-status-waiting) 18%, transparent); animation: attention-pulse 1.6s ease-in-out infinite; }
  .compact.pixel .status, .compact.pixel .chevron { flex: 0 0 auto; }
  @keyframes attention-pulse { 0%, 100% { opacity: 1; transform: scale(1); } 50% { opacity: .55; transform: scale(1.1); } }
  @keyframes attention-bob { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-1.5px); } }

  @media (max-width: 300px) { .compact { grid-template-columns: auto minmax(0, 1fr) auto; } .status { display: none; } }
</style>
