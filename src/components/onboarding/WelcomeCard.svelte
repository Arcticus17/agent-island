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
  /* 首次使用：三步说明，用发丝线和键盘键帽排版，不再是一个浮起的卡盒 */
  .welcome { display: grid; gap: 8px; padding: 12px 0; border-bottom: 1px solid var(--rule); }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .title { color: var(--ink); font-size: var(--fs-sm); font-weight: 650; }
  button {
    min-height: 22px;
    padding: 2px 8px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  button:hover { color: var(--ink); border-color: var(--rule-strong); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  ul { margin: 0; padding: 0; display: grid; gap: 5px; list-style: none; }
  li { display: grid; grid-template-columns: 96px 72px minmax(0, 1fr); align-items: baseline; gap: 10px; color: var(--ink-3); font-size: var(--fs-2xs); }
  kbd {
    justify-self: start;
    padding: 1px 5px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink);
    background: var(--field);
    font: 600 9.5px/1.5 var(--font-mono);
    white-space: nowrap;
  }
  .label { color: var(--ink-2); font-weight: 600; }
  .detail { color: var(--ink-3); }
  @media (max-width: 420px) { li { grid-template-columns: 88px minmax(0, 1fr); } .detail { grid-column: 2; } }
</style>
