<script lang="ts">
  import type { Snippet } from "svelte";

  import { DEFAULT_LAYOUTS, type DefaultLayoutName } from "../../layout/presets";
  import { moveCard, resizeCard, setCardVisible, setCardHeight } from "../../layout/reorder";
  import {
    CARD_SIZES,
    REQUIRED_CARD_IDS,
    type CardId,
    type CardPlacement,
    type CardSize,
    type LayoutConfigV1,
  } from "../../layout/schema";
  import CardGrid from "./CardGrid.svelte";

  interface Props {
    layout: LayoutConfigV1;
    content?: Snippet<[CardPlacement]>;
    onLayoutChange?: (layout: LayoutConfigV1) => void;
    beginInteraction?: () => void;
    endInteraction?: () => void;
    emptyCardIds?: CardId[];
  }

  let {
    layout = $bindable(),
    content,
    onLayoutChange,
    beginInteraction,
    endInteraction,
    emptyCardIds = [],
  }: Props = $props();

  const cardNames: Record<CardId, string> = {
    status: "状态",
    usage: "用量",
    session: "会话",
    log: "回执",
    stats: "统计",
  };
  const sizeNames: Record<CardSize, string> = {
    compact: "紧凑",
    standard: "标准",
    wide: "宽版",
  };
  const presetNames: Record<DefaultLayoutName, string> = {
    minimal: "极简",
    monitoring: "监控",
    debugging: "调试",
  };
  const presetIds = ["minimal", "monitoring", "debugging"] as const satisfies readonly DefaultLayoutName[];

  let editing = $state(false);
  let resetPreset = $state<DefaultLayoutName>(
    layout.preset === "custom" ? "monitoring" : layout.preset,
  );
  let hiddenCards = $derived(layout.cards.filter(({ visible }) => !visible));
  let visibleCards = $derived(layout.cards.filter(({ visible }) => visible));

  $effect(() => {
    if (layout.preset !== "custom") resetPreset = layout.preset;
  });

  function copyLayout(value: LayoutConfigV1): LayoutConfigV1 {
    return {
      version: 1,
      preset: value.preset,
      cards: value.cards.map((card) => ({ ...card })),
    };
  }

  function apply(next: LayoutConfigV1): void {
    if (next === layout) return;
    layout = next;
    onLayoutChange?.(copyLayout(next));
  }

  function move(cardId: CardId, targetIndex: number): void {
    apply(moveCard(layout, cardId, targetIndex));
  }

  function moveVisible(cardId: CardId, direction: -1 | 1): void {
    const sourceVisibleIndex = visibleCards.findIndex(({ id }) => id === cardId);
    const target = visibleCards[sourceVisibleIndex + direction];
    if (!target) return;
    move(cardId, layout.cards.findIndex(({ id }) => id === target.id));
  }

  function cannotMoveVisible(cardId: CardId, direction: -1 | 1): boolean {
    const sourceVisibleIndex = visibleCards.findIndex(({ id }) => id === cardId);
    return sourceVisibleIndex < 0 || !visibleCards[sourceVisibleIndex + direction];
  }

  function resize(cardId: CardId, event: Event): void {
    apply(resizeCard(layout, cardId, (event.currentTarget as HTMLSelectElement).value));
  }

  function setVisible(cardId: CardId, visible: boolean): void {
    apply(setCardVisible(layout, cardId, visible));
  }

  function usePreset(preset: DefaultLayoutName): void {
    resetPreset = preset;
    apply(copyLayout(DEFAULT_LAYOUTS[preset]));
  }

  function reset(): void {
    apply(copyLayout(DEFAULT_LAYOUTS[resetPreset]));
  }

  function isRequired(cardId: CardId): boolean {
    return REQUIRED_CARD_IDS.some((candidate) => candidate === cardId);
  }
</script>

<section class="layout-editor" data-testid="layout-editor" data-editing={editing} aria-label="卡片布局">
  <CardGrid
    {layout}
    {editing}
    {beginInteraction}
    {endInteraction}
    {emptyCardIds}
    onResize={(id, height) => apply(setCardHeight(layout, id, height))}
    onMove={move}
  >
    {#snippet children(placement: CardPlacement)}
      <div class="card-shell">
        {#if editing}
          <div class="card-heading">
            <div>
              <span class="drag-hint" aria-hidden="true">⋮⋮</span>
              <span class="card-name">{cardNames[placement.id]}</span>
            </div>
            <span class="size-badge">{placement.height ? `${placement.height}px` : sizeNames[placement.size]}</span>
          </div>
        {/if}

        <div class="card-content">
          {#if content}
            {@render content(placement)}
          {:else}
            <span>{cardNames[placement.id]}内容区域</span>
          {/if}
        </div>

        {#if editing}
          <div class="card-controls" data-testid="layout-card-controls">
            <button
              data-testid="card-move-up"
              data-card-id={placement.id}
              type="button"
              aria-label={`上移${cardNames[placement.id]}卡片`}
              disabled={cannotMoveVisible(placement.id, -1)}
              onclick={() => moveVisible(placement.id, -1)}
            >↑</button>
            <button
              data-testid="card-move-down"
              data-card-id={placement.id}
              type="button"
              aria-label={`下移${cardNames[placement.id]}卡片`}
              disabled={cannotMoveVisible(placement.id, 1)}
              onclick={() => moveVisible(placement.id, 1)}
            >↓</button>
            <label>
              <span class="sr-only">调整{cardNames[placement.id]}卡片尺寸</span>
              <select
                data-testid="card-size"
                data-card-id={placement.id}
                aria-label={`调整${cardNames[placement.id]}卡片尺寸`}
                value={placement.size}
                onchange={(event) => resize(placement.id, event)}
              >
                {#each CARD_SIZES as size (size)}
                  <option value={size}>{sizeNames[size]}</option>
                {/each}
              </select>
            </label>
            {#if !isRequired(placement.id)}
              <button
                data-testid="card-hide"
                data-card-id={placement.id}
                type="button"
                aria-label={`隐藏${cardNames[placement.id]}卡片`}
                onclick={() => setVisible(placement.id, false)}
              >隐藏</button>
            {/if}
          </div>
        {/if}
      </div>
    {/snippet}
  </CardGrid>

  {#if editing}
    <div class="preset-toolbar" aria-label="布局预设">
      {#each presetIds as preset (preset)}
        <button
          class:active={resetPreset === preset}
          data-testid="layout-preset"
          data-preset={preset}
          type="button"
          aria-label={`应用${presetNames[preset]}布局`}
          aria-pressed={resetPreset === preset}
          onclick={() => usePreset(preset)}
        >{presetNames[preset]}</button>
      {/each}
      <button
        data-testid="layout-reset"
        type="button"
        aria-label={`重置为${presetNames[resetPreset]}布局`}
        onclick={reset}
      >重置</button>
    </div>
  {/if}

  <div class="board-toolbar">
    <select
      class="preset-select"
      data-testid="layout-preset-select"
      aria-label="布局预设"
      title="切换布局预设：极简只留状态与对话"
      value={layout.preset}
      onchange={(event) => {
        const value = event.currentTarget.value;
        if (value !== "custom") usePreset(value as DefaultLayoutName);
      }}
    >
      {#each presetIds as preset (preset)}
        <option value={preset}>{presetNames[preset]}</option>
      {/each}
      {#if layout.preset === "custom"}<option value="custom">自定义</option>{/if}
    </select>
    <button
      class="edit-toggle"
      class:active={editing}
      data-testid="layout-edit-toggle"
      type="button"
      aria-label={editing ? "完成布局编辑" : "编辑布局"}
      aria-pressed={editing}
      onclick={() => { editing = !editing; }}
    >{editing ? "完成" : "编辑"}</button>
  </div>

  {#if editing && hiddenCards.length > 0}
    <div class="restore-tray" aria-label="已隐藏卡片">
      <span>已隐藏</span>
      {#each hiddenCards as placement (placement.id)}
        <button
          data-testid="card-restore"
          data-card-id={placement.id}
          type="button"
          aria-label={`恢复${cardNames[placement.id]}卡片`}
          onclick={() => setVisible(placement.id, true)}
        >+ {cardNames[placement.id]}</button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .layout-editor {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
    color: var(--ink);
  }

  /* 板面在上，布局工具在下：工具属于设置，不属于要看的数据。 */
  .preset-toolbar { order: 2; }
  .board-toolbar { order: 3; }
  .restore-tray { order: 4; }

  .card-shell {
    box-sizing: border-box;
    height: 100%;
    min-height: inherit;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .card-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    color: var(--ink-3);
  }
  .card-heading > div { min-width: 0; display: flex; align-items: center; gap: 6px; }
  .card-name { font-size: var(--fs-2xs); font-weight: 650; letter-spacing: var(--tracking-label); }
  .drag-hint { font-size: 11px; letter-spacing: -.2em; }
  .size-badge { flex: 0 0 auto; font: 500 var(--fs-2xs)/1 var(--font-readout); font-variant-numeric: tabular-nums; }

  .card-content { min-width: 0; flex: 1; min-height: 0; }
  .card-content > span { color: var(--ink-3); font-size: var(--fs-xs); }

  .card-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    padding-top: 8px;
    border-top: 1px dashed var(--rule-strong);
  }

  .board-toolbar,
  .preset-toolbar,
  .restore-tray {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .board-toolbar { justify-content: flex-end; padding-top: 8px; border-top: 1px solid var(--rule); }
  .preset-toolbar { flex-wrap: wrap; padding: 8px 0 0; border-top: 1px solid var(--rule); }
  .preset-toolbar button:last-child { margin-left: auto; }
  .restore-tray { flex-wrap: wrap; padding-top: 8px; }
  .restore-tray > span { margin-right: 2px; color: var(--ink-3); font-size: var(--fs-2xs); }

  button,
  select {
    min-height: 26px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-control);
    color: var(--ink-2);
    background: transparent;
    font: inherit;
    font-size: var(--fs-2xs);
  }
  button { padding: 3px 9px; cursor: pointer; }
  select { padding: 2px 6px; }
  button:hover:not(:disabled), button.active { color: var(--ink); border-color: var(--rule-strong); background: var(--field); }
  button:disabled { opacity: .35; cursor: default; }
  button:focus-visible, select:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .edit-toggle.active { color: var(--ink); border-color: var(--ink-2); }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
  }

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after {
      scroll-behavior: auto !important;
      transition-duration: 1ms !important;
      animation-duration: 1ms !important;
      animation-iteration-count: 1 !important;
    }
  }
</style>
