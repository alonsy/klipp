<script lang="ts">
  import * as api from "../api";
  import HotkeyInput from "../HotkeyInput.svelte";
  import HotkeyTest from "../HotkeyTest.svelte";
  import Icon from "../Icon.svelte";
  import { toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { deviceOptions, matchDevice, set, store } from "./store.svelte";

  const s = $derived(store.s!);
  const cue = $derived(store.s!.cue);
  const manual = $derived(s.replay.manual ?? true);
  let length = $state(store.s!.replay.length_secs as number);
  let volume = $state(Number(store.s!.cue.volume));
  /** The volume before muting, to come back to. */
  let unmuted = $state(Number(store.s!.cue.volume) || 0.6);
  /** The key test's result, in place of the hint. */
  let keyNote = $state("");

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
    hint={manual
      ? "Your shortcut saves the last moments from anywhere, so Klipp records all the time."
      : "No shortcut. Klipp records only while a game with auto-clips is running."}
  >
    <Segmented
      value={manual ? "on" : "off"}
      label="Manual clipping"
      options={[["on", "Always on"], ["off", "Off"]]}
      onchange={(v) => set(["replay", "manual"], v === "on")}
    />
  </Field>
  {#if manual}
    <Field title="Manual clip shortcut" hint={keyNote || "Works even in fullscreen games."}>
      <HotkeyInput value={s.replay.hotkey} onchange={(h) => set(["replay", "hotkey"], h)} />
      <HotkeyTest hotkey={s.replay.hotkey} bind:note={keyNote} cue={cue.device ?? ""} volume={volume} sound={cue.enabled ?? true} />
    </Field>
    <Field title="Manual clip length" hint="How much the shortcut saves.">
      <Slider min={10} max={300} step={5} value={length} label="Manual clip length" oninput={(v) => (length = v)} onchange={(v) => set(["replay", "length_secs"], v)} />
      <span class="value">{fmt(length)}</span>
    </Field>
  {/if}
  <Field title="Game auto-clips" hint="Klipp saves the best moments of supported games by itself. Pick the games in Auto-clips.">
    <Toggle checked={s.auto_clip.enabled} label="Game auto-clips" onchange={(v) => set(["auto_clip", "enabled"], v)} />
  </Field>
</div>

<span class="label group-title">Recording indicators</span>
<div class="group">
  <div class="sub"><Icon name="volume" size={15} />Sound</div>
  <Field title="Manual clip">
    <Toggle checked={cue.enabled} label="Sound for manual clips" onchange={(v) => set(["cue", "enabled"], v)} />
  </Field>
  <Field title="Auto-clip">
    <Toggle checked={cue.auto_clips} label="Sound for auto-clips" onchange={(v) => set(["cue", "auto_clips"], v)} />
  </Field>
  <Field title="Volume">
    <button
      class="icon mute"
      class:muted={volume === 0}
      aria-label={volume === 0 ? "Unmute" : "Mute"}
      title={volume === 0 ? "Unmute" : "Mute"}
      onclick={() => setVolume(volume === 0 ? unmuted : 0)}><Icon name={volume === 0 ? "mute" : "volume"} size={16} /></button
    >
    <Slider min={0} max={1} step={0.05} value={volume} label="Volume" oninput={(v) => (volume = v)} onchange={setVolume} />
    <span class="value">{Math.round(volume * 100)}%</span>
    <button class="test" onclick={test}>Test</button>
  </Field>
  <Field title="Play on" hint="Klipp keeps its sound out of your clips. With Sonar, Voicemeeter or another audio mixer, pick your headset.">
    <select class="device" value={matchDevice("loopback", cue.device)} onchange={(e) => set(["cue", "device"], e.currentTarget.value)} aria-label="Play on">
      <option value="">Windows default</option>
      {#each deviceOptions("loopback", cue.device) as name (name)}<option value={name}>{name}</option>{/each}
    </select>
  </Field>

  <div class="sub"><Icon name="monitor" size={15} />Visual</div>
  <Field title="Auto-clips active" hint="A small mark when a match starts in a game Klipp auto-clips. Never in your clips.">
    <button class="ghost preview" onclick={() => preview("on")}>Preview</button>
    <Toggle checked={s.indicators?.autoclips_on ?? false} label="Auto-clips active indicator" onchange={(v) => set(["indicators", "autoclips_on"], v)} />
  </Field>
  <Field title="Clip saved" hint="A small mark when a clip is saved.">
    <button class="ghost preview" onclick={() => preview("saved")}>Preview</button>
    <Toggle checked={s.indicators?.saves ?? false} label="Clip saved indicator" onchange={(v) => set(["indicators", "saves"], v)} />
  </Field>
</div>

<p class="credits">
  The save sounds are by
  <button class="handle" onclick={() => api.openFile("https://instagram.com/f.inancimen")}>@f.inancimen</button>. Thank you!
</p>

<style>
  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 14px 18px 4px;
    font-size: 11px;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }

  .sub + :global(.field) {
    border-top: none;
  }

  :global(.field) + .sub {
    margin-top: 6px;
    border-top: 1px solid var(--line);
  }

  .mute {
    color: var(--muted);
  }

  .mute.muted {
    color: var(--danger);
  }

  .test {
    padding: 5px 12px;
    font-size: 12.5px;
  }

  .preview {
    padding: 5px 10px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .device {
    max-width: 280px;
  }

  .credits {
    margin: 0 4px;
    font-size: 12px;
    color: var(--faint);
  }

  .handle {
    display: inline;
    padding: 0;
    border: none;
    background: none;
    font-size: inherit;
    color: var(--accent);
  }

  .handle:hover {
    background: none;
    text-decoration: underline;
  }
</style>
