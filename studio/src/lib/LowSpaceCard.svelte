<script lang="ts">
  // When the drive with the clips is nearly full, saves start failing (the
  // error sound plays). Checked when the window opens, when it comes back to
  // the front, when the clips folder changes, and every few minutes.
  import * as api from "./api";
  import Icon from "./Icon.svelte";
  import { library, openPage } from "./state.svelte";

  const LOW_GB = 5;

  let freeGb = $state<number | null>(null);

  $effect(() => {
    void library.clipsDir;
    const look = () => api.freeSpace().then((gb) => (freeGb = gb), () => {});
    look();
    const timer = setInterval(look, 5 * 60 * 1000);
    window.addEventListener("focus", look);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", look);
    };
  });

  const drive = $derived(/^[a-z]:/i.test(library.clipsDir) ? library.clipsDir.slice(0, 2).toUpperCase() : "the clips drive");
</script>

{#if freeGb != null && freeGb < LOW_GB}
  <div class="low">
    <div class="head">
      <Icon name="warning" size={16} />
      <b>Almost out of space</b>
    </div>
    <span>{freeGb} GB free on {drive}. Clips can't be saved once it's full.</span>
    <button onclick={() => openPage("settings", "storage")}>Storage settings</button>
  </div>
{/if}

<style>
  .low {
    --warn: #f2c84b;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .head :global(.icon) {
    color: var(--warn);
  }

  span {
    font-size: 12px;
    color: var(--muted);
  }

  button {
    padding: 6px 12px;
    font-size: 13px;
  }
</style>
