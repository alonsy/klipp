<script lang="ts">
  // "A clip is arriving" wherever the clips page's own placeholder isn't in
  // view: in the editor, in settings, or with the clips sorted another way.
  import { DUR, enter, leave, spring } from "./motion";
  import { filters, incoming, ui } from "./state.svelte";

  let onClips = $derived(!ui.clip && ui.page === "clips" && filters.sort === "newest" && filters.kind !== "favorites");
  let n = $derived(incoming.list.length);
</script>

{#if n > 0 && !onClips}
  <div class="dock">
    <div class="pill" in:enter={{ y: 12, duration: DUR.spring, easing: spring }} out:leave={{ y: 8 }} aria-live="polite">
      <span class="spinner"></span>
      {n === 1 ? "A clip is arriving" : `${n} clips are arriving`}
    </div>
  </div>
{/if}

<style>
  /* Top centre, in the title bar's empty space: clear of the sidebar's status,
     the toasts and the editor's controls, on every page. */
  .dock {
    position: fixed;
    left: 0;
    right: 0;
    top: var(--sp-2);
    z-index: 55;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-full);
    background: var(--surface-3);
    box-shadow: var(--shadow-2);
    font-size: var(--fs-control);
    font-weight: 550;
  }

  .spinner {
    color: var(--accent);
  }
</style>
