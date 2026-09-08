<script lang="ts">
  let { shouldThrow }: { shouldThrow: boolean } = $props();

  const canRender = $derived.by(() => {
    if (shouldThrow) throw new Error("synthetic_card_render_failure");
    return true;
  });
</script>

{#if canRender}<article data-testid="recovered-card">故障卡片已恢复</article>{/if}
