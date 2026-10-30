<script lang="ts">
  import type { DisplayStatus } from "../../bridge/types";
  import { PIXEL_GRID, spriteCells } from "./sprites";

  let {
    agentName = null,
    size = 16,
    status = null,
  }: { agentName?: string | null; size?: number; status?: DisplayStatus | null } = $props();

  const cells = $derived(spriteCells(agentName));
  /** Agent 没在跑（停止/空闲/完成/报错）时也让它"活着"：缓慢呼吸 + 偶尔眨眼。 */
  const resting = $derived(
    status === "stopped" || status === "idle" || status === "done" || status === "error",
  );
</script>

<svg
  class="pixel-agent"
  class:resting
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
    transform-origin: 50% 92%;
  }
  /* 静止态：3.6s 一轮的轻微上浮 + 压缩"眨眼"，像素图用 step 会显得硬，这里保持平滑 */
  .pixel-agent.resting { animation: mascot-rest 3.6s ease-in-out infinite; }
  @keyframes mascot-rest {
    0%, 100% { transform: translateY(0) scaleY(1); }
    38% { transform: translateY(-0.8px) scaleY(1.05); }
    62% { transform: translateY(0) scaleY(1); }
    70% { transform: translateY(0) scaleY(0.84); }
    76% { transform: translateY(0) scaleY(1.02); }
    84%, 100% { transform: translateY(0) scaleY(1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .pixel-agent.resting { animation: none; }
  }
</style>
