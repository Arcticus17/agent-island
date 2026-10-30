<script lang="ts">
  import type { AgentIslandBridge, SessionSearchHit } from "../../bridge/types";

  let {
    bridge,
    onOpen,
  }: {
    bridge: AgentIslandBridge;
    /** 打开命中的会话（由 App 负责加载并展示） */
    onOpen?: (hit: SessionSearchHit) => void;
  } = $props();

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

  function open(hit: SessionSearchHit): void {
    if (onOpen) onOpen(hit);
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
              <div class="hit-row">
                <button type="button" class="hit" data-testid="global-search-hit" data-hit-path={hit.path} onclick={() => open(hit)}>
                  <span class="meta">{hit.agent} · {hit.role} · {timeLabel(hit.at_ms)}</span>
                  <span class="snippet">{hit.snippet}</span>
                </button>
                {#if bridge.revealSession}
                  <button type="button" class="locate" data-testid="global-search-reveal" aria-label="在资源管理器里定位文件" title="在资源管理器里定位文件" onclick={() => void reveal(hit)}>⋯</button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if error && status !== "failed"}<p class="error" role="alert">{error}</p>{/if}
    {/if}
  </div>
</details>

<style>
  /* 全局搜索：系统区里的一个可展开行，命中结果也是一行一条，不再套小卡片 */
  .global-search { border-top: 1px solid var(--rule); }
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
  .global-search[open] > summary::after { transform: rotate(225deg) translateY(-1px); }
  summary:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .badge {
    padding: 1px 6px;
    border-radius: 3px;
    color: var(--ink);
    background: var(--field);
    font: 600 var(--fs-2xs)/1.5 var(--font-readout);
    font-variant-numeric: tabular-nums;
  }
  .muted, .hint { color: var(--ink-3); font-size: var(--fs-2xs); }
  .body { display: grid; gap: 6px; padding: 0 0 10px; }
  .hint, .error { margin: 0; }
  input {
    min-height: 28px;
    padding: 4px 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-control);
    color: var(--ink);
    background: var(--field);
    font-size: var(--fs-2xs);
  }
  input::placeholder { color: var(--ink-3); }
  input:focus-visible { outline: 1px solid var(--ink-2); outline-offset: 1px; }
  ul { margin: 0; padding: 0; display: grid; gap: 0; max-height: 220px; overflow-y: auto; list-style: none; }
  li .hit-row { display: flex; align-items: stretch; border-top: 1px solid var(--rule); }
  li .hit {
    flex: 1 1 auto;
    min-width: 0;
    display: grid;
    gap: 1px;
    padding: 6px 8px 6px 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    cursor: pointer;
  }
  li .locate {
    flex: 0 0 auto;
    width: 24px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--ink-3);
    cursor: pointer;
  }
  li .locate:hover { color: var(--ink); background: var(--field); }
  li .hit:hover .snippet { color: var(--ink); }
  li .hit:hover .meta { color: var(--ink-2); }
  li button:focus-visible { outline: 2px solid var(--ink); outline-offset: -2px; }
  .meta { color: var(--ink-3); font: 400 var(--fs-2xs)/1.5 var(--font-mono); }
  .snippet { color: var(--ink-2); font-size: var(--fs-2xs); overflow-wrap: anywhere; }
  .error { color: var(--state-error); font-size: var(--fs-2xs); }
</style>

