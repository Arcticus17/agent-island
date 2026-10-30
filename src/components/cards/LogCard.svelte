<script lang="ts">
  import { tick } from "svelte";
  import type { SessionView } from "../../bridge/types";
  import { shouldFollowLog } from "../../stores/interaction-store";
  import { highlightSegments, matchLabel, searchRecords } from "../../session/search";

  let { session }: { session: SessionView } = $props();
  let scroller = $state<HTMLElement>();
  let query = $state("");
  /** 哪些长记录被手动展开（按 event_id）；展开后不再截断。 */
  let open = $state<Record<string, boolean>>({});

  const hits = $derived(searchRecords(session.records, query));
  const filtered = $derived(query.trim().length > 0);

  /** 时间羻：每条记录只有时刻，最值钱的是"距上一条多久"。 */
  function timeLabel(atMs: number): string {
    const date = new Date(atMs);
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  }

  function durationLabel(ms: number): string {
    if (ms < 1_000) return "";
    const seconds = ms / 1_000;
    if (seconds < 10) return `+${seconds.toFixed(1)}s`;
    if (seconds < 60) return `+${Math.round(seconds)}s`;
    if (seconds < 3_600) return `+${Math.round(seconds / 60)}m`;
    return `+${Math.round(seconds / 3_600)}h`;
  }

  /** 每条记录与上一条的时间差（用完整记录算，搜索过滤不影响）。 */
  const deltas = $derived.by(() => {
    const map = new Map<string, number>();
    let previous: number | null = null;
    for (const record of session.records) {
      if (previous !== null) map.set(record.event_id, Math.max(0, record.at_ms - previous));
      previous = record.at_ms;
    }
    return map;
  });

  /** 工具调用：role=Tool，或正文以 `Bash(` / `Edit(` 这类形式开头。 */
  function isTool(role: string, text: string): boolean {
    return role === "Tool" || /^[A-Z][A-Za-z]{2,}\(/.test(text.trimStart());
  }

  function isLong(text: string): boolean {
    return text.length > 180 || text.split("\n").length > 3;
  }

  function toggle(eventId: string): void {
    open = { ...open, [eventId]: !open[eventId] };
    void tick().then(() => onLayoutChange());
  }

  function onLayoutChange(): void {
    // 展开一条记录会改变面板高度，交给外层重新量窗口
    window.dispatchEvent(new CustomEvent("agent-island-layout"));
  }

  $effect.pre(() => {
    session.records;
    // 搜索时不自动跟随到底部，否则会一直跳走
    if (filtered) return;
    if (!scroller) return;
    const follow = shouldFollowLog(scroller);
    void tick().then(() => { if (follow && scroller) scroller.scrollTop = scroller.scrollHeight; });
  });
</script>

<article class="card log-card" aria-label="工作回执">
  <div class="heading">
    <input
      bind:value={query}
      data-testid="log-search"
      type="search"
      placeholder="在这轮工作里搜命令、文件、输出…"
      aria-label="搜索会话记录"
      onkeydown={(event) => { if (event.key === "Escape" && query) { event.stopPropagation(); query = ""; } }}
    />
    {#if filtered}
      <button type="button" class="clear" data-testid="log-search-clear" aria-label="清除搜索" onclick={() => (query = "")}>
        <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3 3 L9 9 M9 3 L3 9" /></svg>
      </button>
    {/if}
    <span class="count" data-testid="log-count">{matchLabel(query, hits.length, session.records.length)}</span>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users need to focus and scroll this log region) -->
  <div class="log" data-testid="log-scroll" bind:this={scroller} role="log" aria-label={session.lifecycle === "Historical" ? "所选本地会话回执" : "本轮工作回执"} tabindex="0">
    {#each hits as hit (hit.record.event_id)}
      {@const structuredTool = typeof hit.record.tool === "string" && hit.record.tool.length > 0}
      {@const tool = structuredTool || isTool(hit.record.role, hit.record.text)}
      {@const body = structuredTool ? hit.record.detail ?? hit.record.text : hit.record.text}
      {@const long = isLong(body)}
      {@const expanded = open[hit.record.event_id] === true}
      <div class={`record role-${hit.record.role.toLowerCase()}`} class:tool class:open={expanded} data-event-id={hit.record.event_id}>
        <span class="time">{timeLabel(hit.record.at_ms)}</span>
        <span class="node" aria-hidden="true"></span>
        <span class="role">{hit.record.role}</span>
        <span class="text sensitive">
          {#if structuredTool}
            <span class="chip">{hit.record.tool}</span><span class="cmd">{body}</span>
          {:else}
            {#each highlightSegments(hit.record.text, hit.ranges) as segment}{#if segment.hit}<mark>{segment.text}</mark>{:else}{segment.text}{/if}{/each}
          {/if}
        </span>
        <span class="tail">
          {#if hit.record.success === true}
            <span class="result ok" role="img" aria-label="执行成功"><svg viewBox="0 0 12 12" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 6.4l2.4 2.4 4.4-5" /></svg></span>
          {:else if hit.record.success === false}
            <span class="result fail" role="img" aria-label="执行失败"><svg viewBox="0 0 12 12" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M3.2 3.2l5.6 5.6M8.8 3.2l-5.6 5.6" /></svg></span>
          {/if}
          {#if deltas.has(hit.record.event_id)}<span class="delta">{durationLabel(deltas.get(hit.record.event_id) ?? 0)}</span>{/if}
          {#if long}
            <button type="button" class="more" aria-expanded={expanded} onclick={() => toggle(hit.record.event_id)} title={expanded ? "收起这条记录" : "展开完整内容"}>
              {expanded ? "收起" : `+${Math.max(1, Math.ceil(body.length / 60) - 3)} 行`}
            </button>
          {/if}
        </span>
      </div>
    {:else}
      <p role="status" data-testid="log-empty">{filtered ? `没有匹配「${query.trim()}」的记录` : "这一轮还没有可显示的工作记录。"}</p>
    {/each}
  </div>
</article>

<style>
  /*
   * 工作回执：一行一条记录，左侧时间羻 + 节点，右侧是命令/输出的等宽正文。
   * 工具调用有自己的节点形状；长记录可以就地展开，不跳走。
   */
  .card { min-width: 0; display: grid; gap: 0; }
  .heading { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .heading input {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 26px;
    padding: 2px 0;
    border: 0;
    border-bottom: 1px solid var(--rule);
    border-radius: 0;
    background: transparent;
    color: var(--ink);
    font: 400 var(--fs-2xs)/1.6 var(--font-ui);
    transition: border-color var(--duration-fast) ease-out;
  }
  .heading input::placeholder { color: var(--ink-3); }
  .heading input:hover { border-bottom-color: var(--rule-strong); }
  .heading input:focus-visible { outline: 0; border-bottom-color: var(--ink-2); }
  .clear { flex: 0 0 auto; display: grid; place-items: center; width: 18px; height: 18px; padding: 0; border: 0; border-radius: 3px; color: var(--ink-3); background: transparent; cursor: pointer; }
  .clear:hover { color: var(--ink); background: var(--field); }
  .count { flex: 0 0 auto; color: var(--ink-3); font: 500 var(--fs-2xs)/1 var(--font-readout); font-variant-numeric: tabular-nums; }

  .log { height: 164px; margin-top: 8px; overflow: auto; background: transparent; }
  .log:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }

  .record {
    display: grid;
    grid-template-columns: 52px 10px 56px minmax(0, 1fr) auto;
    align-items: start;
    gap: 6px;
    padding: 6px 2px;
    border-top: 1px solid var(--rule);
  }
  .record:first-child { border-top: 0; }

  .time { color: var(--ink-3); font: 400 var(--fs-2xs)/1.6 var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .node { width: 5px; height: 5px; margin-top: 6px; border-radius: 1px; background: var(--state-idle); }
  .record.role-assistant .node { background: var(--state-working); }
  .record.role-user .node { background: var(--ink-3); border-radius: 50%; }
  .record.tool .node { background: var(--state-waiting); }
  .role { color: var(--ink-3); font: 600 var(--fs-2xs)/1.6 var(--font-mono); letter-spacing: .02em; text-transform: uppercase; white-space: nowrap; overflow: hidden; }
  .record.tool .role { color: var(--state-waiting); }
  .record.role-user .role { color: var(--ink-2); }

  .text {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--ink-log);
    font: 400 var(--fs-sm)/1.6 var(--font-mono);
  }
  .record.tool .text { color: var(--ink); }
  .record.open .text { display: block; overflow: visible; }
  mark { padding: 0 1px; border-radius: 2px; background: color-mix(in srgb, var(--state-working) 30%, transparent); color: var(--ink); }

  /* 结构化工具行：工具名是一枚小标签，后面跟一行命令/路径 */
  .chip {
    display: inline-block;
    margin-right: 7px;
    padding: 0 5px;
    border-radius: 3px;
    color: var(--state-waiting);
    background: color-mix(in srgb, var(--state-waiting) 12%, transparent);
    font: 600 9px/1.7 var(--font-readout);
    letter-spacing: .04em;
    text-transform: uppercase;
    vertical-align: 1px;
  }
  .cmd { color: var(--ink); }

  .tail { display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .result { display: grid; place-items: center; width: 12px; height: 12px; }
  .result.ok { color: var(--state-done); }
  .result.fail { color: var(--state-error); }
  .delta { color: var(--ink-3); font: 400 9.5px/1.6 var(--font-readout); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .more {
    padding: 0;
    border: 0;
    color: var(--ink-3);
    background: none;
    font-size: var(--fs-2xs);
    white-space: nowrap;
    text-decoration: underline;
    text-underline-offset: 2px;
    text-decoration-color: var(--rule-strong);
    cursor: pointer;
  }
  .more:hover { color: var(--ink); text-decoration-color: var(--ink-2); }
  .more:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .log p { margin: 8px 2px; color: var(--ink-3); font-size: var(--fs-2xs); }

  @media (max-width: 420px) {
    .record { grid-template-columns: 44px 8px 46px minmax(0, 1fr); gap: 5px; }
    .time, .role { font-size: 8.5px; letter-spacing: 0; }
    .tail { grid-column: 4; flex-direction: row; align-items: center; gap: 6px; }
    .log { height: 190px; }
  }
</style>
