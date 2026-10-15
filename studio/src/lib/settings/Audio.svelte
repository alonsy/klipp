<script lang="ts">
  import Icon from "../Icon.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import { deviceOptions, devicesOf, matchDevice, set, store } from "./store.svelte";

  interface Source {
    name: string;
    device: string;
    kind: "loopback" | "capture";
  }

  // The saved tracks, and a new one being set up. A new track is only saved
  // once it has a device, so recording restarts once, not on every step.
  let sources = $state<Source[]>(structuredClone($state.snapshot(store.s!.audio.sources)));
  let draft = $state<Source | null>(null);

  function save() {
    set(["audio", "sources"], $state.snapshot(sources));
  }

  /** The draft's "pick a device" placeholder ("" means the Windows default). */
  const PICK = "__pick";

  function addDraft() {
    draft = { name: `Track ${sources.length + 1}`, device: "", kind: "loopback" };
  }

  function commitDraft(device: string) {
    if (!draft || device === PICK) return;
    sources.push({ ...draft, device });
    draft = null;
    save();
  }

  const kinds = [
    ["loopback", "What it plays"],
    ["capture", "Microphone"],
  ] as const;
</script>

<div class="banner"><Icon name="info" size={17} />Changing tracks restarts recording for about a second, and what was buffered so far is dropped.</div>

<p class="intro">
  Each source becomes its own track that you can level in the editor, plus a <b>Mix</b> track of all of them that players and chat apps use.
</p>

<div class="group tracks">
  {#each sources as src, i (i)}
    <div class="track">
      <span class="num mono">{i + 1}</span>
      <input class="name" value={src.name} aria-label="Track name" onchange={(e) => ((src.name = e.currentTarget.value.trim() || src.name), save())} />
      <Segmented value={src.kind} label="Source type" options={kinds} onchange={(v) => ((src.kind = v), (src.device = devicesOf(v)[0] ?? src.device), save())} />
      <select
        class="device"
        value={matchDevice(src.kind, src.device)}
        onchange={(e) => ((src.device = e.currentTarget.value), save())}
        aria-label="Device"
      >
        <option value="">Windows default</option>
        {#each deviceOptions(src.kind, src.device) as name (name)}<option value={name}>{name}</option>{/each}
      </select>
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
    <div class="track new">
      <span class="num mono">{sources.length + 1}</span>
      <input class="name" bind:value={draft.name} aria-label="Track name" />
      <Segmented value={draft.kind} label="Source type" options={kinds} onchange={(v) => draft && (draft.kind = v)} />
      <select class="device" value={PICK} onchange={(e) => commitDraft(e.currentTarget.value)} aria-label="Device">
        <option value={PICK} disabled>Pick a device…</option>
        <option value="">Windows default</option>
        {#each devicesOf(draft.kind) as name (name)}<option value={name}>{name}</option>{/each}
      </select>
      <button class="ghost icon" aria-label="Cancel" title="Cancel" onclick={() => (draft = null)}><Icon name="x" size={16} /></button>
    </div>
    <p class="draft-hint">The track is added once you pick its device.</p>
  {/if}

  {#if sources.length === 0 && !draft}
    <p class="none">No audio is recorded. Add a track for your game sound or microphone.</p>
  {/if}
</div>

{#if !draft}
  <button class="add" onclick={addDraft}><Icon name="mic" size={16} />Add track</button>
{/if}

<style>
  .intro {
    margin: 0 4px;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.55;
  }

  .intro b {
    color: var(--accent);
  }

  .tracks {
    padding: 6px;
  }

  .track {
    display: grid;
    grid-template-columns: 26px 120px auto 1fr auto;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 12px;
    border-radius: 10px;
  }

  .track + .track {
    border-top: 1px solid var(--line);
    border-radius: 0;
  }

  .track.new {
    background: var(--accent-soft);
    border-radius: 10px;
  }

  .num {
    font-size: 11px;
    color: var(--faint);
  }

  .name,
  .device {
    min-width: 0;
  }

  .draft-hint,
  .none {
    margin: 6px 12px 8px;
    font-size: 12px;
    color: var(--muted);
  }

  .add {
    align-self: flex-start;
  }
</style>
