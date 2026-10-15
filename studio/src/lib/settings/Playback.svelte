<script lang="ts">
  import { prefs, setPref } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const keys = [
    [["Space"], "Play / pause"],
    [["I"], "Start the clip here; keeps playing from the new start"],
    [["O"], "End the clip here; replays the last 2 seconds"],
    [["L"], "Loop the trimmed part"],
    [["←", "→"], "Back / forward 1 s (Shift: 5 s)"],
    [[",", "."], "Previous / next frame"],
    [["[", "]"], "Previous / next event"],
    [["Home", "End"], "Go to the start / end of the trim"],
    [["F"], "Fullscreen"],
    [["Esc"], "Back to clips"],
  ] as const;
</script>

<div class="group">
  <Field title="Play clips when opened">
    <Toggle checked={prefs.autoplay} label="Play clips when opened" onchange={(v) => setPref("autoplay", v)} />
  </Field>
  <Field title="Loop by default" hint="Replay the trimmed part until you stop it. L switches it in the editor.">
    <Toggle checked={prefs.loop} label="Loop by default" onchange={(v) => setPref("loop", v)} />
  </Field>
</div>

<p class="remember">Trim points and track levels are remembered for each clip, so you can come back to an edit later.</p>

<span class="label group-title">Editor keys</span>
<div class="group keys">
  {#each keys as [combo, what] (what)}
    <div class="key">
      <span class="combo">{#each combo as k (k)}<kbd>{k}</kbd>{/each}</span>
      <span>{what}</span>
    </div>
  {/each}
</div>

<style>
  .remember {
    margin: 0 4px;
    font-size: 13px;
    color: var(--muted);
  }

  .keys {
    padding: 8px 18px;
  }

  .key {
    display: grid;
    grid-template-columns: 120px 1fr;
    align-items: center;
    padding: 7px 0;
    font-size: 13px;
  }

  .key + .key {
    border-top: 1px solid var(--line);
  }

  .combo {
    display: flex;
    gap: 4px;
  }
</style>
