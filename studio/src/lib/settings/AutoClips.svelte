<script lang="ts">
  import Icon from "../Icon.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);
  const a = $derived(s.auto_clip);
  const cs2 = $derived(s.auto_clip.cs2);
  let pre = $state(Number(store.s!.auto_clip.pre_roll_secs));
  let post = $state(Number(store.s!.auto_clip.post_roll_secs));
  let gap = $state(Number(store.s!.auto_clip.merge_gap_secs));

  const events = [
    ["headshot", "headshot", "Every headshot", "A clip for each headshot kill."],
    ["any_kill", "kill", "Every kill", "Lots of clips; good for practice sessions."],
    ["knife_or_zeus_kill", "knife", "Knife and Zeus kills", "The humiliating ones."],
    ["death", "death", "Deaths", "See what got you."],
    ["mvp", "mvp", "Round MVP", "When you're the round's MVP."],
  ] as const;
</script>

<div class="group">
  <Field title="Save clips automatically" hint="Klipp watches supported games and saves the moments worth keeping, with no hotkey.">
    <Toggle checked={a.enabled} label="Save clips automatically" onchange={(v) => set(["auto_clip", "enabled"], v)} />
  </Field>
</div>

<span class="label group-title">Timing</span>
<div class="group" class:disabled={!a.enabled}>
  <Field title="Keep before" hint="Seconds before the first event of a clip.">
    <Slider min={0} max={30} value={pre} label="Keep before" oninput={(v) => (pre = v)} onchange={(v) => set(["auto_clip", "pre_roll_secs"], v)} />
    <span class="value">{pre} s</span>
  </Field>
  <Field title="Keep after" hint="Seconds after the last one.">
    <Slider min={0} max={15} value={post} label="Keep after" oninput={(v) => (post = v)} onchange={(v) => set(["auto_clip", "post_roll_secs"], v)} />
    <span class="value">{post} s</span>
  </Field>
  <Field title="Join events within" hint="Events this close together become one clip: a 2K followed by a third kill is one 3K clip, not two.">
    <Slider min={3} max={30} value={gap} label="Join events within" oninput={(v) => (gap = v)} onchange={(v) => set(["auto_clip", "merge_gap_secs"], v)} />
    <span class="value">{gap} s</span>
  </Field>
</div>

<div class="game-head">
  <Icon event="kill" size={22} />
  <span class="game">Counter-Strike 2</span>
  <Toggle checked={cs2.enabled} label="Counter-Strike 2 auto-clips" onchange={(v) => set(["auto_clip", "cs2", "enabled"], v)} />
</div>
<p class="note">Uses Valve's Game State Integration, which is allowed with VAC. Restart CS2 once after turning it on.</p>

<div class="group" class:disabled={!a.enabled || !cs2.enabled}>
  <Field title="Multi-kills" hint="Clip kills in quick succession, starting from this many.">
    <Segmented
      value={cs2.multi_kill_min}
      label="Multi-kills"
      options={[[0, "Off"], [2, "2K"], [3, "3K"], [4, "4K"], [5, "ACE"]]}
      onchange={(v) => set(["auto_clip", "cs2", "multi_kill_min"], v)}
    />
  </Field>
  <Field
    title="What counts as a 2K"
    hint={cs2.multi_kill_mode === "round"
      ? "Kills anywhere in the same round, like the scoreboard. The clip covers the last kills that were close together."
      : `Kills within ${gap} s of each other: the highlight you'd actually want to watch.`}
  >
    <Segmented
      value={cs2.multi_kill_mode}
      label="What counts as a multi-kill"
      options={[["streak", "Close together"], ["round", "Same round"]]}
      onchange={(v) => set(["auto_clip", "cs2", "multi_kill_mode"], v)}
    />
  </Field>
  {#each events as [key, icon, title, hint] (key)}
    <Field {title} {hint}>
      <span class="ev"><Icon event={icon} size={20} /></span>
      <Toggle checked={cs2[key]} label={title} onchange={(v) => set(["auto_clip", "cs2", key], v)} />
    </Field>
  {/each}
</div>

<p class="note">Round wins, losses and defuses are marked on the timeline of every clip, but never saved on their own.</p>

<span class="label group-title">Advanced</span>
<div class="group">
  <Field title="Timing log" hint="Writes every game update to cs2.log, to check how quickly events arrive.">
    <Toggle checked={cs2.debug_log} label="Timing log" onchange={(v) => set(["auto_clip", "cs2", "debug_log"], v)} />
  </Field>
</div>

<style>
  .disabled {
    opacity: 0.5;
  }

  .game-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 8px 4px -10px;
  }

  .game {
    flex: 1;
    font-size: 16px;
    font-weight: 650;
  }

  .note {
    margin: 0 4px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--muted);
  }

  .ev {
    display: inline-flex;
    opacity: 0.85;
  }
</style>
