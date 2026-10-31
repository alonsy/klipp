<script lang="ts">
  import * as api from "../api";
  import { routerInstalled } from "../discordTrack";
  import HotkeyInput from "../HotkeyInput.svelte";
  import HotkeyTest from "../HotkeyTest.svelte";
  import Icon from "../Icon.svelte";
  import { toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import Reveal from "./Reveal.svelte";
  import { deviceOptions, devicesOf, matchDevice, set, store } from "./store.svelte";

  const s = $derived(store.s!);
  const cue = $derived(store.s!.cue);
  const manual = $derived(s.replay.manual ?? true);
  let length = $state(store.s!.replay.length_secs as number);
  let volume = $state(Number(store.s!.cue.volume));
  /** The volume before muting, to come back to. */
  let unmuted = $state(Number(store.s!.cue.volume) || 0.6);
  /** The key test's result, in place of the hint. */
  let keyNote = $state("");

  /** With a mixer the sound can land in clips, so that advice stays in view. */
  const mixer = $derived(routerInstalled(devicesOf("loopback")));
  const PLAY_ON = "Klipp keeps its sound out of your clips. With Sonar, Voicemeeter or another audio mixer, pick your headset.";

  function fmt(secs: number) {
    return secs >= 60 ? `${Math.floor(secs / 60)} min${secs % 60 ? ` ${secs % 60} s` : ""}` : `${secs} s`;
  }

  function setVolume(v: number) {
    volume = v;
    if (v > 0) unmuted = v;
    set(["cue", "volume"], v);
  }

  /** As a real save would sound: on this device, at this volume. */
  function test() {
    api.playSound(cue.device, volume).catch((e) => toast(`Couldn't play it: ${e}`));
  }

  /** Shows the mark on screen now, as it will look over a game. */
  function preview(kind: "on" | "saved" | "failed") {
    api.previewIndicator(kind).catch((e) => toast(`Couldn't show it: ${e}`));
  }
</script>

<div class="group">
  <Field
    title="Manual clipping"
    tip={manual ? "Your shortcut saves the last moments from anywhere, so Klipp records all the time." : undefined}
    hint={manual ? undefined : "No shortcut. Klipp records only while a game with auto-clips is running."}
  >
    <Segmented
      value={manual ? "on" : "off"}
      label="Manual clipping"
      options={[["on", "Always on"], ["off", "Off"]]}
      onchange={(v) => set(["replay", "manual"], v === "on")}
    />
  </Field>
  <Reveal open={manual}>
    <Field title="Shortcut" tip="Works even in fullscreen games." hint={keyNote || undefined}>
      <HotkeyInput value={s.replay.hotkey} onchange={(h) => set(["replay", "hotkey"], h)} />
      <HotkeyTest hotkey={s.replay.hotkey} bind:note={keyNote} cue={cue.device ?? ""} volume={volume} sound={cue.enabled ?? true} />
    </Field>
    <Field title="Clip length" tip="How much the shortcut saves.">
      <Slider min={10} max={300} step={5} value={length} label="Manual clip length" oninput={(v) => (length = v)} onchange={(v) => set(["replay", "length_secs"], v)} />
      <span class="value">{fmt(length)}</span>
    </Field>
  </Reveal>
  <Field title="Game auto-clips" tip="Klipp saves the best moments of supported games by itself. Pick the games in Auto-clips.">
    <Toggle checked={s.auto_clip.enabled} label="Game auto-clips" onchange={(v) => set(["auto_clip", "enabled"], v)} />
  </Field>
</div>

<div class="group">
  <h3 class="group-title">Sound</h3>
  <Field title="Manual clips">
    <Toggle checked={cue.enabled} label="Sound for manual clips" onchange={(v) => set(["cue", "enabled"], v)} />
  </Field>
  <Field title="Auto-clips">
    <Toggle checked={cue.auto_clips} label="Sound for auto-clips" onchange={(v) => set(["cue", "auto_clips"], v)} />
  </Field>
  <Field title="Volume">
    <button
      class="ghost icon mute"
      class:muted={volume === 0}
      aria-label={volume === 0 ? "Unmute" : "Mute"}
      title={volume === 0 ? "Unmute" : "Mute"}
      onclick={() => setVolume(volume === 0 ? unmuted : 0)}><Icon name={volume === 0 ? "mute" : "volume"} size={16} /></button
    >
    <Slider min={0} max={1} step={0.05} value={volume} label="Volume" oninput={(v) => (volume = v)} onchange={setVolume} />
    <span class="value">{Math.round(volume * 100)}%</span>
    <button onclick={test}>Test</button>
  </Field>
  <Field title="Play on" tip={mixer ? undefined : PLAY_ON} hint={mixer ? PLAY_ON : undefined}>
    <select value={matchDevice("loopback", cue.device)} onchange={(e) => set(["cue", "device"], e.currentTarget.value)} aria-label="Play on">
      <option value="">Windows default</option>
      {#each deviceOptions("loopback", cue.device) as name (name)}<option value={name}>{name}</option>{/each}
    </select>
  </Field>
</div>

<div class="group">
  <h3 class="group-title">On screen</h3>
  <Field title="Auto-clips active" tip="A small mark when a match starts in a game Klipp auto-clips. Never in your clips.">
    <button class="ghost preview" onclick={() => preview("on")}>Preview</button>
    <Toggle checked={s.indicators?.autoclips_on ?? false} label="Auto-clips active indicator" onchange={(v) => set(["indicators", "autoclips_on"], v)} />
  </Field>
  <Field title="Clip saved" tip="A small mark when a clip is saved.">
    <button class="ghost preview" onclick={() => preview("saved")}>Preview</button>
    <Toggle checked={s.indicators?.saves ?? false} label="Clip saved indicator" onchange={(v) => set(["indicators", "saves"], v)} />
  </Field>
</div>

<p class="note">
  The save sounds are by
  <button class="handle" onclick={() => api.openFile("https://instagram.com/f.inancimen")}>@f.inancimen</button>. Thank you!
</p>

<style>
  .mute {
    color: var(--muted);
  }

  .mute.muted {
    color: var(--danger);
  }

  .preview {
    padding: 5px 10px;
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .handle {
    display: inline;
    padding: 0;
    background: none;
    font-size: inherit;
    color: var(--muted);
  }

  .handle:hover {
    background: none;
    color: var(--text);
    text-decoration: underline;
  }
</style>
