<script lang="ts">
  // Minimize, maximize and close, drawn by the app so the window is one
  // surface. Close asks first if recording carries on (see CloseNotice).
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  const win = getCurrentWindow();
  let maximized = $state(false);

  onMount(() => {
    const check = () => win.isMaximized().then((m) => (maximized = m));
    check();
    const off = win.onResized(check);
    return () => {
      off.then((f) => f());
    };
  });
</script>

<div class="controls">
  <button class="ctl" aria-label="Minimize" title="Minimize" onclick={() => win.minimize()}>
    <svg viewBox="0 0 10 10" width="10" height="10"><path d="M0 5.5h10" /></svg>
  </button>
  <button class="ctl" aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"} onclick={() => win.toggleMaximize()}>
    {#if maximized}
      <svg viewBox="0 0 10 10" width="10" height="10"><path d="M2.5 2.5V1.2c0-.4.3-.7.7-.7h5.6c.4 0 .7.3.7.7v5.6c0 .4-.3.7-.7.7H7.5" /><rect x=".5" y="2.5" width="7" height="7" rx=".8" /></svg>
    {:else}
      <svg viewBox="0 0 10 10" width="10" height="10"><rect x=".5" y=".5" width="9" height="9" rx="1" /></svg>
    {/if}
  </button>
  <button class="ctl close" aria-label="Close" title="Close" onclick={() => win.close()}>
    <svg viewBox="0 0 10 10" width="10" height="10"><path d="M.7.7l8.6 8.6M9.3.7L.7 9.3" /></svg>
  </button>
</div>

<style>
  .controls {
    display: flex;
    flex-shrink: 0;
    height: 36px;
  }

  .ctl {
    width: 46px;
    height: 36px;
    padding: 0;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--muted);
  }

  .ctl:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .ctl:active {
    transform: none;
  }

  .close:hover {
    background: #e5484d;
    color: #fff;
  }

  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
    shape-rendering: geometricPrecision;
  }
</style>
