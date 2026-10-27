<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import type { MonitorInfo } from "../api";
  import Icon from "../Icon.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);
  let quality = $state(store.s!.video.quality as number);

  let screens = $state<MonitorInfo[]>([]);
  onMount(() => {
    api.monitors().then((m) => (screens = m), () => {});
  });
  const label = (m: MonitorInfo) => `${m.name} · ${m.width} × ${m.height}`;
  /** The saved monitor, if it isn't connected now (it's still recorded once it is). */
  const missing = $derived(!!s.video.monitor && screens.length > 0 && !screens.some((m) => m.id === s.video.monitor));

  function qualityName(q: number) {
    return q <= 24 ? "Very high" : q <= 30 ? "High" : q <= 35 ? "Medium" : "Low";
  }

  const performance = $derived((s.video.performance ?? "off") as "off" | "on" | "plus");
  const performanceHints = {
    off: "Klipp always records as set above.",
    on: "While a game keeps your graphics card busy, Klipp records at a lower bitrate, then a lower frame rate, and goes back up when the card has room.",
    plus: "As Performance, and Klipp records the lightest way your PC allows: the light capture method and the encoder's fastest setting.",
  };
</script>

<div class="banner"><Icon name="info" size={17} />Changing these restarts recording for about a second, and what was buffered so far is dropped.</div>

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
      options={[["native", "Native"], ["1440p", "1440p"], ["1080p", "1080p"], ["720p", "720p"]]}
      onchange={(v) => set(["video", "resolution"], v)}
    />
  </Field>
  <Field title="Frame rate">
    <Segmented value={s.video.fps} label="Frame rate" options={[[30, "30"], [60, "60"], [120, "120"]]} onchange={(v) => set(["video", "fps"], v)} />
    <span class="unit label">fps</span>
  </Field>
  <Field title="Quality" hint="Higher quality makes bigger files and uses more memory.">
    <Slider min={18} max={40} step={1} value={58 - quality} label="Quality" oninput={(v) => (quality = 58 - v)} onchange={(v) => set(["video", "quality"], 58 - v)} />
    <span class="value">{qualityName(quality)}</span>
  </Field>
  <Field title="Bitrate limit" hint="A ceiling for busy scenes; keeps memory use predictable.">
    <select value={s.video.max_bitrate_mbps} onchange={(e) => set(["video", "max_bitrate_mbps"], Number(e.currentTarget.value))} aria-label="Bitrate limit">
      {#each [20, 30, 40, 60, 80] as m (m)}<option value={m}>{m} Mbps</option>{/each}
    </select>
  </Field>
</div>

<div class="group">
  <Field title="Performance mode" hint={performanceHints[performance]}>
    <Segmented
      value={performance}
      label="Performance mode"
      options={[["off", "Off"], ["on", "Performance"], ["plus", "Performance+"]]}
      onchange={(v) => set(["video", "performance"], v)}
    />
  </Field>
  {#if performance !== "plus"}
    <Field title="Capture method" hint="Light does the least work on the graphics card, for weaker ones. Automatic picks what works best on your Windows.">
      <select value={s.video.capture ?? "auto"} onchange={(e) => set(["video", "capture"], e.currentTarget.value)} aria-label="Capture method">
        <option value="auto">Automatic</option>
        <option value="light">Light</option>
        <option value="wgc">Windows Graphics Capture</option>
        <option value="duplication">Desktop Duplication</option>
      </select>
    </Field>
  {/if}
</div>

<style>
  .unit {
    font-size: 10px;
  }

  .screen {
    max-width: 300px;
  }
</style>
