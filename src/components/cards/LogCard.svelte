<script lang="ts">
  import { tick } from "svelte";
  import type { SessionView } from "../../bridge/types";
  import { shouldFollowLog } from "../../stores/interaction-store";
  import { highlightSegments, matchLabel, searchRecords } from "../../session/search";

  let { session }: { session: SessionView } = $props();
  let scroller = $state<HTMLElement>();
  let query = $state("");

  const hits = $derived(searchRecords(session.records, query));
  const filtered = $derived(query.trim().length > 0);

  $effect.pre(() => {
    session.records;
    // 搜索时不自动跟随到底部，否则会一直跳走
    if (filtered) return;
    if (!scroller) return;
    const follow = shouldFollowLog(scroller);
    void tick().then(() => { if (follow && scroller) scroller.scrollTop = scroller.scrollHeight; });
  });
</script>

<article class="card log-card">
  <div class="heading">
    <span class="eyebrow">结构化日志</span>
    <span data-testid="log-count">{matchLabel(query, hits.length, session.records.length)}</span>
  </div>
  <div class="search-row">
    <input
      bind:value={query}
      data-testid="log-search"
      type="search"
      placeholder="搜索本会话记录…"
      aria-label="搜索会话记录"
      onkeydown={(event) => { if (event.key === "Escape" && query) { event.stopPropagation(); query = ""; } }}
    />
    {#if filtered}
      <button type="button" data-testid="log-search-clear" aria-label="清除搜索" onclick={() => (query = "")}>×</button>
    {/if}
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users need to focus and scroll this log region) -->
  <div class="log" data-testid="log-scroll" bind:this={scroller} role="log" aria-label={session.lifecycle === "Historical" ? "所选本地会话日志" : "当前会话日志"} tabindex="0">
    {#each hits as hit (hit.record.event_id)}
      <div class={`record role-${hit.record.role.toLowerCase()}`} data-event-id={hit.record.event_id}><span class="role">{hit.record.role}</span><span class="text sensitive">{#each highlightSegments(hit.record.text, hit.ranges) as segment}{#if segment.hit}<mark>{segment.text}</mark>{:else}{segment.text}{/if}{/each}</span></div>
    {:else}
      <p role="status" data-testid="log-empty">{filtered ? `没有匹配「${query.trim()}」的记录` : "当前会话尚无可显示的对话记录。"}</p>
    {/each}
  </div>
</article>

<style>
  .card { min-width: 0; padding: 13px; border: 1px solid var(--island-border-subtle); border-radius: 16px; background: var(--island-surface-raised); }
  .log-card { grid-column: 1 / -1; } .heading { display: flex; justify-content: space-between; color: var(--island-text-secondary); font-size: 10px; }
  .eyebrow { letter-spacing: .12em; text-transform: uppercase; }
  .search-row { display: flex; align-items: center; gap: 6px; margin-top: 8px; }
  .search-row input { flex: 1 1 auto; min-width: 0; min-height: 26px; padding: 3px 8px; border: 1px solid var(--island-border-subtle); border-radius: var(--island-radius-control); background: var(--island-control-surface); color: var(--island-text-primary); font-size: 11px; }
  .search-row input:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 1px; }
  .search-row button { flex: 0 0 auto; width: 22px; height: 22px; border: 1px solid var(--island-border-subtle); border-radius: 50%; background: var(--island-control-surface); color: var(--island-text-secondary); font-size: 12px; line-height: 1; cursor: pointer; }
  .log { height: 164px; margin-top: 9px; overflow: auto; border-radius: 11px; background: var(--island-log-surface); scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track); }
  .log:focus-visible { outline: 2px solid var(--island-accent-primary); }
  .record { display: grid; grid-template-columns: 62px minmax(0, 1fr); gap: 8px; padding: 7px 9px; border-bottom: 1px solid var(--island-log-divider); }
  .role { color: var(--island-log-role-default); font: 600 10px/1.5 ui-monospace, "Cascadia Code", monospace; text-transform: uppercase; }
  .text { overflow-wrap: anywhere; color: var(--island-log-text); font: 400 11px/1.5 ui-monospace, "Cascadia Code", monospace; }
  mark { padding: 0 1px; border-radius: 2px; background: color-mix(in srgb, var(--island-accent-primary) 35%, transparent); color: var(--island-text-primary); }
  .role-tool .role { color: var(--island-log-role-tool); } .role-user .role { color: var(--island-log-role-user); }
  @media (max-width: 420px) { .record { grid-template-columns: 52px minmax(0, 1fr); } .log { height: 190px; } }
</style>
