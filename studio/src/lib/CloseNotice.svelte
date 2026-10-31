<script lang="ts">
  import * as api from "./api";
  import { clock } from "./format";
  import { markSvg } from "./icons";
  import { DUR, enter, leave, pop } from "./motion";
  import { recorder, toast, ui } from "./state.svelte";

  let dontShow = $state(false);

  async function close() {
    try {
      await api.closeWindow(dontShow);
    } catch (e) {
      toast(`Couldn't close: ${e}`);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") ui.closeNotice = false;
    if (e.key === "Enter") close();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="scrim" role="presentation" onclick={() => (ui.closeNotice = false)} in:enter={{ y: 0, duration: DUR.base }} out:leave>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="close-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}} in:pop={{ duration: DUR.slow }} out:pop={{ duration: DUR.fast }}>
    <div class="tray" aria-hidden="true">
      <div class="taskbar">
        <span class="sys"></span><span class="sys"></span>
        <span class="mark">{@html markSvg(20)}</span>
        <span class="sys"></span>
        <span class="time mono">{clock(Date.now())}</span>
      </div>
    </div>
    <h2 id="close-title">Klipp keeps recording</h2>
    <p>Closing this window doesn't stop it. From the tray, <kbd>{recorder.hotkey}</kbd> and auto-clips still work.</p>
    <p class="muted">To stop recording, right-click the tray icon and choose <b>Quit Klipp</b>.</p>
    <div class="actions">
      <label class="check"><input type="checkbox" bind:checked={dontShow} /> Don't show again</label>
      <button class="ghost" onclick={() => (ui.closeNotice = false)}>Cancel</button>
      <button class="primary" onclick={close}>Close window</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    background: var(--scrim);
    backdrop-filter: blur(6px);
  }

  .dialog {
    width: min(440px, calc(100vw - 32px));
    padding: var(--sp-3) var(--sp-3) var(--sp-5);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--shadow-3);
  }

  /* The corner of the screen where Klipp goes. */
  .tray {
    display: flex;
    justify-content: flex-end;
    padding: var(--sp-5) var(--sp-5) 0;
    margin-bottom: var(--sp-5);
    border-radius: var(--r-md);
    background: var(--sunken);
    overflow: hidden;
  }

  .taskbar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-sm) var(--r-sm) 0 0;
    background: var(--surface-2);
  }

  .sys {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    background: var(--surface-4);
  }

  .mark {
    position: relative;
    display: inline-flex;
    padding: var(--sp-1);
    border-radius: var(--r-xs);
    background: var(--accent-soft);
    color: var(--text);
  }

  /* A soft ring goes out from the icon, to say "here". */
  .mark::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: 0 0 0 1.5px var(--accent);
    animation: here 2s var(--ease-out) 300ms infinite;
  }

  .time {
    font-size: var(--fs-xs);
    color: var(--muted);
  }

  h2,
  p,
  .actions {
    margin-inline: var(--sp-3);
  }

  h2 {
    margin-block: 0 var(--sp-2);
    font-family: var(--font-display);
    font-size: var(--fs-lg);
    letter-spacing: -0.02em;
  }

  p {
    margin-block: 0 var(--sp-2);
    line-height: 1.5;
    color: var(--text);
  }

  .muted {
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .muted b {
    color: var(--text);
    font-weight: 600;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-top: var(--sp-5);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-right: auto;
    color: var(--muted);
    font-size: var(--fs-control);
    cursor: pointer;
  }

  @keyframes here {
    60%,
    100% {
      opacity: 0;
      transform: scale(1.5);
    }
  }
</style>
