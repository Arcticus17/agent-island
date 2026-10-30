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
  /* 卡片渲染失败：一条带错误色的板行，说明哪个卡片坏了、错误码是什么 */
  .card-fallback {
    min-width: 0;
    min-height: 64px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 10px 10px;
    border: 1px solid var(--state-error);
    border-radius: var(--radius-control);
    color: var(--ink);
    background: color-mix(in srgb, var(--state-error) 9%, transparent);
  }

  .card-fallback.wide { grid-column: 1 / -1; }

  .fallback-copy { min-width: 0; display: grid; gap: 4px; }

  .eyebrow { color: var(--state-error); font-size: var(--fs-2xs); font-weight: 650; }

  code {
    overflow: hidden;
    color: var(--ink-2);
    font: 400 var(--fs-2xs)/1.5 var(--font-mono);
    overflow-wrap: anywhere;
  }

  button {
    flex: 0 0 auto;
    min-height: 26px;
    padding: 3px 9px;
    border: 1px solid var(--rule-strong);
    border-radius: 4px;
    color: var(--ink);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }

  button:hover { background: var(--field); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }

  @media (max-width: 520px) { .card-fallback.wide { grid-column: auto; } }

  @media (max-width: 360px) {
    .card-fallback { align-items: stretch; flex-direction: column; }
    button { width: 100%; }
  }
</style>
