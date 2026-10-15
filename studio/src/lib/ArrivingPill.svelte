<script lang="ts">
  // "A clip is arriving" wherever the clips page's own placeholder isn't in
  // view: in the editor, in settings, or with the clips sorted another way.
  import { fly } from "svelte/transition";
  import { filters, incoming, ui } from "./state.svelte";

  let onClips = $derived(!ui.clip && ui.page === "clips" && filters.sort === "newest" && filters.kind !== "favorites");
  let n = $derived(incoming.list.length);
</script>

{#if n > 0 && !onClips}
  <div class="pill" transition:fly={{ y: 12, duration: 200 }} aria-live="polite">
    <span class="spinner"></span>
    {n === 1 ? "A clip is arriving" : `${n} clips are arriving`}
  </div>
{/if}

<style>
  .pill {
    position: fixed;
    left: 20px;
    bottom: 20px;
    z-index: 55;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 16px 9px 12px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--line-strong);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.4);
    font-size: 13px;
    font-weight: 550;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--accent);
    border-right-color: transparent;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
