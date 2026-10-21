<script lang="ts">
  import { PIXEL_GRID, spriteCells } from "./sprites";

  let {
    agentName = null,
    size = 16,
  }: { agentName?: string | null; size?: number } = $props();

  const cells = $derived(spriteCells(agentName));
</script>

<svg
  class="pixel-agent"
  viewBox={`0 0 ${PIXEL_GRID} ${PIXEL_GRID}`}
  width={size}
  height={size}
  aria-hidden="true"
  focusable="false"
  shape-rendering="crispEdges"
>
  {#each cells as cell (cell.key)}
    <rect x={cell.x} y={cell.y} width="1" height="1"></rect>
  {/each}
</svg>

<style>
  /* 单色精灵跟随状态色：.agent-island.status-* 已经设好 --island-status-current */
  .pixel-agent {
    display: block;
    fill: var(--island-status-current, currentColor);
    filter: drop-shadow(0 0 5px color-mix(in srgb, var(--island-status-current, currentColor) 55%, transparent));
  }
</style>
