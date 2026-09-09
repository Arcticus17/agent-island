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
  }

  let {
    layout,
    editing = false,
    children,
    onMove,
    beginInteraction,
    endInteraction,
  }: Props = $props();

  let draggingId = $state<CardId | null>(null);
  let interactionActive = false;

  function isCardId(value: string): value is CardId {
    return CARD_IDS.some((candidate) => candidate === value);
  }

  function startDrag(event: DragEvent, cardId: CardId): void {
    if (!editing) return;
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
    if (!editing) finishDrag();
  });

  onDestroy(finishDrag);
</script>

<div class="card-grid" data-testid="card-grid" data-editing={editing}>
  {#each layout.cards as placement, index (placement.id)}
    {#if placement.visible}
      <article
        class={`layout-card size-${placement.size}`}
        class:dragging={draggingId === placement.id}
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
      </article>
    {/if}
  {/each}
</div>

<style>
  .card-grid {
    width: 100%;
    max-width: 100%;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--island-space-card, 10px);
    min-width: 0;
  }

  .layout-card {
    min-width: 0;
    min-height: 104px;
    overflow: hidden;
    border: 1px solid var(--island-border-subtle, ButtonBorder);
    border-radius: var(--island-radius-card, 16px);
    color: var(--island-text-primary, CanvasText);
    background: var(--island-surface-card, var(--island-surface-raised, Canvas));
  }

  .layout-card.size-compact {
    min-height: 76px;
  }

  .layout-card.size-standard {
    min-height: 104px;
  }

  .layout-card.size-wide {
    grid-column: 1 / -1;
    min-height: 116px;
  }

  .layout-card[draggable="true"] {
    cursor: grab;
  }

  .layout-card[draggable="true"]:active {
    cursor: grabbing;
  }

  .layout-card.dragging {
    opacity: 0.62;
    outline: 2px solid var(--island-accent-primary, Highlight);
    outline-offset: 2px;
  }

  @media (max-width: 420px) {
    .card-grid {
      grid-template-columns: minmax(0, 1fr);
    }

    .layout-card,
    .layout-card.size-compact,
    .layout-card.size-standard,
    .layout-card.size-wide {
      grid-column: auto;
      max-width: 100%;
    }
  }
</style>
