<script lang="ts">
  import Icon from "../Icon.svelte";
  import Field from "../ui/Field.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { deviceOptions, matchDevice, set, store } from "./store.svelte";

  const cue = $derived(store.s!.cue);
  let volume = $state(Number(store.s!.cue.volume));
</script>

<div class="group">
  <Field title="Play a sound when a clip is saved" hint="So you know it worked without leaving the game.">
    <Toggle checked={cue.enabled} label="Play a sound when a clip is saved" onchange={(v) => set(["cue", "enabled"], v)} />
  </Field>
  <Field title="Also for auto-clips">
    <Toggle checked={cue.auto_clips} label="Also for auto-clips" onchange={(v) => set(["cue", "auto_clips"], v)} />
  </Field>
  <Field title="Volume">
    <Slider min={0} max={1} step={0.05} value={volume} label="Volume" oninput={(v) => (volume = v)} onchange={(v) => set(["cue", "volume"], v)} />
    <span class="value">{Math.round(volume * 100)}%</span>
  </Field>
  <Field title="Play on" hint="Pick a device you don't record, like your headset, or the sound ends up in your next clip.">
    <select class="device" value={matchDevice("loopback", cue.device)} onchange={(e) => set(["cue", "device"], e.currentTarget.value)} aria-label="Play on">
      <option value="">Windows default</option>
      {#each deviceOptions("loopback", cue.device) as name (name)}<option value={name}>{name}</option>{/each}
    </select>
  </Field>
</div>

<span class="label group-title">Your own sounds</span>
<div class="group">
  <Field title="Saved" hint="A short WAV file (under 5 seconds). Empty uses Klipp's sound.">
    <input class="path" placeholder="Built-in" value={cue.sound} onchange={(e) => set(["cue", "sound"], e.currentTarget.value.trim())} />
  </Field>
  <Field title="Auto-clip saved" hint="Empty uses the same sound as above.">
    <input class="path" placeholder="Same as saved" value={cue.auto_sound} onchange={(e) => set(["cue", "auto_sound"], e.currentTarget.value.trim())} />
  </Field>
</div>
<p class="note"><Icon name="info" size={14} />Paste a full path, e.g. D:\sounds\saved.wav.</p>

<style>
  .device {
    max-width: 300px;
  }

  .path {
    width: 260px;
  }

  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 4px;
    font-size: 12.5px;
    color: var(--muted);
  }
</style>
