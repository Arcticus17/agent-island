<script lang="ts">
  import LayoutEditor from "../../../src/components/layout/LayoutEditor.svelte";
  import type { LayoutConfigV1 } from "../../../src/layout/schema";

  interface Props {
    initialLayout: LayoutConfigV1;
    onLayoutChange?: (layout: LayoutConfigV1) => void;
  }

  let { initialLayout: layout = $bindable(), onLayoutChange }: Props = $props();

  export function replaceLayout(next: LayoutConfigV1): void {
    layout = next;
  }
</script>

<LayoutEditor bind:layout {onLayoutChange} />
