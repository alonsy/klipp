<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "../api";
  import type { MovePlan } from "../api";
  import { size } from "../format";
  import Icon from "../Icon.svelte";
  import SqueezeDialog from "../SqueezeDialog.svelte";
  import { library, reload, toast } from "../state.svelte";
  import { onMount } from "svelte";
  import Field from "../ui/Field.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { set, store } from "./store.svelte";

  const st = $derived(store.s!.storage);

  // The drive the clips are on: how full it is, and Klipp's part of it.
  let drive = $state<api.Drive | null>(null);
  onMount(() => {
    api.drives().then((list) => {
      const dir = library.clipsDir.toUpperCase();
      drive = list.find((d) => dir.startsWith(d.letter)) ?? null;
    }, () => {});
  });
  const GB = 1e9;
  const disk = $derived.by(() => {
    if (!drive || drive.totalGb <= 0) return null;
    const klipp = total / GB;
    const free = drive.freeGb;
    const other = Math.max(0, drive.totalGb - free - klipp);
    const pct = (gb: number) => (gb / drive!.totalGb) * 100;
    return { klipp, free, other, total: drive.totalGb, klippPct: pct(klipp), otherPct: pct(other) };
  });

  const AGES = [0, 7, 14, 30, 60, 90, 180, 365];
  const SIZES = [0, 10, 25, 50, 100, 200, 500, 1000];

  /** What the auto-delete settings would remove right now (the recorder does the deleting). */
  const wouldDelete = $derived.by(() => {
    const days = Number(st.delete_after_days ?? 30);
    const maxGb = Number(st.max_gb ?? 0);
    const oldest = [...library.clips].sort((a, b) => a.created - b.created);
    const gone = new Set<string>();
    if (days > 0) {
      const before = Date.now() - days * 86_400_000;
      for (const c of oldest) if (!c.favorite && c.created < before) gone.add(c.path);
    }
    if (maxGb > 0) {
      let left = oldest.reduce((t, c) => t + (gone.has(c.path) ? 0 : c.sizeBytes), 0);
      for (const c of oldest) {
        if (left <= maxGb * GB) break;
        if (c.favorite || gone.has(c.path)) continue;
        gone.add(c.path);
        left -= c.sizeBytes;
      }
    }
    const clips = library.clips.filter((c) => gone.has(c.path));
    return { n: clips.length, bytes: clips.reduce((t, c) => t + c.sizeBytes, 0) };
  });

  // Changing the folder: pick it, then (if there are clips) move them or not.
  type Change = { kind: "ask"; from: string; to: string; plan: MovePlan } | { kind: "moving"; to: string; done: number; total: number };
  let change = $state<Change | null>(null);
  let squeezing = $state(false);

  async function pick() {
    const to = await open({ directory: true, defaultPath: library.clipsDir, title: "Choose where Klipp saves clips" }).catch(() => null);
    if (typeof to !== "string" || !to) return;
    let plan: MovePlan;
    try {
      plan = await api.planMove(to);
    } catch (e) {
      toast(`Couldn't use that folder: ${e}`);
      return;
    }
    if (plan.problem) {
      toast(plan.problem);
      return;
    }
    if (plan.clips === 0 && plan.bytes === 0) {
      await use(to);
      toast(`Clips are now saved in ${to}.`);
      return;
    }
    change = { kind: "ask", from: library.clipsDir, to, plan };
  }

  /** New clips go to `to` from now on. */
  async function use(to: string) {
    await set(["storage", "clips_dir"], to);
    await reload();
  }

  async function leave() {
    if (change?.kind !== "ask") return;
    const { from, to } = change;
    change = null;
    await use(to);
    toast(`New clips are saved in ${to}. Older ones stay in ${from}.`);
  }

  async function move() {
    if (change?.kind !== "ask") return;
    const { from, to, plan } = change;
    const moving = { kind: "moving" as const, to, done: 0, total: plan.bytes };
    change = moving;
    const off = await api.onMoveProgress((done, total) => {
      if (change?.kind === "moving") change = { ...moving, done, total };
    });
    try {
      // The recorder saves into the new folder from now on, even during the move.
      await set(["storage", "clips_dir"], to);
      const report = await api.moveClips(from, to);
      const what = report.moved === 1 ? "1 clip" : `${report.moved} clips`;
      if (report.left.length === 0) toast(`Moved ${what} to ${to}.`);
      else toast(`Moved ${what}. ${report.left.length} stayed in ${from}: open in another app, or already in the new folder.`, undefined, 12000);
    } catch (e) {
      toast(`Moving stopped: ${e}. Clips that weren't moved yet are still in ${from}.`, undefined, 12000);
    } finally {
      off();
      change = null;
      await reload();
    }
  }

  let byGame = $derived.by(() => {
    const m = new Map<string, { n: number; bytes: number }>();
    for (const c of library.clips) {
      const g = m.get(c.game) ?? { n: 0, bytes: 0 };
      g.n++;
      g.bytes += c.sizeBytes;
      m.set(c.game, g);
    }
    return [...m].sort((a, b) => b[1].bytes - a[1].bytes);
  });
  let total = $derived(byGame.reduce((t, [, g]) => t + g.bytes, 0));
</script>

<div class="group">
  <Field title="Clips folder" hint="Each game gets its own folder inside. Klipp keeps its notes about clips in a hidden .klipp folder there.">
    <span class="path mono" title={library.clipsDir}>{library.clipsDir}</span>
    <button onclick={pick} disabled={change?.kind === "moving"}>Change…</button>
    <button onclick={() => api.reveal(library.clipsDir)}>Open</button>
  </Field>
  {#if change?.kind === "ask"}
    {@const p = change.plan}
    <div class="move">
      <p>
        <b>Move your {p.clips === 1 ? "clip" : `${p.clips} clips`} ({size(p.bytes)}) to <span class="mono">{change.to}</span>?</b>
        They keep their trims, favorites and thumbnails.
        {p.sameDrive ? "It's the same drive, so it only takes a moment." : "They're copied to the other drive, then removed here."}
        If you don't move them, they stay where they are and the library only shows the new folder.
      </p>
      {#if !p.room}
        <p class="bad"><Icon name="warning" size={15} />Not enough space there: they need {size(p.bytes)}, and {size(p.freeBytes ?? 0)} is free.</p>
      {/if}
      <div class="actions">
        <button class="primary" disabled={!p.room} onclick={move}>Move clips</button>
        <button onclick={leave}>Don't move them</button>
        <button class="ghost" onclick={() => (change = null)}>Cancel</button>
      </div>
    </div>
  {:else if change?.kind === "moving"}
    {@const pct = change.total > 0 ? Math.min(100, Math.round((change.done / change.total) * 100)) : 0}
    <div class="move">
      <p><b>Moving clips… {pct}%</b> Klipp keeps recording; new clips already go to the new folder.</p>
      <div class="bar"><div class="fill" style="width: {pct}%"></div></div>
    </div>
  {/if}
</div>

<span class="label group-title">Space</span>
<div class="group usage">
  {#if disk}
    <div class="total">
      <span class="big mono">{disk.free.toFixed(0)} GB</span><span class="muted">free on {drive?.letter} of {disk.total.toFixed(0)} GB</span>
    </div>
    <div class="disk" role="img" aria-label="Klipp's clips {size(total)}, other files {disk.other.toFixed(0)} GB, free {disk.free.toFixed(0)} GB">
      <span class="klipp" style="width: {Math.max(disk.klippPct, 0.6)}%"></span>
      <span class="other" style="width: {disk.otherPct}%"></span>
    </div>
    <div class="legend">
      <span><i class="dot"></i>Klipp's clips <b class="mono">{size(total)}</b></span>
      <span><i class="dot other"></i>Other files <b class="mono">{disk.other.toFixed(0)} GB</b></span>
      <span><i class="dot free"></i>Free <b class="mono">{disk.free.toFixed(0)} GB</b></span>
    </div>
  {:else}
    <div class="total"><span class="big mono">{size(total)}</span><span class="muted">{library.clips.length} clips</span></div>
  {/if}
  <ul>
    {#each byGame as [game, g], i (game)}
      <li>
        <span class="dot" style="opacity: {1 - i * 0.18}"></span>
        <span class="game">{game}</span>
        <span class="muted mono">{g.n === 1 ? "1 clip" : `${g.n} clips`}</span>
        <span class="mono">{size(g.bytes)}</span>
      </li>
    {/each}
  </ul>
</div>

<div class="group">
  <Field title="Squeeze space" hint="Makes your clips smaller by re-encoding them. You pick how much quality to give up.">
    <button onclick={() => (squeezing = true)} disabled={library.clips.length === 0}>Squeeze space…</button>
  </Field>
</div>
<SqueezeDialog open={squeezing} onclose={() => (squeezing = false)} />

<span class="label group-title">Delete old clips</span>
<div class="group">
  <Field title="Delete old clips by themselves" hint="Favorites are never deleted. Deleted clips go to the Recycle Bin.">
    <Toggle checked={st.auto_delete ?? false} label="Delete old clips by themselves" onchange={(v) => set(["storage", "auto_delete"], v)} />
  </Field>
  {#if st.auto_delete}
    <Field title="Older than">
      <select value={Number(st.delete_after_days ?? 30)} onchange={(e) => set(["storage", "delete_after_days"], Number(e.currentTarget.value))} aria-label="Delete clips older than">
        {#each AGES as d (d)}<option value={d}>{d === 0 ? "Any age" : d === 365 ? "1 year" : `${d} days`}</option>{/each}
      </select>
    </Field>
    <Field title="When clips take more than" hint="The oldest go first.">
      <select value={Number(st.max_gb ?? 0)} onchange={(e) => set(["storage", "max_gb"], Number(e.currentTarget.value))} aria-label="Delete the oldest clips when clips take more than">
        {#each SIZES as g (g)}<option value={g}>{g === 0 ? "No limit" : g >= 1000 ? "1 TB" : `${g} GB`}</option>{/each}
      </select>
    </Field>
    <p class="preview">
      {#if Number(st.delete_after_days ?? 30) === 0 && Number(st.max_gb ?? 0) === 0}
        Pick an age or a size, or nothing is deleted.
      {:else if wouldDelete.n === 0}
        Nothing would be deleted right now.
      {:else}
        Right now this deletes <b>{wouldDelete.n === 1 ? "1 clip" : `${wouldDelete.n} clips`}</b> ({size(wouldDelete.bytes)}), within the hour.
      {/if}
    </p>
  {/if}
</div>
<style>
  .path {
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--muted);
  }

  .move {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 18px 16px;
    border-top: 1px solid var(--line);
  }

  .move p {
    margin: 0;
    max-width: 640px;
    color: var(--muted);
    line-height: 1.55;
  }

  .move b {
    color: var(--text);
    font-weight: 600;
  }

  .move .bad {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--danger);
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s linear;
  }

  .usage {
    padding: 18px;
  }

  .total {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 12px;
  }

  .big {
    font-size: 22px;
    font-weight: 700;
  }

  .muted {
    color: var(--muted);
  }


  .disk {
    display: flex;
    gap: 2px;
    height: 12px;
    border-radius: 6px;
    overflow: hidden;
    background: color-mix(in srgb, var(--surface-3) 60%, transparent);
  }

  .disk span {
    height: 100%;
    transition: width 0.3s var(--ease);
  }

  .disk .klipp {
    background: var(--accent);
  }

  .disk .other {
    background: var(--surface-3);
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin-top: 10px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .legend b {
    color: var(--text);
    font-weight: 500;
  }

  .legend .dot {
    display: inline-block;
  }

  .dot.other {
    background: var(--surface-3);
  }

  .dot.free {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line);
  }

  .preview {
    margin: 0;
    padding: 0 18px 14px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .preview b {
    color: var(--text);
    font-weight: 600;
  }

  ul {
    list-style: none;
    margin: 14px 0 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 12px 1fr auto 80px;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    font-size: 13px;
  }

  li .mono {
    text-align: right;
    font-size: 12px;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 3px;
    background: var(--accent);
  }
</style>
