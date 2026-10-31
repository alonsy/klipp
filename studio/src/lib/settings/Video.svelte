<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import type { MonitorInfo } from "../api";
  import type { PcTest } from "../pctest";
  import { matchPreset, PRESETS, presetValues, type PresetId, type VideoChoice } from "../presets";
  import { toast } from "../state.svelte";
  import SystemMeters from "../SystemMeters.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Reveal from "./Reveal.svelte";
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

  // Custom shows the settings themselves, and stays open while they're changed,
  // even when they happen to match a preset on the way.
  let custom = $state(false);
  $effect(() => {
    if (preset === "custom") custom = true;
  });
  // Without a screen list no preset can match, but Custom still has to open.
  const picked = $derived(custom ? "custom" : preset);
  const picks = $derived([
    ...PRESETS.map((p) => {
      const v = presetValues(p.id, screen);
      return { id: p.id as PresetId | "custom", name: p.name, spec: `${v.resolution === "native" ? `${screen.height}p` : v.resolution} · ${v.fps} fps`, hint: p.hint };
    }),
    { id: "custom" as const, name: "Custom", spec: "Your own", hint: "Your own mix of the settings." },
  ]);
  const pickedIndex = $derived(picks.findIndex((p) => p.id === picked));

  function usePreset(id: PresetId) {
    const values = presetValues(id, screen);
    quality = values.quality;
    setMany(Object.entries(values).map(([k, v]) => ({ key: ["video", k], value: v })));
    custom = false;
  }

  function pick(id: PresetId | "custom") {
    if (id === picked) return;
    if (id === "custom") custom = true;
    else usePreset(id);
  }

  /** Left and right move the choice, as in a native radio group. */
  function onkeydown(e: KeyboardEvent) {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    e.preventDefault();
    const next = (Math.max(pickedIndex, 0) + step + picks.length) % picks.length;
    pick(picks[next].id);
    (e.currentTarget as HTMLElement).querySelectorAll("button")[next]?.focus();
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

<div class="group">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="presets" role="radiogroup" aria-label="Preset" tabindex="-1" style:--n={picks.length} style:--i={pickedIndex} {onkeydown}>
    {#if pickedIndex >= 0}<span class="thumb" aria-hidden="true"></span>{/if}
    {#each picks as p (p.id)}
      <button class="preset" class:on={picked === p.id} role="radio" aria-checked={picked === p.id} title={p.hint} onclick={() => pick(p.id)}>
        <span class="pname">{p.name}</span>
        <span class="pspec mono">{p.spec}</span>
      </button>
    {/each}
  </div>

  <Reveal open={picked === "custom"}>
    <div class="custom">
      <Field title="Resolution" tip="Native records your screen as it is.">
        <Segmented
          value={s.video.resolution}
          label="Resolution"
          options={resolutions.map((r) => [r, r === "native" ? "Native" : r] as const)}
          onchange={(v) => set(["video", "resolution"], v)}
        />
      </Field>
      <Field title="Frame rate" hint={s.video.fps > screen.refreshHz ? `Your screen runs at ${screen.refreshHz} Hz.` : undefined}>
        <Segmented value={s.video.fps} label="Frame rate" options={[[30, "30 fps"], [60, "60 fps"], [120, "120 fps"]]} onchange={(v) => set(["video", "fps"], v)} />
      </Field>
      <Field title="Quality" tip="Higher quality makes bigger files and uses more memory.">
        <Slider min={18} max={40} step={1} value={58 - quality} label="Quality" oninput={(v) => (quality = 58 - v)} onchange={(v) => set(["video", "quality"], 58 - v)} />
        <span class="value quality">{qualityName(quality)}</span>
      </Field>
      <Field title="Bitrate limit" tip="The most a busy scene may take. Lower keeps memory and files smaller.">
        <select value={s.video.max_bitrate_mbps} onchange={(e) => set(["video", "max_bitrate_mbps"], Number(e.currentTarget.value))} aria-label="Bitrate limit">
          {#each BITRATES.includes(s.video.max_bitrate_mbps) ? BITRATES : [...BITRATES, s.video.max_bitrate_mbps].sort((a, b) => a - b) as m (m)}
            <option value={m}>{m} Mbit/s</option>
          {/each}
        </select>
      </Field>
      <Field title="When a game needs the graphics card" tip={performanceHints[choice.performance]}>
        <Segmented
          value={choice.performance}
          label="When a game needs the graphics card"
          options={[["off", "Stay as set"], ["on", "Step down"], ["plus", "Lightest"]]}
          onchange={(v) => set(["video", "performance"], v)}
        />
      </Field>
      <Reveal open={choice.performance !== "plus"}>
        <Field title="Capture method" tip="Light does the least work on the graphics card, for weaker ones. Automatic picks what works best on your Windows.">
          <select value={choice.capture} onchange={(e) => set(["video", "capture"], e.currentTarget.value)} aria-label="Capture method">
            <option value="auto">Automatic</option>
            <option value="light">Light</option>
            <option value="wgc">Windows Graphics Capture</option>
            <option value="duplication">Desktop Duplication</option>
          </select>
        </Field>
        <Field title="Encoder speed" tip="Fastest asks the least of the graphics card's encoder, at a little more size for the same quality.">
          <Segmented
            value={choice.fast_encoder ? "fast" : "quality"}
            label="Encoder speed"
            options={[["quality", "Quality"], ["fast", "Fastest"]]}
            onchange={(v) => set(["video", "fast_encoder"], v === "fast")}
          />
        </Field>
      </Reveal>
    </div>
  </Reveal>

  {#if screens.length > 1 || missing}
    <Field title="Screen" hint={missing ? "The screen chosen before isn't connected, so Klipp records the main display until it is." : undefined}>
      <select value={s.video.monitor} onchange={(e) => set(["video", "monitor"], e.currentTarget.value)} aria-label="Screen">
        <option value="">Main display{screens[0]?.primary ? ` (${screens[0].name})` : ""}</option>
        {#each screens as m (m.id)}<option value={m.id}>{label(m)}</option>{/each}
        {#if missing}<option value={s.video.monitor}>Not connected</option>{/if}
      </select>
    </Field>
  {/if}
  <Field title="Codec" tip="AV1 makes files about half the size. H.264 plays on anything, even old phones.">
    <Segmented value={s.video.codec} label="Codec" options={[["av1", "AV1"], ["h264", "H.264"]]} onchange={(v) => set(["video", "codec"], v)} />
  </Field>
</div>

<p class="note">Changing these restarts recording for about a second, and what was buffered so far is dropped.</p>

<style>
  .presets {
    --pad: 3px;
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    margin-bottom: var(--sp-2);
    padding: var(--pad);
    border-radius: var(--r-sm);
    background: var(--sunken);
  }

  .thumb {
    position: absolute;
    top: var(--pad);
    bottom: var(--pad);
    left: var(--pad);
    width: calc((100% - 2 * var(--pad)) / var(--n));
    border-radius: 7px;
    background: var(--surface-3);
    box-shadow: var(--shadow-1);
    transform: translateX(calc(var(--i) * 100%));
    transition: transform var(--dur-spring) var(--spring);
  }

  .preset {
    position: relative;
    flex-direction: column;
    gap: 1px;
    padding: 7px var(--sp-1);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
  }

  .preset:hover {
    background: transparent;
    color: var(--text);
  }

  .preset:active {
    transform: none;
  }

  .preset.on {
    color: var(--text);
  }

  .pname {
    font-size: var(--fs-control);
    font-weight: 550;
  }

  .pspec {
    color: var(--faint);
    font-size: var(--fs-xs);
    white-space: nowrap;
    transition: color var(--dur-fast);
  }

  .on .pspec {
    color: var(--muted);
  }

  .quality {
    min-width: 72px;
  }
</style>
