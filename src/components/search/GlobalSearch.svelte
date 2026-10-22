<script lang="ts">
  import type { AgentIslandBridge, SessionSearchHit } from "../../bridge/types";

  let { bridge }: { bridge: AgentIslandBridge } = $props();

  let query = $state("");
  let hits = $state<SessionSearchHit[]>([]);
  let status = $state<"idle" | "searching" | "ready" | "failed">("idle");
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let request = 0;

  const supported = $derived(!!bridge.searchSessions);

  // 输入防抖：每敲一个字就扫盘很不划算
  let timer: number | undefined;
  $effect(() => {
    const text = query;
    if (timer !== undefined) window.clearTimeout(timer);
    if (!supported) return;
    if (text.trim().length < 2) {
      hits = [];
      status = "idle";
      return;
    }
    timer = window.setTimeout(() => void run(text), 300);
    return () => { if (timer !== undefined) window.clearTimeout(timer); };
  });

  async function run(text: string): Promise<void> {
    if (!bridge.searchSessions) return;
    const current = ++request;
    status = "searching";
    error = null;
    try {
      const found = await bridge.searchSessions(text, 20);
      if (current !== request) return;
      hits = found;
      status = "ready";
    } catch (cause) {
      if (current !== request) return;
      hits = [];
      status = "failed";
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function reveal(hit: SessionSearchHit): Promise<void> {
    if (!bridge.revealSession) {
      error = "当前宿主不支持定位文件";
      return;
    }
    try {
      await bridge.revealSession(hit.path);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function timeLabel(atMs: number): string {
    if (!atMs) return "";
    const date = new Date(atMs);
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    const hour = String(date.getHours()).padStart(2, "0");
    const minute = String(date.getMinutes()).padStart(2, "0");
    return `${month}-${day} ${hour}:${minute}`;
  }
</script>

<details class="global-search" data-testid="global-search" ontoggle={(event) => { loaded = event.currentTarget.open; }}>
  <summary>
    <span>全局搜索</span>
    {#if hits.length > 0}
      <span class="badge" data-testid="global-search-count">{hits.length} 条</span>
    {:else}
      <span class="muted">跨会话找一句话</span>
    {/if}
  </summary>
  <div class="body">
    {#if !supported}
      <p class="hint">当前宿主不支持跨会话搜索</p>
    {:else}
      <input
        bind:value={query}
        data-testid="global-search-input"
        type="search"
        placeholder="至少两个字符，例如 npm、布局"
        aria-label="搜索所有会话"
      />
      <p class="hint" role="status" data-testid="global-search-status">
        {#if status === "searching"}正在搜索…
        {:else if status === "failed"}搜索失败{error ? `：${error}` : ""}
        {:else if query.trim().length < 2}输入至少两个字符
        {:else if hits.length === 0}没有找到相关消息
        {:else}共 {hits.length} 条（最多显示 20 条）{/if}
      </p>
      {#if hits.length > 0}
        <ul data-testid="global-search-results">
          {#each hits as hit, index (hit.path + hit.role + index)}
            <li>
              <button type="button" data-testid="global-search-hit" data-hit-path={hit.path} onclick={() => void reveal(hit)}>
                <span class="meta">{hit.agent} · {hit.role} · {timeLabel(hit.at_ms)}</span>
                <span class="snippet">{hit.snippet}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if error && status !== "failed"}<p class="error" role="alert">{error}</p>{/if}
    {/if}
  </div>
</details>

<style>
  .global-search { border-top: 1px solid var(--island-border-subtle); }
  summary { display: flex; align-items: center; gap: 8px; padding: 8px 12px; list-style: none; cursor: pointer; color: var(--island-text-secondary); font-size: 11px; }
  summary::-webkit-details-marker { display: none; }
  .badge { padding: 1px 6px; border: 1px solid var(--island-border-subtle); border-radius: 999px; background: var(--island-control-surface); color: var(--island-text-primary); font-size: 10px; font-weight: 700; }
  .muted, .hint { color: var(--island-text-tertiary); font-size: 10px; }
  .body { display: grid; gap: 6px; padding: 0 12px 12px; }
  .hint, .error { margin: 0; }
  input { min-height: 28px; padding: 4px 8px; border: 1px solid var(--island-border-subtle); border-radius: var(--island-radius-control); background: var(--island-control-surface); color: var(--island-text-primary); font-size: 11px; }
  input:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 1px; }
  ul { margin: 0; padding: 0; display: grid; gap: 4px; max-height: 220px; overflow-y: auto; list-style: none; }
  li button { width: 100%; display: grid; gap: 2px; padding: 6px 8px; border: 1px solid var(--island-border-subtle); border-radius: 8px; background: var(--island-control-surface); color: var(--island-text-secondary); text-align: left; cursor: pointer; }
  li button:hover { border-color: var(--island-accent-primary); }
  li button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 1px; }
  .meta { color: var(--island-text-tertiary); font-size: 10px; }
  .snippet { color: var(--island-text-primary); font-size: 11px; overflow-wrap: anywhere; }
  .error { color: var(--island-refresh-error-text); font-size: 10px; }
</style>
