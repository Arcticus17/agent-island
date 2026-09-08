<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    cardName: string;
    errorCode?: string;
    wide?: boolean;
    children: Snippet;
    onError?: (error: unknown) => void;
  }

  let {
    cardName,
    errorCode = "card_render_failed",
    wide = false,
    children,
    onError,
  }: Props = $props();

  function reportError(error: unknown): void {
    onError?.(error);
  }
</script>

{#snippet failed(_error: unknown, reset: () => void)}
  <section
    class:wide
    class="card-fallback"
    data-testid="card-fallback"
    data-error-code={errorCode}
    role="alert"
  >
    <div class="fallback-copy">
      <span class="eyebrow">{cardName}暂时不可用</span>
      <code>{errorCode}</code>
    </div>
    <button type="button" aria-label={`重新加载${cardName}`} onclick={reset}>
      重新加载
    </button>
  </section>
{/snippet}

<svelte:boundary {failed} onerror={reportError}>
  {@render children()}
</svelte:boundary>

<style>
  .card-fallback {
    min-width: 0;
    min-height: 92px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 13px;
    border: 1px solid var(--island-border-subtle);
    border-radius: 16px;
    color: var(--island-text-primary);
    background: var(--island-surface-raised);
  }

  .card-fallback.wide {
    grid-column: 1 / -1;
  }

  .fallback-copy {
    min-width: 0;
    display: grid;
    gap: 6px;
  }

  .eyebrow {
    color: var(--island-refresh-error-text);
    font-size: 11px;
    font-weight: 700;
  }

  code {
    overflow: hidden;
    color: var(--island-text-secondary);
    font: 500 10px/1.4 ui-monospace, "Cascadia Code", monospace;
    overflow-wrap: anywhere;
  }

  button {
    flex: 0 0 auto;
    min-height: 36px;
    padding: 7px 11px;
    border: 1px solid var(--island-border-subtle);
    border-radius: 10px;
    color: var(--island-text-primary);
    background: var(--island-control-surface);
    cursor: pointer;
  }

  button:hover {
    background: var(--island-control-hover);
  }

  button:focus-visible {
    outline: 2px solid var(--island-accent-primary);
    outline-offset: 2px;
  }

  @media (max-width: 520px) {
    .card-fallback.wide {
      grid-column: auto;
    }
  }

  @media (max-width: 360px) {
    .card-fallback {
      align-items: stretch;
      flex-direction: column;
    }

    button {
      width: 100%;
    }
  }
</style>
