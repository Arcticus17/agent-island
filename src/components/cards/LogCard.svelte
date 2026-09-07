<script lang="ts">
  import { tick } from "svelte";
  import type { SessionView } from "../../bridge/types";
  import { shouldFollowLog } from "../../stores/interaction-store";
  let { session }: { session: SessionView } = $props();
  let scroller = $state<HTMLElement>();
  $effect.pre(() => {
    session.records;
    if (!scroller) return;
    const follow = shouldFollowLog(scroller);
    void tick().then(() => { if (follow && scroller) scroller.scrollTop = scroller.scrollHeight; });
  });
</script>

<article class="card log-card">
  <div class="heading"><span class="eyebrow">结构化日志</span><span>{session.records.length}</span></div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users need to focus and scroll this log region) -->
  <div class="log" data-testid="log-scroll" bind:this={scroller} role="log" aria-label="当前会话日志" tabindex="0">
    {#each session.records as record (record.event_id)}
      <div class={`record role-${record.role.toLowerCase()}`} data-event-id={record.event_id}><span class="role">{record.role}</span><span class="text">{record.text}</span></div>
    {/each}
  </div>
</article>

<style>
  .card { min-width: 0; padding: 13px; border: 1px solid var(--island-border-subtle); border-radius: 16px; background: var(--island-surface-raised); }
  .log-card { grid-column: 1 / -1; } .heading { display: flex; justify-content: space-between; color: var(--island-text-secondary); font-size: 10px; }
  .eyebrow { letter-spacing: .12em; text-transform: uppercase; }
  .log { height: 164px; margin-top: 9px; overflow: auto; border-radius: 11px; background: var(--island-log-surface); scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track); }
  .log:focus-visible { outline: 2px solid var(--island-accent-primary); }
  .record { display: grid; grid-template-columns: 62px minmax(0, 1fr); gap: 8px; padding: 7px 9px; border-bottom: 1px solid var(--island-log-divider); }
  .role { color: var(--island-log-role-default); font: 600 10px/1.5 ui-monospace, "Cascadia Code", monospace; text-transform: uppercase; }
  .text { overflow-wrap: anywhere; color: var(--island-log-text); font: 400 11px/1.5 ui-monospace, "Cascadia Code", monospace; }
  .role-tool .role { color: var(--island-log-role-tool); } .role-user .role { color: var(--island-log-role-user); }
  @media (max-width: 420px) { .record { grid-template-columns: 52px minmax(0, 1fr); } .log { height: 190px; } }
</style>
