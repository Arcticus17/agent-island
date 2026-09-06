<main class="svelte-island" aria-label="Agent Island">
  <span class="status-dot" aria-hidden="true"></span>
  <span>Agent Island</span>
</main>

<style>
  .svelte-island {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: currentColor;
  }
</style>
