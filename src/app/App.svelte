<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import "../themes/graphite-glass.css";
  import CompactIsland from "../components/island/CompactIsland.svelte";
  import ExpandedIsland from "../components/island/ExpandedIsland.svelte";
  import { tauriBridge } from "../bridge/tauri";
  import type { AgentIslandBridge } from "../bridge/types";
  import { createAgentStore, type AgentStoreState } from "../stores/agent-store";

  let {
    bridge = tauriBridge,
    pollIntervalMs = 1_500,
    initiallyExpanded = false,
  }: {
    bridge?: AgentIslandBridge;
    pollIntervalMs?: number;
    initiallyExpanded?: boolean;
  } = $props();

  const store = createAgentStore(untrack(() => bridge));
  let viewState: AgentStoreState = $state(store.current());
  let expanded = $state(untrack(() => initiallyExpanded));
  let islandElement = $state<HTMLElement>();
  let resizeRevision = 0;
  const collapsedWindowHeight = 60;

  async function syncWindowSize(): Promise<void> {
    const revision = ++resizeRevision;
    const targetExpanded = expanded;
    await tick();
    if (revision !== resizeRevision) return;
    const renderedHeight = islandElement
      ? Math.ceil(Math.max(
          islandElement.scrollHeight,
          islandElement.getBoundingClientRect().height,
        ))
      : 0;
    const availableHeight = window.screen?.availHeight || renderedHeight;
    const safeExpandedHeight = Math.max(
      collapsedWindowHeight,
      Math.floor(availableHeight * 0.92),
    );
    const height = targetExpanded
      ? Math.min(
          Math.max(collapsedWindowHeight, renderedHeight),
          safeExpandedHeight,
        )
      : collapsedWindowHeight;
    try {
      await bridge.resizeWindow?.({
        width: Math.max(1, Math.round(window.innerWidth)),
        height,
      });
    } catch {
      // A browser preview or closing Tauri window may not support resizing.
    }
  }

  function toggleExpanded(): void {
    expanded = !expanded;
    void syncWindowSize();
  }

  onMount(() => {
    const unsubscribe = store.subscribe((next) => {
      viewState = next;
      void syncWindowSize();
    });
    void store.refresh();
    const timer = pollIntervalMs > 0
      ? window.setInterval(() => void store.refresh(), pollIntervalMs)
      : undefined;
    return () => {
      resizeRevision += 1;
      unsubscribe();
      if (timer !== undefined) window.clearInterval(timer);
    };
  });
</script>

{#if viewState.snapshot}
  <main
    class={`agent-island status-${viewState.selectedAgent?.display_status ?? "idle"}`}
    class:expanded
    data-testid="agent-island"
    aria-label="Agent Island"
    bind:this={islandElement}
  >
    <span class="sr-only" data-testid="selected-agent-id">{viewState.selectedAgentId ?? ""}</span>
    <CompactIsland agent={viewState.selectedAgent} {expanded} onToggle={toggleExpanded} />
    {#if expanded}
      <ExpandedIsland
        agents={viewState.snapshot.agents}
        selectedAgent={viewState.selectedAgent}
        session={viewState.selectedAgent?.active_session ?? null}
        refreshError={viewState.refreshError}
        onSelectAgent={(id) => store.selectAgent(id)}
      />
    {/if}
  </main>
{:else}
  <div class="island-loading" aria-live="polite" bind:this={islandElement}>正在连接 Agent Island…</div>
{/if}

<style>
  .agent-island, .island-loading {
    box-sizing: border-box;
    width: min(720px, calc(100vw - 24px));
    color: var(--island-text-primary);
    font: 500 14px/1.45 Inter, ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  .agent-island {
    overflow: hidden;
    border: 1px solid var(--island-border-subtle);
    border-radius: 24px;
    background: var(--island-background);
    box-shadow: var(--island-shadow);
    backdrop-filter: blur(22px) saturate(125%);
  }
  .agent-island.expanded { max-height: 100vh; overflow-y: auto; scrollbar-color: var(--island-scrollbar-thumb) var(--island-scrollbar-track); }
  .island-loading { padding: 12px 18px; border: 1px solid var(--island-border-subtle); border-radius: 999px; background: var(--island-surface); }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  @media (max-width: 420px) { .agent-island, .island-loading { width: calc(100vw - 16px); } .agent-island { border-radius: 20px; } }
</style>
