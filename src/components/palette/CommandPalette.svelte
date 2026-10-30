<script lang="ts">
  import { tick } from "svelte";
  import { filterCommands, moveSelection, type PaletteCommand } from "../../palette/commands";
  import type { Appearance } from "../../themes/theme-style";

  let {
    commands,
    onRun,
    onClose,
    appearance = "dark",
  }: {
    commands: readonly PaletteCommand[];
    onRun: (command: PaletteCommand) => void;
    onClose: () => void;
    appearance?: Appearance;
  } = $props();

  let query = $state("");
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const results = $derived(filterCommands(commands, query));

  $effect(() => {
    void tick().then(() => input?.focus());
  });

  // 过滤结果变短时把选中项拉回范围内
  $effect(() => {
    if (active >= results.length) active = Math.max(0, results.length - 1);
  });

  function keydown(event: KeyboardEvent): void {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      active = moveSelection(active, 1, results.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      active = moveSelection(active, -1, results.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const command = results[active];
      if (command) onRun(command);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<div class="palette-backdrop" data-theme-style={appearance} role="presentation" onpointerdown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="命令面板" data-testid="palette">
    <input
      bind:this={input}
      bind:value={query}
      data-testid="palette-input"
      class="search"
      type="text"
      placeholder="搜索 Agent、会话或动作…"
      aria-label="搜索命令"
      onkeydown={keydown}
    />
    {#if results.length === 0}
      <p class="empty" data-testid="palette-empty">没有匹配项</p>
    {:else}
      <ul class="results" role="listbox" data-testid="palette-results">
        {#each results as command, index (command.id)}
          <li
            role="option"
            aria-selected={index === active}
            class:active={index === active}
            data-testid="palette-option"
            data-command-id={command.id}
            onpointerenter={() => (active = index)}
            onclick={() => onRun(command)}
            onkeydown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onRun(command);
              }
            }}
          >
            <span class="title">{command.title}</span>
            <span class="hint">{command.hint}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="footer">↑↓ 选择 · Enter 执行 · Esc 关闭</p>
  </div>
</div>

<style>
  /* 命令面板：整块板上的一个浮层，遮罩用主题里的 scrim，不用模糊 */
  .palette-backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-overlay, 40);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 56px;
    background: var(--scrim);
  }
  .palette {
    width: min(380px, calc(100% - 32px));
    display: grid;
    gap: 6px;
    padding: 10px 12px 8px;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-event);
    background: var(--ground-raised);
  }
  .search {
    min-height: 34px;
    padding: 6px 9px;
    border: 1px solid var(--rule);
    border-radius: 6px;
    color: var(--ink);
    background: var(--field);
    font: 400 var(--fs-sm)/1.4 var(--font-ui);
  }
  .search::placeholder { color: var(--ink-3); }
  .search:focus-visible { outline: 1px solid var(--ink-2); outline-offset: 1px; }
  .results { margin: 0; padding: 0; display: grid; gap: 0; max-height: 280px; overflow-y: auto; list-style: none; }
  .results li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 8px;
    border-top: 1px solid var(--rule);
    border-radius: 0;
    color: var(--ink-2);
    font-size: var(--fs-xs);
    cursor: pointer;
  }
  .results li:first-child { border-top: 0; }
  .results li.active { color: var(--ink); background: var(--field); }
  .title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { flex: 0 0 auto; color: var(--ink-3); font-size: var(--fs-2xs); }
  .empty, .footer { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); }
  .footer { padding-top: 6px; border-top: 1px solid var(--rule); text-align: right; }
</style>
