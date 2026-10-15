<script lang="ts">
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import { toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { deviceOptions, matchDevice, set, store } from "./store.svelte";

  const cue = $derived(store.s!.cue);
  let volume = $state(Number(store.s!.cue.volume));

  /** As a real save would sound: on this device, at this volume. */
  function play(error = false) {
    api.playSound(cue.device, volume, error).catch((e) => toast(`Couldn't play it: ${e}`));
  }
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
    <button class="icon" aria-label="Play the save sound" title="Play the save sound" onclick={() => play()}><Icon name="play" size={15} /></button>
    <button class="icon" aria-label="Play the error sound" title="Play the error sound (when a save can't happen)" onclick={() => play(true)}
      ><Icon name="warning" size={15} /></button
    >
  </Field>
  <Field title="Play on" hint="Pick a device you don't record, like your headset, or the sound ends up in your next clip.">
    <select class="device" value={matchDevice("loopback", cue.device)} onchange={(e) => set(["cue", "device"], e.currentTarget.value)} aria-label="Play on">
      <option value="">Windows default</option>
      {#each deviceOptions("loopback", cue.device) as name (name)}<option value={name}>{name}</option>{/each}
    </select>
  </Field>
</div>

<style>
  .device {
    max-width: 300px;
  }
</style>
