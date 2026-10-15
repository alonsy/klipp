<script lang="ts">
  import Icon from "../Icon.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);
  let quality = $state(store.s!.video.quality as number);

  function qualityName(q: number) {
    return q <= 24 ? "Very high" : q <= 30 ? "High" : q <= 35 ? "Medium" : "Low";
  }
</script>

<div class="banner"><Icon name="info" size={17} />Changing these restarts recording for about a second, and what was buffered so far is dropped.</div>

<div class="group">
  <Field title="Codec" hint="AV1 makes files about half the size. H.264 plays on anything, even old phones.">
    <Segmented value={s.video.codec} label="Codec" options={[["av1", "AV1"], ["h264", "H.264"]]} onchange={(v) => set(["video", "codec"], v)} />
  </Field>
  <Field title="Resolution" hint="Native records your screen as it is.">
    <Segmented
      value={s.video.resolution}
      label="Resolution"
      options={[["native", "Native"], ["1080p", "1080p"], ["720p", "720p"]]}
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

<style>
  .unit {
    font-size: 10px;
  }
</style>
