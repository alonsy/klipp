<script lang="ts">
  // When the drive with the clips is nearly full, saves start failing (the
  // error sound plays). Checked when the window opens, when it comes back to
  // the front, when the clips folder changes, and every few minutes.
  import * as api from "./api";
  import Icon from "./Icon.svelte";
  import { enter, leave } from "./motion";
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

  const drive = $derived(/^[a-z]:/i.test(library.clipsDir) ? `drive ${library.clipsDir[0].toUpperCase()}` : "the clips drive");
</script>

{#if freeGb != null && freeGb < LOW_GB}
  <div class="low" in:enter={{ y: 8 }} out:leave={{ y: 4 }}>
    <div class="head">
      <Icon name="warning" size={16} />
      <b>Almost out of space</b>
    </div>
    <p>{freeGb} GB free on {drive}. Clips can't be saved once it's full.</p>
    <button onclick={() => openPage("settings", "storage")}>Storage settings</button>
  </div>
{/if}

<style>
  .low {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
    border-radius: var(--r-sm);
    background: var(--warn-soft);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-control);
  }

  .head b {
    font-weight: 600;
  }

  .head :global(.icon) {
    color: var(--warn);
  }

  p {
    margin: 0;
    font-size: var(--fs-xs);
    line-height: 1.4;
    color: var(--muted);
  }

  button {
    padding: 6px var(--sp-3);
    font-size: var(--fs-control);
  }
</style>
