<script lang="ts">
  import HotkeyInput from "../HotkeyInput.svelte";
  import Field from "../ui/Field.svelte";
  import Slider from "../ui/Slider.svelte";
  import { set, store } from "./store.svelte";

  /** What games actually average at 1440p AV1 (measured with CS2). */
  const TYPICAL_MBPS = 28;

  const s = $derived(store.s!);
  let length = $state(store.s!.replay.length_secs as number);
  let ram = $derived({
    typical: Math.round((length * TYPICAL_MBPS) / 8),
    worst: Math.round(((length + 2) * s.video.max_bitrate_mbps) / 8),
  });

  function fmt(secs: number) {
    return secs >= 60 ? `${Math.floor(secs / 60)} min${secs % 60 ? ` ${secs % 60} s` : ""}` : `${secs} s`;
  }
</script>

<div class="group">
  <Field title="Save hotkey" hint="Saves the last moments from anywhere, even in fullscreen games.">
    <HotkeyInput value={s.replay.hotkey} onchange={(h) => set(["replay", "hotkey"], h)} />
  </Field>
  <Field title="Clip length" hint="How much the hotkey saves. Changing it keeps what's already buffered.">
    <Slider min={10} max={300} step={5} value={length} label="Clip length" oninput={(v) => (length = v)} onchange={(v) => set(["replay", "length_secs"], v)} />
    <span class="value">{fmt(length)}</span>
  </Field>
</div>

<div class="ram">
  <div class="bar"><div class="typical" style="width: {Math.min(100, (ram.typical / ram.worst) * 100)}%"></div></div>
  <p>
    Keeping {fmt(length)} in memory takes about <b class="mono">{ram.typical} MB</b> while gaming, and never more than
    <b class="mono">{ram.worst} MB</b> (at the {s.video.max_bitrate_mbps} Mbps limit). Quiet desktop scenes take far less.
  </p>
</div>

<style>
  .ram {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 4px;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .typical {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s var(--ease);
  }

  p {
    margin: 0;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.55;
  }

  b {
    color: var(--text);
    font-weight: 500;
  }
</style>
