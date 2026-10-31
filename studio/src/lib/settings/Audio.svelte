<script lang="ts">
  import { onMount } from "svelte";
  import { DISCORD, routerInstalled } from "../discordTrack";
  import Icon from "../Icon.svelte";
  import LevelCheck from "../LevelCheck.svelte";
  import { enter, leave } from "../motion";
  import { prefs, setPref, toast, ui } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import Reveal from "./Reveal.svelte";
  import { deviceOptions, devicesOf, matchDevice, set, store } from "./store.svelte";

  interface Source {
    name: string;
    device: string;
    kind: "loopback" | "capture" | "discord";
  }

  // The saved tracks, and a new one being set up (Multi Device Track Mode). A
  // new track is only saved once it has a device, so recording restarts once.
  let sources = $state<Source[]>(structuredClone($state.snapshot(store.s!.audio.sources)));
  let draft = $state<Source | null>(null);

  function save() {
    set(["audio", "sources"], $state.snapshot(sources));
  }

  const playback = $derived(devicesOf("loopback"));
  const recording = $derived(devicesOf("capture"));
  const sonarFound = $derived(playback.some((n) => n.includes("Sonar - Gaming")) && playback.some((n) => n.includes("Sonar - Chat")));
  /** An audio router replays Discord from its own program; then Game can't leave it out. */
  const router = $derived(routerInstalled(playback));

  const isSonar = (list: Source[]) => list.some((s) => s.kind === "loopback" && /sonar - (gaming|chat)/i.test(s.device));
  /** One track of what the PC plays, maybe Discord's own and a microphone: what the simple mode edits. */
  const isSimple = (list: Source[]) => {
    const count = (k: Source["kind"]) => list.filter((s) => s.kind === k).length;
    return count("loopback") === 1 && count("capture") <= 1 && count("discord") <= 1 && !isSonar(list);
  };

  type Mode = "simple" | "sonar" | "multi";
  const mode = $derived<Mode>(prefs.audioMultiDevice || (!isSonar(sources) && !isSimple(sources)) ? "multi" : isSonar(sources) ? "sonar" : "simple");

  // The simple mode's choices, read from the tracks.
  const output = $derived(sources.find((s) => s.kind === "loopback")?.device ?? "");
  const mic = $derived(sources.find((s) => s.kind === "capture"));
  const hasDiscord = $derived(sources.some((s) => s.kind === DISCORD));

  function simple(next: { output?: string; discord?: boolean; mic?: boolean; micDevice?: string }) {
    const discord = (next.discord ?? hasDiscord) && !router;
    const withMic = next.mic ?? !!mic;
    sources = [
      { name: discord ? "Game" : "PC audio", device: next.output ?? output, kind: "loopback" },
      ...(discord ? [{ name: "Chat", device: "", kind: DISCORD } as Source] : []),
      ...(withMic ? [{ name: "Mic", device: next.micDevice ?? mic?.device ?? "", kind: "capture" } as Source] : []),
    ];
    save();
  }

  function useSonar(on: boolean) {
    if (on) {
      const withMic = !!mic || recording.some((n) => n.includes("Sonar - Microphone"));
      sources = [
        { name: "Game", device: "Sonar - Gaming", kind: "loopback" },
        { name: "Chat", device: "Sonar - Chat", kind: "loopback" },
        ...(withMic ? [{ name: "Mic", device: "Sonar - Microphone", kind: "capture" } as Source] : []),
      ];
      save();
    } else {
      simple({ output: "", discord: false, micDevice: "" });
    }
  }

  function setMulti(on: boolean) {
    setPref("audioMultiDevice", on);
    // Back to one output and one microphone: keep the first of each.
    if (!on && !isSimple(sources) && !isSonar(sources)) simple({});
  }

  onMount(() => {
    // Sonar splits game, chat and mic into devices: switch to its tracks once,
    // while the audio is still as setup or the defaults left it.
    const untouched = isSimple(sources) && sources.every((s) => s.device === "") && !hasDiscord;
    const check = setInterval(() => {
      if (playback.length === 0) return;
      clearInterval(check);
      if (sonarFound && untouched && !prefs.sonarSwitched) {
        setPref("sonarSwitched", true);
        useSonar(true);
        toast("SteelSeries Sonar found: Klipp now records its Game, Chat and Mic as separate tracks.");
      }
    }, 300);
    return () => clearInterval(check);
  });

  /** The draft's "pick a device" placeholder ("" means the Windows default). */
  const PICK = "__pick";
  const kinds = [
    ["loopback", "What it plays"],
    ["capture", "Microphone"],
  ] as const;

  function commitDraft(device: string) {
    if (!draft || device === PICK) return;
    sources.push({ ...draft, device });
    draft = null;
    save();
  }
</script>

<div class="group">
  {#if sonarFound && mode !== "multi"}
    <Field title="SteelSeries Sonar" tip="Record Sonar's Game, Chat and Mic as separate tracks, so you can turn voices down later or leave them out when you share.">
      <Toggle checked={mode === "sonar"} label="Use Sonar's tracks" onchange={useSonar} />
    </Field>
  {/if}

  <Reveal open={mode === "simple"}>
    <Field title="Output" tip="What you hear: your game and everything else.">
      <select value={matchDevice("loopback", output)} onchange={(e) => simple({ output: e.currentTarget.value })} aria-label="Output">
        <option value="">Windows default</option>
        {#each deviceOptions("loopback", output) as name (name)}<option value={name}>{name}</option>{/each}
      </select>
    </Field>
    <Field title="Microphone" tip="Your voice, on its own track.">
      {#if mic}
        <select value={matchDevice("capture", mic.device)} onchange={(e) => simple({ micDevice: e.currentTarget.value })} aria-label="Microphone" in:enter={{ x: 8, y: 0 }}>
          <option value="">Windows default</option>
          {#each deviceOptions("capture", mic.device) as name (name)}<option value={name}>{name}</option>{/each}
        </select>
      {/if}
      <Toggle checked={!!mic} label="Record the microphone" onchange={(v) => simple({ mic: v })} />
    </Field>
    <Field
      title="Separate Discord"
      badge="Experimental"
      tip={router ? undefined : "Discord voice chat on its own track, the rest on Game. The Discord app only, not Discord in a browser. It may not work on every PC."}
      hint={router ? "Not while an audio mixer like Sonar or Voicemeeter is installed: it plays Discord again through its own program." : undefined}
    >
      <Toggle checked={hasDiscord} label="Try separating Discord and Windows sounds" disabled={router && !hasDiscord} onchange={(v) => simple({ discord: v })} />
    </Field>
    <Reveal open={hasDiscord}>
      <div class="check">
        <LevelCheck hint="Play something in Discord (Voice & Video → Let's Check). Chat should move while Game stays still." />
      </div>
    </Reveal>
    <div class="space"></div>
  </Reveal>

  <Reveal open={mode === "sonar"}>
    <div class="tracks">
      {#each sources as src, i (i)}
        <div class="row">
          <span class="name">{src.name}</span>
          <span class="dev">{src.device}</span>
        </div>
      {/each}
    </div>
    <p class="note after">
      Pick your headset under
      <button class="link" onclick={() => (ui.section = "recording")}>Recording → Play on</button>, so the save sound stays out of your clips.
    </p>
    <div class="space"></div>
  </Reveal>

  <Field title="Multi Device Track Mode" badge="Advanced" tip="For sound cards and audio software: record any devices, each as its own track, plus a Mix of all of them.">
    <Toggle checked={mode === "multi"} label="Multi Device Track Mode" onchange={setMulti} />
  </Field>

  <Reveal open={mode === "multi"}>
    <div class="tracks">
      {#each sources as src, i (i)}
        <div class="track">
          <input class="name" value={src.name} aria-label="Track name" onchange={(e) => ((src.name = e.currentTarget.value.trim() || src.name), save())} />
          {#if src.kind === DISCORD}
            <span class="fixed">Only the Discord app <span class="exp">Experimental</span></span>
          {:else}
            <Segmented value={src.kind} label="Source type" options={kinds} onchange={(v) => ((src.kind = v), (src.device = devicesOf(v)[0] ?? src.device), save())} />
            <select value={matchDevice(src.kind, src.device)} onchange={(e) => ((src.device = e.currentTarget.value), save())} aria-label="Device">
              <option value="">Windows default</option>
              {#each deviceOptions(src.kind, src.device) as name (name)}<option value={name}>{name}</option>{/each}
            </select>
          {/if}
          <button
            class="ghost icon"
            aria-label="Remove track"
            title="Remove track"
            onclick={() => {
              sources.splice(i, 1);
              save();
            }}><Icon name="trash" size={16} /></button
          >
        </div>
      {/each}

      {#if draft}
        <div class="track" in:enter={{ y: -4 }} out:leave>
          <input class="name" bind:value={draft.name} aria-label="Track name" />
          <Segmented value={draft.kind} label="Source type" options={kinds} onchange={(v) => draft && (draft.kind = v)} />
          <select value={PICK} onchange={(e) => commitDraft(e.currentTarget.value)} aria-label="Device">
            <option value={PICK} disabled>Pick a device…</option>
            <option value="">Windows default</option>
            {#each devicesOf(draft.kind) as name (name)}<option value={name}>{name}</option>{/each}
          </select>
          <button class="ghost icon" aria-label="Cancel" title="Cancel" onclick={() => (draft = null)}><Icon name="x" size={16} /></button>
        </div>
        <p class="note after">The track is added once you pick its device.</p>
      {/if}

      {#if sources.length === 0 && !draft}
        <p class="note after">No audio is recorded. Add a track for your game sound or microphone.</p>
      {/if}
    </div>
    {#if !draft}
      <button class="add" onclick={() => (draft = { name: `Track ${sources.length + 1}`, device: "", kind: "loopback" })}><Icon name="mic" size={16} />Add track</button>
    {/if}
  </Reveal>
</div>

<p class="note">Changing audio restarts recording for about a second, and what was buffered so far is dropped.</p>

<style>
  .check :global(.levels) {
    padding: var(--sp-2) 0;
  }

  .space {
    height: var(--sp-4);
  }

  .tracks {
    display: flex;
    flex-direction: column;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-5);
    min-height: 40px;
  }

  .row .name {
    font-weight: 550;
  }

  .dev {
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .track {
    display: grid;
    grid-template-columns: 112px auto minmax(0, 1fr) 34px;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) 0;
  }

  .track select {
    min-width: 0;
    max-width: none;
  }

  .track input {
    min-width: 0;
  }

  .fixed {
    grid-column: span 2;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .exp {
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .after {
    margin-top: var(--sp-1);
  }

  .add {
    margin-top: var(--sp-2);
  }

  .link {
    display: inline;
    padding: 0;
    background: none;
    font-size: inherit;
    color: var(--muted);
    text-decoration: underline;
  }

  .link:hover {
    background: none;
    color: var(--text);
  }
</style>
