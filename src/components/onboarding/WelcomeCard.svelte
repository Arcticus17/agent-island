<script lang="ts">
  import { ONBOARDING_STEPS } from "../../onboarding/state";

  let { onDismiss }: { onDismiss: () => void } = $props();
</script>

<section class="welcome" data-testid="welcome-card" aria-label="上手引导">
  <div class="head">
    <span class="title">第一次用？三件事就够</span>
    <button type="button" data-testid="welcome-dismiss" onclick={onDismiss}>知道了</button>
  </div>
  <ul>
    {#each ONBOARDING_STEPS as step (step.keys)}
      <li>
        <kbd>{step.keys}</kbd>
        <span class="label">{step.title}</span>
        <span class="detail">{step.detail}</span>
      </li>
    {/each}
  </ul>
</section>

<style>
  .welcome { display: grid; gap: 8px; padding: 10px 12px; border: 1px solid color-mix(in srgb, var(--island-accent-primary) 35%, var(--island-border-subtle)); border-radius: 14px; background: color-mix(in srgb, var(--island-accent-primary) 8%, var(--island-surface-raised)); }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .title { color: var(--island-text-primary); font-size: 12px; font-weight: 700; }
  button { min-height: 24px; padding: 2px 10px; border: 1px solid var(--island-accent-primary); border-radius: var(--island-radius-control); color: var(--island-accent-primary); background: transparent; font-size: 11px; cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
  ul { margin: 0; padding: 0; display: grid; gap: 5px; list-style: none; }
  li { display: flex; align-items: baseline; gap: 7px; flex-wrap: wrap; color: var(--island-text-secondary); font-size: 11px; }
  kbd { padding: 1px 5px; border: 1px solid var(--island-border-subtle); border-radius: 5px; background: var(--island-control-surface); color: var(--island-text-primary); font: 600 10px/1.5 ui-monospace, "Cascadia Code", monospace; }
  .label { color: var(--island-text-primary); }
  .detail { color: var(--island-text-tertiary); }
</style>
