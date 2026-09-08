<script lang="ts">
  import CardBoundary from "../../../src/components/common/CardBoundary.svelte";
  import ThrowingCard from "./ThrowingCard.svelte";

  let shouldThrow = $state(true);
  let interactionCount = $state(0);
</script>

<main data-testid="boundary-harness">
  <CardBoundary cardName="会话卡片" errorCode="session_card_render_failed">
    <ThrowingCard {shouldThrow} />
  </CardBoundary>

  <article data-testid="healthy-card">
    <span data-testid="healthy-count">{interactionCount}</span>
    <button type="button" data-testid="healthy-action" onclick={() => interactionCount += 1}>
      操作正常卡片
    </button>
    <button type="button" data-testid="repair-failed-card" onclick={() => shouldThrow = false}>
      修复故障来源
    </button>
  </article>
</main>
