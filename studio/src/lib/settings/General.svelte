<script lang="ts">
  import * as api from "../api";
  import { openPage, prefs, recorder, refreshRecorder, setPref, toast, ui } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const accents = [
    ["#ff5a2e", "Ember"],
    ["#c6f432", "Volt"],
    ["#5b8cff", "Signal"],
    ["#f4f2ec", "Bone"],
  ] as const;

  const chosen = $derived(accents.findIndex(([color]) => color === prefs.accent));

  async function autostart(on: boolean) {
    try {
      await api.setAutostart(on);
      recorder.autostart = on;
    } catch (e) {
      toast(`Couldn't change that: ${e}`);
      refreshRecorder();
    }
  }
</script>

<div class="group">
  <Field title="Start with Windows" tip="Klipp starts in the tray when you sign in, ready to record.">
    <Toggle checked={recorder.autostart} label="Start with Windows" onchange={autostart} />
  </Field>
  <Field title="Explain closing the window" tip="Closing this window keeps Klipp in the tray. Shows a reminder when you close it.">
    <Toggle checked={prefs.closeNotice} label="Explain closing the window" onchange={(v) => setPref("closeNotice", v)} />
  </Field>
  <Field title="Group clips by session" tip="Clips from one sitting (and one map) are shown together.">
    <Toggle checked={prefs.groupSessions} label="Group clips by session" onchange={(v) => setPref("groupSessions", v)} />
  </Field>
  <Field title="Accent color">
    <div class="swatches" role="radiogroup" aria-label="Accent color" style:--i={chosen}>
      {#if chosen >= 0}<span class="ring" aria-hidden="true"></span>{/if}
      {#each accents as [color, name] (color)}
        <button
          class="swatch"
          style:--c={color}
          role="radio"
          aria-checked={prefs.accent === color}
          title={name}
          aria-label={name}
          onclick={() => setPref("accent", color)}
        ></button>
      {/each}
    </div>
  </Field>
</div>

<div class="group">
  <Field title="Setup diagnostics" tip="Tests your PC again, shows what Klipp uses on it, and walks through audio, video and saving.">
    <button
      onclick={() => {
        ui.setupRerun = true;
        openPage("setup");
      }}>Run again</button
    >
  </Field>
</div>

<style>
  .swatches {
    --size: 24px;
    --gap: 12px;
    position: relative;
    display: flex;
    gap: var(--gap);
    padding: var(--sp-1);
  }

  .swatch {
    position: relative;
    width: var(--size);
    height: var(--size);
    padding: 0;
    border-radius: 50%;
    background: var(--c);
  }

  .swatch:hover {
    background: var(--c);
    transform: scale(1.08);
  }

  /* One ring that travels to the chosen colour. */
  .ring {
    position: absolute;
    top: 0;
    left: 0;
    width: calc(var(--size) + 8px);
    height: calc(var(--size) + 8px);
    border-radius: 50%;
    box-shadow: inset 0 0 0 2px var(--line-strong);
    transform: translateX(calc(var(--i) * (var(--size) + var(--gap))));
    transition: transform var(--dur-spring) var(--spring);
  }
</style>
