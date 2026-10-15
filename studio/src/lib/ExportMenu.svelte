<script lang="ts">
  // The Export button in the editor's play bar, and the options it opens:
  // share (fits a size limit), a lossless trimmed copy, or high quality.
  import { onMount } from "svelte";
  import * as api from "./api";
  import type { ExportMode } from "./api";
  import { size } from "./format";
  import Icon from "./Icon.svelte";
  import { prefs, reload, setPref, toast } from "./state.svelte";
  import Toggle from "./ui/Toggle.svelte";

  interface Props {
    path: string;
    inPoint: number;
    outPoint: number;
    volumes: number[];
    mixVolume: number;
  }
  let { path, inPoint, outPoint, volumes, mixVolume }: Props = $props();

  // Remembered between sessions; a per-viewer convenience only.
  const TARGET_KEY = "klipp.shareTargetMb";
  let targetMb = $state(10);
  try {
    targetMb = Number(localStorage.getItem(TARGET_KEY)) || 10;
  } catch {
    // Storage unavailable; keep the default.
  }

  let open = $state(false);
  let root: HTMLDivElement;
  let busy = $state<{ id: number; mode: ExportMode; fraction: number } | null>(null);
  let result = $state<{ path: string; sizeBytes: number; mode: ExportMode } | null>(null);

  onMount(() => {
    let off = () => {};
    api
      .onExport({
        progress: (id, fraction) => {
          if (busy?.id === id) busy.fraction = fraction;
        },
        done: async (id, out, sizeBytes) => {
          if (busy?.id !== id) return;
          const mode = busy.mode;
          busy = null;
          result = { path: out, sizeBytes, mode };
          if (mode === "trim") reload();
          const what = { share: `Ready to share (${size(sizeBytes)})`, trim: "Trimmed copy saved to your library", quality: `Exported (${size(sizeBytes)})` }[mode];
          const copyNow = () => api.copyToClipboard(out).then(() => toast("Copied. Paste it with Ctrl+V."), (e) => toast(`Couldn't copy: ${e}`));
          if (prefs.copyAfterExport) {
            await api.copyToClipboard(out).then(
              () => toast(`${what} and copied. Paste it with Ctrl+V.`),
              () => toast(what, { label: "Copy", run: copyNow }),
            );
          } else {
            // Left the clipboard alone; one click copies it if wanted.
            toast(what, { label: "Copy", run: copyNow }, 8000);
          }
        },
        failed: (id, error) => {
          if (busy?.id !== id) return;
          busy = null;
          if (!/cancel/i.test(error)) toast(`Export failed: ${error}`);
        },
      })
      .then((unlisten) => (off = unlisten));
    return () => off();
  });

  async function start(mode: ExportMode) {
    if (busy) return;
    result = null;
    try {
      localStorage.setItem(TARGET_KEY, String(targetMb));
    } catch {
      // Not important.
    }
    const id = await api.startExport({ path, start: inPoint, end: outPoint, mode, targetMb, volumes, mixVolume });
    busy = { id, mode, fraction: 0 };
  }

  function outside(e: PointerEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }

  /** E opens it; Escape closes it (before the editor sees Escape). */
  function key(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement).tagName;
    if (open && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    } else if (!open && (e.key === "e" || e.key === "E") && tag !== "INPUT" && tag !== "SELECT" && !e.ctrlKey && !e.altKey) {
      open = true;
    }
  }

  let names = $derived<Record<ExportMode, string>>({ trim: "Saving trimmed copy", quality: "Exporting", share: `Fitting it under ${targetMb} MB` });
  const targets = [
    [10, "10 MB"],
    [50, "50 MB"],
    [500, "500 MB"],
  ] as const;
</script>

<svelte:window onpointerdown={outside} onkeydowncapture={key} />

<div class="export" bind:this={root}>
  <button
    class="primary trigger"
    class:busy={!!busy}
    style="--p: {busy ? busy.fraction * 100 : 0}%"
    aria-expanded={open}
    title="Export or share this clip (E)"
    onclick={() => (open = !open)}
  >
    {#if busy}
      <span class="spinner"></span><span class="mono">{Math.round(busy.fraction * 100)}%</span>
    {:else}
      <Icon name="send" size={16} />Export
    {/if}
  </button>

  {#if open}
  <div class="panel" role="dialog" aria-label="Export">
  <div class="head">
    <span class="label">Export</span>
    <span class="len mono">{(outPoint - inPoint).toFixed(1)} s</span>
  </div>

  <div class="share">
    <button class="primary big" disabled={!!busy} onclick={() => start("share")}><Icon name="send" size={17} />Export to share</button>
    <div class="targets" role="radiogroup" aria-label="Size limit">
      {#each targets as [mb, name] (mb)}
        <button role="radio" aria-checked={targetMb === mb} class:on={targetMb === mb} disabled={!!busy} onclick={() => (targetMb = mb)}>{name}</button>
      {/each}
    </div>
    <span class="note">Fits under the size limit, so chat apps take it as is.</span>
  </div>

  <div class="others">
    <button disabled={!!busy} onclick={() => start("trim")} title="Same quality, every audio track kept, saved next to the original">
      <Icon name="scissors" size={16} />Save trimmed copy
    </button>
    <button disabled={!!busy} onclick={() => start("quality")} title="High quality H.264 with your track levels, plays everywhere">
      <Icon name="export" size={16} />Export high quality
    </button>
  </div>

  <div class="copy">
    <div class="copy-text">
      <span class="copy-title">Copy to clipboard when done</span>
      <span class="copy-hint">{prefs.copyAfterExport ? "Replaces what you have copied now, ready for Ctrl+V." : "Your clipboard is left alone."}</span>
    </div>
    <Toggle checked={prefs.copyAfterExport} label="Copy to clipboard when done" onchange={(v) => setPref("copyAfterExport", v)} />
  </div>

  {#if busy}
    <div class="progress">
      <div class="p-label"><span>{names[busy.mode]}…</span><span class="mono">{Math.round(busy.fraction * 100)}%</span></div>
      <div class="track"><div class="fill" style="width: {busy.fraction * 100}%"></div></div>
      <button class="ghost cancel" onclick={() => busy && api.cancelExport(busy.id)}>Cancel</button>
    </div>
  {:else if result}
    <div class="result">
      <Icon name="check" size={16} />
      <span class="file" title={result.path}>{result.path.split("\\").pop()} <span class="mono">· {size(result.sizeBytes)}</span></span>
      <button class="ghost icon" title="Copy" onclick={() => result && api.copyToClipboard(result.path).then(() => toast("Copied. Paste it anywhere with Ctrl+V."))}>
        <Icon name="copy" size={16} />
      </button>
      <button class="ghost icon" title="Show in folder" onclick={() => result && api.reveal(result.path)}><Icon name="folder" size={16} /></button>
    </div>
  {/if}
  </div>
  {/if}
</div>

<style>
  .export {
    position: relative;
  }

  .trigger {
    min-width: 108px;
  }

  /* While exporting, the button fills up with its progress. */
  .trigger.busy {
    background: linear-gradient(90deg, var(--accent) var(--p), color-mix(in srgb, var(--accent) 40%, var(--surface-2)) var(--p));
  }

  .spinner {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 2px solid currentColor;
    border-right-color: transparent;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .panel {
    position: absolute;
    bottom: calc(100% + 12px);
    left: 50%;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 320px;
    padding: 16px;
    border-radius: 16px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    box-shadow: 0 20px 50px rgba(0, 0, 0, 0.45);
    transform: translateX(-50%);
    animation: pop 0.16s var(--ease);
  }

  @keyframes pop {
    from {
      opacity: 0;
      transform: translate(-50%, 6px) scale(0.98);
    }
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .len {
    font-size: 12px;
    color: var(--muted);
  }

  .share {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--line);
  }

  .big {
    width: 100%;
    padding: 10px 14px;
  }

  .targets {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 4px;
    padding: 3px;
    border-radius: 9px;
    background: var(--bg);
  }

  .targets button {
    padding: 5px 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .targets button.on {
    background: var(--surface-3);
    color: var(--text);
  }

  .note {
    font-size: 11px;
    line-height: 1.45;
    color: var(--faint);
  }

  .others {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .others button {
    justify-content: flex-start;
  }

  .copy {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 2px 0;
    border-top: 1px solid var(--line);
  }

  .copy-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .copy-title {
    font-size: 13px;
    font-weight: 550;
  }

  .copy-hint {
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--muted);
  }

  .progress {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 8px;
    align-items: center;
  }

  .p-label {
    grid-column: 1 / -1;
    display: flex;
    justify-content: space-between;
    color: var(--muted);
    font-size: 13px;
  }

  .track {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s linear;
  }

  .cancel {
    padding: 3px 8px;
    font-size: 12px;
  }

  .result {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 4px 6px 10px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--good) 10%, transparent);
    color: var(--good);
  }

  .file {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: 12px;
  }

  .file .mono {
    color: var(--muted);
  }

  .result button.icon {
    width: 28px;
    height: 28px;
    color: var(--muted);
  }
</style>
