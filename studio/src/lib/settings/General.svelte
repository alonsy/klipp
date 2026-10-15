<script lang="ts">
  import * as api from "../api";
  import { prefs, recorder, refreshRecorder, setPref, toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const accents = [
    ["#ff5a2e", "Ember"],
    ["#c6f432", "Volt"],
    ["#5b8cff", "Signal"],
    ["#f4f2ec", "Bone"],
  ] as const;

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
  <Field title="Start with Windows" hint="Recording starts in the tray when you sign in. The Klipp window opens only when you open it.">
    <Toggle checked={recorder.autostart} label="Start with Windows" onchange={autostart} />
  </Field>
  <Field title="Explain closing the window" hint="Closing this window keeps Klipp recording in the tray. Shows a reminder when you close it.">
    <Toggle checked={prefs.closeNotice} label="Explain closing the window" onchange={(v) => setPref("closeNotice", v)} />
  </Field>
</div>

<span class="label group-title">Look</span>
<div class="group">
  <Field title="Accent color" hint="Used for highlights, multi-kills and the record dot.">
    <div class="swatches" role="radiogroup" aria-label="Accent color">
      {#each accents as [color, name] (color)}
        <button
          class="swatch"
          class:on={prefs.accent === color}
          style="--c: {color}"
          role="radio"
          aria-checked={prefs.accent === color}
          title={name}
          aria-label={name}
          onclick={() => setPref("accent", color)}
        ></button>
      {/each}
    </div>
  </Field>
  <Field title="Group clips by session" hint="Clips from one sitting (and one map) are shown together, newest first.">
    <Toggle checked={prefs.groupSessions} label="Group clips by session" onchange={(v) => setPref("groupSessions", v)} />
  </Field>
</div>

<style>
  .swatches {
    display: flex;
    gap: 10px;
  }

  .swatch {
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid transparent;
    background: var(--c);
    box-shadow: inset 0 0 0 3px var(--surface);
  }

  .swatch:hover {
    background: var(--c);
    transform: scale(1.08);
  }

  .swatch.on {
    border-color: var(--c);
  }
</style>
