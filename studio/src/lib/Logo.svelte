<script lang="ts">
  import { markSvg } from "./icons";

  let { size = 26 }: { size?: number } = $props();
</script>

<span class="logo">
  {@html markSvg(size)}
  <span class="word" style="font-size: {size * 0.78}px">klipp</span>
</span>

<style>
  .logo {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    line-height: 1;
  }

  .word {
    font-family: var(--font-display);
    font-weight: 700;
    letter-spacing: -0.03em;
    color: var(--text);
  }
</style>
