<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import type { MonitorInfo } from "../api";
  import Icon from "../Icon.svelte";
  import type { PcTest } from "../pctest";
  import { matchPreset, PRESETS, presetValues, type PresetId, type VideoChoice } from "../presets";
  import { toast } from "../state.svelte";
  import SystemMeters from "../SystemMeters.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import { set, setMany, store } from "./store.svelte";

  const s = $derived(store.s!);
  let quality = $state(store.s!.video.quality as number);

  let screens = $state<MonitorInfo[]>([]);
  /** The last PC test, and one running now. */
  let test = $state<PcTest | null>(null);
  let testing = $state(false);
  onMount(() => {
    api.monitors().then((m) => (screens = m), () => {});
    api.pcTest().then((t) => (test = t), () => {});
  });

  async function runTest() {
    testing = true;
    try {
      test = await api.runPcTest();
    } catch (e) {
      toast(`The PC test didn't finish: ${e}`);
    } finally {
      testing = false;
    }
  }
  /** The mix, and each source on its own track. */
  const tracks = $derived(1 + (s.audio?.sources?.length ?? 2));
  const label = (m: MonitorInfo) => `${m.name} · ${m.width} × ${m.height}`;
  /** The saved monitor, if it isn't connected now (it's still recorded once it is). */
  const missing = $derived(!!s.video.monitor && screens.length > 0 && !screens.some((m) => m.id === s.video.monitor));
  /** The screen Klipp records: what the presets are made for. */
  const screen = $derived.by(() => {
    const m = screens.find((m) => m.id === s.video.monitor) ?? screens.find((m) => m.primary) ?? screens[0];
    return { height: m?.height ?? test?.monitor?.height ?? 1080, refreshHz: m?.refreshHz ?? test?.monitor?.refreshHz ?? 60 };
  });

  const choice = $derived<VideoChoice>({
    codec: s.video.codec,
    resolution: s.video.resolution,
    fps: s.video.fps,
    // The slider's value while dragging, so the drawings move with it.
    quality,
    max_bitrate_mbps: Number(s.video.max_bitrate_mbps),
    capture: s.video.capture ?? "auto",
    performance: s.video.performance ?? "off",
    fast_encoder: s.video.fast_encoder ?? false,
  });
  const preset = $derived(screens.length ? matchPreset(choice, screen) : null);

  function usePreset(id: PresetId) {
    const values = presetValues(id, screen);
    quality = values.quality;
    setMany(Object.entries(values).map(([k, v]) => ({ key: ["video", k], value: v })));
  }

  function qualityName(q: number) {
    return q <= 24 ? "Very high" : q <= 30 ? "High" : q <= 35 ? "Medium" : "Low";
  }

  const resolutions = $derived(
    (["native", "1440p", "1080p", "720p"] as const).filter((r) => r === "native" || Number(r.replace("p", "")) < screen.height),
  );
  const BITRATES = [10, 15, 20, 30, 40, 50, 60, 80, 100, 150];

  const performanceHints = {
    off: "Klipp always records as set above.",
    on: "While a game keeps your graphics card busy, Klipp lowers its bitrate, then its frame rate, and goes back up when there's room.",
    plus: "As Step down, recorded the lightest way your PC allows: the light capture and the fastest encoder.",
  };
</script>

<SystemMeters {test} {choice} lengthSecs={Number(s.replay?.length_secs ?? 60)} {screen} audioTracks={tracks} onrun={runTest} running={testing} />

<span class="label group-title">Preset</span>
<div class="presets" role="radiogroup" aria-label="Preset">
  {#each PRESETS as p (p.id)}
    {@const v = presetValues(p.id, screen)}
    <button class="preset" class:on={preset === p.id} role="radio" aria-checked={preset === p.id} onclick={() => preset !== p.id && usePreset(p.id)}>
      <span class="pname">{p.name}</span>
      <span class="pspec mono">{v.resolution === "native" ? `${screen.height}p` : v.resolution} · {v.fps} fps</span>
    </button>
  {/each}
  <div class="preset custom" class:on={preset === "custom"} role="radio" aria-checked={preset === "custom"} aria-disabled="true">
    <span class="pname">Custom</span>
    <span class="pspec">Your own settings</span>
  </div>
</div>
{#if preset}
  <p class="phint">
    {preset === "custom" ? "Your own mix of the settings below. Pick a preset to go back to one." : PRESETS.find((p) => p.id === preset)?.hint}
  </p>
{/if}

<span class="label group-title">Settings</span>
<div class="group">
  {#if screens.length > 1 || missing}
    <Field title="Screen" hint={missing ? "The screen chosen before isn't connected, so Klipp records the main display until it is." : "Which screen Klipp records."}>
      <select class="screen" value={s.video.monitor} onchange={(e) => set(["video", "monitor"], e.currentTarget.value)} aria-label="Screen">
        <option value="">Main display{screens[0]?.primary ? ` (${screens[0].name})` : ""}</option>
        {#each screens as m (m.id)}<option value={m.id}>{label(m)}</option>{/each}
        {#if missing}<option value={s.video.monitor}>Not connected</option>{/if}
      </select>
    </Field>
  {/if}
  <Field title="Codec" hint="AV1 makes files about half the size. H.264 plays on anything, even old phones.">
    <Segmented value={s.video.codec} label="Codec" options={[["av1", "AV1"], ["h264", "H.264"]]} onchange={(v) => set(["video", "codec"], v)} />
  </Field>
  <Field title="Resolution" hint="Native records your screen as it is.">
    <Segmented
      value={s.video.resolution}
      label="Resolution"
      options={resolutions.map((r) => [r, r === "native" ? "Native" : r] as const)}
      onchange={(v) => set(["video", "resolution"], v)}
    />
  </Field>
  <Field title="Frame rate" hint={screen.refreshHz < 120 ? `Your screen runs at ${screen.refreshHz} Hz.` : undefined}>
    <Segmented value={s.video.fps} label="Frame rate" options={[[30, "30"], [60, "60"], [120, "120"]]} onchange={(v) => set(["video", "fps"], v)} />
    <span class="unit label">fps</span>
  </Field>
  <Field title="Quality" hint="Higher quality makes bigger files and uses more memory.">
    <Slider min={18} max={40} step={1} value={58 - quality} label="Quality" oninput={(v) => (quality = 58 - v)} onchange={(v) => set(["video", "quality"], 58 - v)} />
    <span class="value">{qualityName(quality)}</span>
  </Field>
  <Field title="Bitrate limit" hint="The most a busy scene may take. Lower keeps memory and files smaller.">
    <select value={s.video.max_bitrate_mbps} onchange={(e) => set(["video", "max_bitrate_mbps"], Number(e.currentTarget.value))} aria-label="Bitrate limit">
      {#each BITRATES.includes(s.video.max_bitrate_mbps) ? BITRATES : [...BITRATES, s.video.max_bitrate_mbps].sort((a, b) => a - b) as m (m)}
        <option value={m}>{m} Mbit/s</option>
      {/each}
    </select>
  </Field>
</div>

<span class="label group-title">Performance</span>
<div class="group">
  <Field title="When a game needs the graphics card" hint={performanceHints[choice.performance]}>
    <Segmented
      value={choice.performance}
      label="When a game needs the graphics card"
      options={[["off", "Stay as set"], ["on", "Step down"], ["plus", "Lightest"]]}
      onchange={(v) => set(["video", "performance"], v)}
    />
  </Field>
  {#if choice.performance !== "plus"}
    <Field title="Capture method" hint="Light does the least work on the graphics card, for weaker ones. Automatic picks what works best on your Windows.">
      <select value={choice.capture} onchange={(e) => set(["video", "capture"], e.currentTarget.value)} aria-label="Capture method">
        <option value="auto">Automatic</option>
        <option value="light">Light</option>
        <option value="wgc">Windows Graphics Capture</option>
        <option value="duplication">Desktop Duplication</option>
      </select>
    </Field>
    <Field title="Encoder speed" hint="Fastest asks the least of the graphics card's encoder, at a little more size for the same quality.">
      <Segmented
        value={choice.fast_encoder ? "fast" : "quality"}
        label="Encoder speed"
        options={[["quality", "Quality"], ["fast", "Fastest"]]}
        onchange={(v) => set(["video", "fast_encoder"], v === "fast")}
      />
    </Field>
  {/if}
</div>

<p class="restart"><Icon name="info" size={14} />Changing these restarts recording for about a second, and what was buffered so far is dropped.</p>

<style>
  .presets {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 8px;
  }

  .preset {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 12px 12px 11px;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--surface);
    text-align: left;
    transition:
      border-color 0.15s,
      background 0.15s;
  }

  .preset:hover {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
    background: var(--surface);
  }

  .preset.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .preset.custom {
    cursor: default;
    opacity: 0.6;
  }

  .preset.custom.on {
    opacity: 1;
  }

  .pname {
    font-weight: 650;
    font-size: 13.5px;
    color: var(--text);
  }

  .pspec {
    font-size: 11px;
    color: var(--muted);
  }

  .phint {
    margin: -10px 4px 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--muted);
  }

  .unit {
    font-size: 10px;
  }

  .screen {
    max-width: 300px;
  }

  .restart {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 4px;
    font-size: 12px;
    color: var(--faint);
  }
</style>
