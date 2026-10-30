<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Snippet } from "svelte";

  import { CARD_IDS, type CardId, type CardPlacement, type LayoutConfigV1 } from "../../layout/schema";

  interface Props {
    layout: LayoutConfigV1;
    editing?: boolean;
    children?: Snippet<[CardPlacement]>;
    onMove?: (cardId: CardId, targetIndex: number) => void;
    beginInteraction?: () => void;
    endInteraction?: () => void;
    onResize?: (cardId: CardId, height: number) => void;
    emptyCardIds?: CardId[];
  }

  let {
    layout,
    editing = false,
    children,
    onMove,
    beginInteraction,
    endInteraction,
    onResize,
    emptyCardIds = [],
  }: Props = $props();

  let draggingId = $state<CardId | null>(null);
  const cardNames: Record<CardId, string> = { status: "状态", usage: "用量", session: "会话", log: "回执", stats: "统计" };
  let interactionActive = false;
  let resizing = $state<{ id: CardId; pointerId: number; startY: number; startHeight: number; height: number; node: HTMLElement } | null>(null);

  function finishResize(commit = false): void {
    const current = resizing;
    if (!current) return;
    resizing = null;
    try {
      if (current.node.hasPointerCapture(current.pointerId)) current.node.releasePointerCapture(current.pointerId);
      if (commit) onResize?.(current.id, current.height);
    } finally {
      finishDrag();
    }
  }

  function startResize(event: PointerEvent, cardId: CardId): void {
    if (!editing || event.button !== 0 || resizing || draggingId) return;
    event.preventDefault();
    event.stopPropagation();
    const node = event.currentTarget as HTMLElement;
    const height = node.parentElement!.getBoundingClientRect().height;
    try {
      node.setPointerCapture(event.pointerId);
      beginInteraction?.();
      interactionActive = true;
      resizing = { id: cardId, pointerId: event.pointerId, startY: event.clientY, startHeight: height, height, node };
    } catch {
      if (node.hasPointerCapture(event.pointerId)) node.releasePointerCapture(event.pointerId);
      finishDrag();
    }
  }

  function moveResize(event: PointerEvent): void {
    if (!resizing || resizing.pointerId !== event.pointerId) return;
    resizing.height = Math.max(96, Math.min(600, Math.round(resizing.startHeight + event.clientY - resizing.startY)));
  }

  function keyResize(event: KeyboardEvent, placement: CardPlacement): void {
    if (event.key === "Escape") { finishResize(); return; }
    const delta = event.key === "ArrowUp" ? -16 : event.key === "ArrowDown" ? 16 : 0;
    if (!delta && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    const height = (event.currentTarget as HTMLElement).parentElement!.getBoundingClientRect().height;
    onResize?.(placement.id, event.key === "Home" ? 96 : event.key === "End" ? 600 : Math.max(96, Math.min(600, Math.round(height + delta))));
  }

  function isCardId(value: string): value is CardId {
    return CARD_IDS.some((candidate) => candidate === value);
  }

  function startDrag(event: DragEvent, cardId: CardId): void {
    if (!editing || resizing || (event.target as HTMLElement).closest("button,select,input")) { event.preventDefault(); return; }
    draggingId = cardId;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", cardId);
    }
    if (!interactionActive) {
      try {
        beginInteraction?.();
        interactionActive = true;
      } catch {
        draggingId = null;
        interactionActive = false;
      }
    }
  }

  function allowDrop(event: DragEvent): void {
    if (!editing || draggingId === null) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  function finishDrag(): void {
    draggingId = null;
    if (!interactionActive) return;
    interactionActive = false;
    endInteraction?.();
  }

  function releaseOnDragCancel(node: HTMLElement): { destroy: () => void } {
    const release = (): void => finishDrag();
    node.addEventListener("dragcancel", release);
    return {
      destroy: () => node.removeEventListener("dragcancel", release),
    };
  }

  function dropCard(event: DragEvent, targetIndex: number): void {
    if (!editing) return;
    event.preventDefault();
    const transferred = event.dataTransfer?.getData("text/plain") ?? "";
    const transferredId = isCardId(transferred)
      && layout.cards.some(({ id }) => id === transferred)
      ? transferred
      : null;
    const cardId = draggingId !== null
      && (transferred === "" || transferredId === draggingId)
      ? draggingId
      : null;
    try {
      if (cardId) onMove?.(cardId, targetIndex);
    } finally {
      finishDrag();
    }
  }

  $effect(() => {
    if (!editing) { finishResize(); finishDrag(); }
  });

  onDestroy(() => { finishResize(); finishDrag(); });
</script>

<div class="card-grid" data-testid="card-grid" data-editing={editing}>
  {#each layout.cards as placement, index (placement.id)}
    {#if placement.visible}
      <article
        class={`layout-card size-${placement.size}`}
        class:dragging={draggingId === placement.id}
        class:empty-card={!editing && emptyCardIds.includes(placement.id) && placement.height === undefined}
        class:custom-height={placement.height !== undefined || resizing?.id === placement.id}
        style:height={resizing?.id === placement.id ? `${resizing.height}px` : placement.height !== undefined ? `${placement.height}px` : undefined}
        data-testid="layout-card"
        data-card-id={placement.id}
        data-card-index={index}
        draggable={editing}
        ondragstart={(event) => startDrag(event, placement.id)}
        ondragover={allowDrop}
        ondrop={(event) => dropCard(event, index)}
        ondragend={finishDrag}
        use:releaseOnDragCancel
      >
        {#if children}
          {@render children(placement)}
        {:else}
          <span>{placement.id}</span>
        {/if}
        {#if editing}
          <button type="button" class="resize-handle" data-testid="card-resize-handle" data-card-id={placement.id}
            aria-label={`调整${cardNames[placement.id]}卡片高度`} title="拖动调整高度；方向键微调，Home 最小，End 最大"
            onpointerdown={(event) => startResize(event, placement.id)} onpointermove={moveResize}
            onpointerup={(event) => { if (event.pointerId === resizing?.pointerId) finishResize(true); }}
            onpointercancel={() => finishResize()} onlostpointercapture={() => finishResize()}
            onkeydown={(event) => keyResize(event, placement)}>↕</button>
        {/if}
      </article>
    {/if}
  {/each}
</div>

<style>
  /*
   * 板面：不是"两张卡片并排"，而是被 1px 发丝分成格的行。
   * 列与列之间一条竖线，行与行之间一条横线，没有盒子、没有投影、没有圆角。
   */
  .card-grid {
    width: 100%;
    max-width: 100%;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0;
    min-width: 0;
  }

  .layout-card {
    position: relative;
    box-sizing: border-box;
    min-width: 0;
    overflow: visible;
    border: 0;
    border-top: 1px solid var(--rule);
    border-radius: 0;
    color: var(--ink);
    background: transparent;
    padding: 13px 14px;
    --layout-log-height: 164px;
  }

  .layout-card.size-compact { padding-block: 9px; --layout-log-height: 96px; }
  .layout-card.size-wide { padding-block: 16px; grid-column: 1 / -1; --layout-log-height: 240px; }

  /* 两列时：左列一条竖线 + 右留白，右列左留白。整行卡片单独占满，不画竖线。 */
  .layout-card:nth-child(odd):not(.size-wide) { padding-right: 14px; border-right: 1px solid var(--rule); }
  .layout-card:nth-child(even):not(.size-wide) { padding-left: 14px; }
  .layout-card:first-child { border-top: 0; }

  .layout-card[draggable="true"] { cursor: grab; }
  .layout-card[draggable="true"]:active { cursor: grabbing; }

  .layout-card.dragging { opacity: .5; }

  .layout-card.custom-height { padding-bottom: 20px; }
  .layout-card.empty-card { align-self: start; padding-block: 8px; }

  .resize-handle {
    position: absolute; bottom: 0; left: 0; width: 100%; height: 16px;
    padding: 0; border: 0; border-top: 1px dashed var(--rule-strong);
    color: var(--ink-3); background: transparent;
    cursor: ns-resize; touch-action: none; font-size: 11px;
  }
  .resize-handle:hover { color: var(--ink); background: var(--field); }
  .resize-handle:focus-visible { outline: 2px solid var(--ink); outline-offset: -2px; }

  @media (max-width: 420px) {
    .card-grid { grid-template-columns: minmax(0, 1fr); }
    .layout-card,
    .layout-card.size-compact,
    .layout-card.size-standard,
    .layout-card.size-wide {
      grid-column: auto;
      max-width: 100%;
      padding-inline: 14px;
      border-right: 0;
    }
  }
</style>
