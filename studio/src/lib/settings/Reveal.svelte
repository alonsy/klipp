<script lang="ts">
  // Opens and closes its content smoothly by its real height, whatever that is.
  // The content stays mounted; while closed it can't be focused or read.
  import type { Snippet } from "svelte";

  let { open, children }: { open: boolean; children: Snippet } = $props();
</script>

<div class="reveal" class:open>
  <div class="inner" inert={!open}>{@render children()}</div>
</div>

<style>
  .reveal {
    display: grid;
    grid-template-rows: 0fr;
    min-width: 0;
    transition: grid-template-rows var(--dur) var(--ease-out);
  }

  .reveal.open {
    grid-template-rows: 1fr;
    transition-duration: var(--dur-slow);
  }

  /* The margin and padding keep focus rings from being clipped. */
  .inner {
    min-width: 0;
    min-height: 0;
    margin: -4px;
    padding: 4px;
    overflow: hidden;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out);
  }

  .open .inner {
    opacity: 1;
    transition: opacity var(--dur) var(--ease-out) 60ms;
  }
</style>
