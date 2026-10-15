<script lang="ts">
  import * as api from "./api";
  import { clock } from "./format";
  import { markSvg } from "./icons";
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

<div class="scrim" role="presentation" onclick={() => (ui.closeNotice = false)}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="close-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <div class="tray">
      <div class="taskbar">
        <span class="sys"></span><span class="sys"></span>
        <span class="mark">{@html markSvg(22)}</span>
        <span class="sys"></span>
        <span class="time mono">{clock(Date.now())}</span>
      </div>
    </div>
    <h2 id="close-title">Klipp keeps recording</h2>
    <p>
      Closing this window doesn't stop Klipp. It stays in the tray and keeps the last {recorder.lengthSecs} seconds ready, so
      <kbd>{recorder.hotkey}</kbd> and auto-clips still work.
    </p>
    <p class="muted">To stop recording, right-click the Klipp icon in the tray and choose <b>Quit Klipp</b>.</p>
    <div class="actions">
      <label class="check"><input type="checkbox" bind:checked={dontShow} /> Don't show this again</label>
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
    animation: fade 0.18s var(--ease);
  }

  .dialog {
    width: min(460px, calc(100vw - 32px));
    padding: 22px 24px 20px;
    border-radius: 18px;
    background: var(--surface);
    border: 1px solid var(--line);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
    animation: rise 0.22s var(--ease);
  }

  .tray {
    display: flex;
    justify-content: flex-end;
    padding: 20px 16px 0;
    margin-bottom: 18px;
    border-radius: 12px;
    background: linear-gradient(160deg, var(--surface-3), var(--stage));
    overflow: hidden;
  }

  .taskbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 10px 10px 0 0;
    background: rgba(0, 0, 0, 0.45);
  }

  .sys {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    background: var(--surface-3);
  }

  .mark {
    display: inline-flex;
    padding: 3px;
    border-radius: 6px;
    background: var(--accent-soft);
    outline: 1.5px solid var(--accent);
    color: var(--text);
  }

  .time {
    font-size: 11px;
    color: var(--muted);
  }

  h2 {
    margin: 0 0 8px;
    font-family: var(--font-display);
    font-size: 18px;
    letter-spacing: -0.02em;
  }

  p {
    margin: 0 0 10px;
    line-height: 1.5;
    color: var(--text);
  }

  .muted {
    color: var(--muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 18px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-right: auto;
    color: var(--muted);
    cursor: pointer;
  }

  input[type="checkbox"] {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
    padding: 0;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.98);
    }
  }
</style>
