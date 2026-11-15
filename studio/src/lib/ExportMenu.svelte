<script lang="ts">
  // The Export button in the editor's top strip, and the options it opens:
  // share (fits a size limit), a lossless trimmed copy, or high quality.
  // While exporting, and when done, the panel shows only that.
  import { onMount } from "svelte";
  import * as api from "./api";
  import type { ExportMode } from "./api";
  import { cloud, refreshCloud } from "./cloud.svelte";
  import { eventName, mapName, size } from "./format";
  import Icon from "./Icon.svelte";
  import { DUR, leave, unfold } from "./motion";
  import { openPage, prefs, reload, setPref, toast } from "./state.svelte";
  import { swallowPress } from "./ui/clicks";
  import Segmented from "./ui/Segmented.svelte";
  import Toggle from "./ui/Toggle.svelte";

  interface Props {
    path: string;
    inPoint: number;
    outPoint: number;
    /** Names of the clip's separate tracks, in the order of `volumes`. */
    tracks: string[];
    /** Which of `tracks` the recorder knows carry voices; empty on older clips. */
    voices: boolean[];
    volumes: number[];
    mixVolume: number;
    /** The speed it plays at in the editor. */
    speed: number;
    /** The clip itself: what a link on klipp.dev says about it (game, moment, map). */
    clip?: api.Clip;
  }
  let { path, inPoint, outPoint, tracks, voices, volumes, mixVolume, speed, clip }: Props = $props();

  /** Off exports at 1×, for a clip only slowed down to look at. */
  let keepSpeed = $state(true);

  /** Tracks that carry voices: what the recorder knows from the device, or a name that says so. */
  const VOICE = /chat|mic|voice|discord|party|team/i;
  let voiceTracks = $derived(tracks.map((t, i) => voices[i] || VOICE.test(t)));
  let voiceNames = $derived(tracks.filter((_, i) => voiceTracks[i]));
  /** "Chat and Mic" */
  let voiceList = $derived(voiceNames.length > 1 ? `${voiceNames.slice(0, -1).join(", ")} and ${voiceNames.at(-1)}` : (voiceNames[0] ?? ""));
  /** Levels for a re-encode: voice tracks muted when asked to. */
  let outVolumes = $derived(prefs.exportWithoutVoices ? volumes.map((v, i) => (voiceTracks[i] ? 0 : v)) : volumes);

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
  let result = $state<{ path: string; sizeBytes: number; mode: ExportMode; copied: boolean } | null>(null);
  /** The button says Done for a moment, for when the panel is closed. */
  let justDone = $state(false);
  let doneTimer = 0;

  // "Share as a link": the share export, then that file goes to the person's account on klipp.dev.
  /** The export running now goes on to klipp.dev when it's done. */
  let linkAfter = $state(false);
  let upload = $state<{ id: number; fraction: number } | null>(null);
  let shared = $state<{ url: string; copied: boolean } | null>(null);
  /** How long the exported file plays (the plan limits it), from the last export's range and speed. */
  let exportedSecs = 0;
  /** The export for this link starts with the Klipp tab (Free accounts). */
  let marked = false;
  /** Free accounts' links start with the Klipp tab; said before they press. */
  let linkHint = $derived(
    `Puts it on your Klipp account, under the size limit above, and copies a link that plays right in Discord${cloud.status?.account?.plan.mark ? ". On the Free plan the video starts with the Klipp tab for a moment" : ""}`,
  );
  /** Whatever is in progress: the export, then the upload. */
  let working = $derived(busy ?? upload);
  let percent = $derived(working ? Math.round(working.fraction * 100) : 0);

  // Opening the panel looks up who is signed in (nobody: no request), so the link's hint can say what the plan adds.
  $effect(() => {
    if (open && !cloud.status && !cloud.loading) refreshCloud();
  });

  /** "Ace on Mirage": the moment and the map, when the clip has them. */
  function linkTitle(c: api.Clip): string {
    const what = c.label ? eventName(c.label, c.game) : "";
    const where = c.map ? mapName(c.map) : "";
    return what && where ? `${what} on ${where}` : what;
  }

  /** The moments inside the exported range, the clip's own first. */
  function linkTags(c: api.Clip): string[] {
    const inside = c.events.filter((e) => e.atSecs >= inPoint - 0.5 && e.atSecs <= outPoint + 0.5).map((e) => e.kind);
    return [...new Set([...(c.label ? [c.label] : []), ...inside])].slice(0, 8);
  }

  async function shareLink() {
    if (busy || upload) return;
    // Asked again each time: the plan decides whether the video starts with the Klipp tab.
    const status = await refreshCloud();
    if (!status?.signedIn) {
      open = false;
      toast("Sign in to Klipp to share a clip as a link.", { label: "Sign in", run: () => openPage("settings", "account") }, 9000);
      return;
    }
    shared = null;
    marked = !!status.account?.plan.mark;
    await start("share", true);
  }

  async function sendToKlipp(file: string) {
    try {
      const id = await api.cloudUpload({
        path: file,
        title: clip ? linkTitle(clip) : "",
        game: clip?.game || null,
        tags: clip ? linkTags(clip) : [],
        map: clip?.map || null,
        visibility: "unlisted",
        durationSecs: exportedSecs > 0 ? exportedSecs : null,
        marked,
      });
      upload = { id, fraction: 0 };
    } catch (e) {
      toast(`Couldn't upload: ${e}`);
    }
  }

  onMount(() => {
    let off = () => {};
    let offUpload = () => {};
    api
      .onCloudUpload({
        progress: (id, fraction) => {
          if (upload?.id === id) upload.fraction = fraction;
        },
        done: async (id, url) => {
          if (upload?.id !== id) return;
          upload = null;
          const copied = await api.copyText(url);
          shared = { url, copied };
          justDone = true;
          clearTimeout(doneTimer);
          doneTimer = window.setTimeout(() => (justDone = false), 2600);
          toast(copied ? "Link copied. Paste it anywhere with Ctrl+V." : "Your link is ready.", { label: "Open", run: () => api.openFile(url) }, 8000);
          refreshCloud();
        },
        failed: (id, error, why) => {
          if (upload?.id !== id) return;
          upload = null;
          if (why.cancelled) return;
          if (why.signedOut) {
            refreshCloud();
            toast(`Couldn't upload: ${error}`, { label: "Sign in", run: () => openPage("settings", "account") }, 9000);
          } else toast(`Couldn't upload: ${error}`, undefined, 9000);
        },
      })
      .then((unlisten) => (offUpload = unlisten));
    api
      .onExport({
        progress: (id, fraction) => {
          if (busy?.id === id) busy.fraction = fraction;
        },
        done: async (id, out, sizeBytes) => {
          if (busy?.id !== id) return;
          const mode = busy.mode;
          busy = null;
          result = { path: out, sizeBytes, mode, copied: false };
          if (linkAfter) {
            // The file was only a step: it goes on to klipp.dev, and the clipboard waits for the link.
            linkAfter = false;
            await sendToKlipp(out);
            return;
          }
          justDone = true;
          clearTimeout(doneTimer);
          doneTimer = window.setTimeout(() => (justDone = false), 2600);
          if (mode === "trim") reload();
          const what = { share: `Ready to share (${size(sizeBytes)})`, trim: "Trimmed copy saved to your library", quality: `Exported (${size(sizeBytes)})` }[mode];
          const copyNow = () => api.copyToClipboard(out).then(() => toast("Copied. Paste it with Ctrl+V."), (e) => toast(`Couldn't copy: ${e}`));
          if (prefs.copyAfterExport) {
            await api.copyToClipboard(out).then(
              () => {
                if (result?.path === out) result.copied = true;
                toast(`${what} and copied. Paste it with Ctrl+V.`);
              },
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
          linkAfter = false;
          if (!/cancel/i.test(error)) toast(`Export failed: ${error}`);
        },
      })
      .then((unlisten) => (off = unlisten));
    return () => {
      off();
      offUpload();
      clearTimeout(doneTimer);
    };
  });

  async function start(mode: ExportMode, thenLink = false) {
    if (busy || upload) return;
    result = null;
    shared = null;
    try {
      localStorage.setItem(TARGET_KEY, String(targetMb));
    } catch {
      // Not important.
    }
    const outSpeed = keepSpeed && mode !== "trim" ? speed : 1;
    exportedSecs = (outPoint - inPoint) / outSpeed;
    linkAfter = thenLink;
    if (!thenLink) marked = false;
    const id = await api.startExport({ path, start: inPoint, end: outPoint, mode, targetMb, volumes: outVolumes, mixVolume, speed: outSpeed, mark: marked });
    busy = { id, mode, fraction: 0 };
  }

  function outside(e: PointerEvent) {
    if (!open || root.contains(e.target as Node)) return;
    open = false;
    swallowPress(e);
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
  const doneNames: Record<ExportMode, string> = { share: "Ready to share", trim: "Trimmed copy saved", quality: "Exported" };
  const targets = [
    [10, "10 MB"],
    [50, "50 MB"],
    [500, "500 MB"],
  ] as const;
</script>

<svelte:window onpointerdowncapture={outside} onkeydowncapture={key} />

<div class="export" bind:this={root}>
  <button
    class="primary trigger"
    class:busy={!!working}
    class:quiet={open && !working}
    aria-expanded={open}
    title="Export or share this clip (E)"
    onclick={() => (open = !open)}
  >
    {#if working}
      <span class="veil" style="transform: scaleX({1 - (working?.fraction ?? 0)})"></span>
      <span class="face mono">{percent}%</span>
    {:else if justDone}
      <span class="face done-face"><Icon name="check" size={15} />Done</span>
    {:else}
      <span class="face"><Icon name="send" size={15} />Export</span>
    {/if}
  </button>

  {#if open}
    <div class="panel" role="dialog" aria-label="Export" in:unfold out:leave={{ y: -4 }}>
      <div class="options" class:covered={working || result || shared} inert={!!(working || result || shared)}>
        <div class="share">
          <Segmented value={targetMb} options={targets} label="Size limit" onchange={(v) => (targetMb = v)} />
          <button class="primary big" disabled={!!working} title="Fits under the size limit, so chat apps take it as is" onclick={() => start("share")}><Icon name="send" size={16} />Share</button>
        </div>

        <div class="others">
          <button class="ghost" disabled={!!working} onclick={shareLink} title={linkHint}>
            <Icon name="link" size={16} />Share as a link
          </button>
          <button class="ghost" disabled={!!working} onclick={() => start("trim")} title="Same quality, every audio track kept, saved next to the original">
            <Icon name="scissors" size={16} />Save trimmed copy
          </button>
          <button class="ghost" disabled={!!working} onclick={() => start("quality")} title="High quality H.264 with your track levels, plays everywhere">
            <Icon name="export" size={16} />Export high quality
          </button>
        </div>

        <div class="toggles">
          {#if speed !== 1}
            <div class="copy">
              <div class="copy-text">
                <span class="copy-title">Export at {speed}×</span>
                <span class="copy-hint">A trimmed copy stays at 1×.</span>
              </div>
              <Toggle checked={keepSpeed} label="Export at {speed}×" onchange={(v) => (keepSpeed = v)} />
            </div>
          {/if}

          {#if voiceNames.length > 0}
            <div class="copy">
              <div class="copy-text">
                <span class="copy-title">Without voices</span>
                <span class="copy-hint">
                  {prefs.exportWithoutVoices ? `Leaves ${voiceList} out. A trimmed copy keeps every track.` : `${voiceList} ${voiceNames.length > 1 ? "stay" : "stays"} in.`}
                </span>
              </div>
              <Toggle checked={prefs.exportWithoutVoices} label="Without voices" onchange={(v) => setPref("exportWithoutVoices", v)} />
            </div>
          {/if}

          <div class="copy" title={prefs.copyAfterExport ? "Replaces what you have copied now, ready for Ctrl+V" : "Your clipboard is left alone"}>
            <div class="copy-text">
              <span class="copy-title">Copy when done</span>
            </div>
            <Toggle checked={prefs.copyAfterExport} label="Copy to clipboard when done" onchange={(v) => setPref("copyAfterExport", v)} />
          </div>
        </div>
      </div>

      {#if busy}
        <div class="status" in:unfold={{ duration: DUR.slow }} out:leave>
          <div class="ring-box">
            <svg class="ring" viewBox="0 0 100 100" aria-hidden="true">
              <circle class="ring-track" cx="50" cy="50" r="44" pathLength="100" />
              <circle class="ring-fill" cx="50" cy="50" r="44" pathLength="100" style="stroke-dashoffset: {100 - busy.fraction * 100}" />
            </svg>
            <!-- Keeps turning when an encoder sits on one percent for a while. -->
            <svg class="ring comet" viewBox="0 0 100 100" aria-hidden="true"><circle cx="50" cy="50" r="44" pathLength="100" /></svg>
            <span class="ring-num mono">{percent}%</span>
          </div>
          <span class="s-title">{linkAfter ? "Getting it ready" : names[busy.mode]}…</span>
          <button class="ghost cancel" onclick={() => busy && api.cancelExport(busy.id)}>Cancel</button>
        </div>
      {:else if upload}
        <div class="status" in:unfold={{ duration: DUR.slow }} out:leave>
          <div class="ring-box">
            <svg class="ring" viewBox="0 0 100 100" aria-hidden="true">
              <circle class="ring-track" cx="50" cy="50" r="44" pathLength="100" />
              <circle class="ring-fill" cx="50" cy="50" r="44" pathLength="100" style="stroke-dashoffset: {100 - upload.fraction * 100}" />
            </svg>
            <!-- Keeps turning while klipp.dev checks the file at the end. -->
            <svg class="ring comet" viewBox="0 0 100 100" aria-hidden="true"><circle cx="50" cy="50" r="44" pathLength="100" /></svg>
            <span class="ring-num mono">{percent}%</span>
          </div>
          <span class="s-title">{upload.fraction >= 1 ? "Almost there" : "Uploading to klipp.dev"}…</span>
          <button class="ghost cancel" onclick={() => upload && api.cloudCancelUpload(upload.id)}>Cancel</button>
        </div>
      {:else if shared}
        <div class="status" in:unfold={{ duration: DUR.slow }} out:leave>
          <button class="ghost icon again" onclick={() => (shared = null)} aria-label="Back to the exported file" title="Back"><Icon name="back" size={16} /></button>
          <div class="mark">
            <span class="burst"></span>
            <span class="tick"><Icon name="check" size={30} /></span>
          </div>
          <span class="s-title">{shared.copied ? "Link copied" : "Your link is ready"}</span>
          <span class="file mono" title={shared.url}>{shared.url.replace(/^https?:\/\//, "")}</span>
          <span class="pasted">Only people with the link can watch it.</span>
          <div class="after">
            <button onclick={() => shared && api.copyText(shared.url).then((ok) => toast(ok ? "Link copied. Paste it anywhere with Ctrl+V." : "Couldn't copy the link."))}><Icon name="copy" size={15} />Copy link</button>
            <button onclick={() => shared && api.openFile(shared.url)}><Icon name="link" size={15} />Open</button>
          </div>
        </div>
      {:else if result}
        <div class="status" in:unfold={{ duration: DUR.slow }} out:leave>
          <button class="ghost icon again" onclick={() => (result = null)} aria-label="Back to export options" title="Back"><Icon name="back" size={16} /></button>
          <div class="mark">
            <span class="burst"></span>
            <span class="tick"><Icon name="check" size={30} /></span>
          </div>
          <span class="s-title">{doneNames[result.mode]}</span>
          <span class="file" title={result.path}>{result.path.split("\\").pop()} <span class="mono">· {size(result.sizeBytes)}</span></span>
          {#if result.copied}<span class="pasted">Copied. Paste it with Ctrl+V.</span>{/if}
          <div class="after">
            <button onclick={() => result && api.copyToClipboard(result.path).then(() => toast("Copied. Paste it anywhere with Ctrl+V."))}><Icon name="copy" size={15} />Copy</button>
            <button onclick={() => result && api.reveal(result.path)}><Icon name="folder" size={15} />Show in folder</button>
          </div>
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
    position: relative;
    min-width: 96px;
    height: 32px;
    padding: 0 var(--sp-4);
    border: none;
    overflow: hidden;
  }

  /* The accent passes into the panel while it's open: one primary at a time. */
  .trigger.quiet {
    background: var(--surface-3);
    color: var(--text);
  }

  .face {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    animation: fade var(--dur) var(--ease-out);
  }

  .done-face {
    animation: rise var(--dur-spring) var(--spring);
  }

  /* While exporting, a veil draws back from the left and the button lights up with it. */
  .veil {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--on-accent) 30%, transparent);
    transform-origin: right;
    transition: transform var(--dur) linear;
  }

  .trigger.busy::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(100deg, transparent 30%, color-mix(in srgb, var(--text) 26%, transparent) 50%, transparent 70%);
    transform: translateX(-100%);
    animation: sheen 1.6s linear infinite;
  }

  @keyframes sheen {
    to {
      transform: translateX(100%);
    }
  }

  .panel {
    position: absolute;
    top: calc(100% + var(--sp-2));
    right: 0;
    z-index: 30;
    width: 296px;
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--shadow-3);
    transform-origin: top right;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: var(--sp-3);
    transition:
      opacity var(--dur-fast) var(--ease-out),
      transform var(--dur) var(--ease-out);
  }

  /* Still there, keeping the panel's size, so nothing jumps when the status takes over. */
  .options.covered {
    opacity: 0;
    transform: scale(0.97);
  }

  .share {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .share :global(.segmented) {
    display: grid;
  }

  .big {
    width: 100%;
    height: 38px;
  }

  .others {
    display: flex;
    flex-direction: column;
  }

  .others button {
    justify-content: flex-start;
    gap: var(--sp-3);
    padding: 7px var(--sp-2);
  }

  .toggles {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-2) var(--sp-1);
    background: linear-gradient(var(--line), var(--line)) top / 100% 1px no-repeat;
  }

  .copy {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .copy-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-width: 0;
  }

  .copy-title {
    font-size: var(--fs-control);
    font-weight: 550;
  }

  .copy-hint {
    font-size: var(--fs-xs);
    line-height: 1.4;
    color: var(--muted);
  }

  .status {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    padding: var(--sp-4);
    text-align: center;
    transform-origin: center;
  }

  .ring-box {
    position: relative;
    display: grid;
    place-items: center;
    width: 104px;
    height: 104px;
    margin-bottom: var(--sp-2);
  }

  .ring {
    position: absolute;
    inset: 0;
    transform: rotate(-90deg);
    fill: none;
    stroke-width: 5;
    stroke-linecap: round;
  }

  .ring-track {
    stroke: var(--surface-3);
  }

  .ring-fill {
    stroke: var(--accent);
    stroke-dasharray: 100;
    transition: stroke-dashoffset var(--dur) linear;
  }

  .comet {
    animation: orbit 1.8s linear infinite;
  }

  .comet circle {
    stroke: var(--text);
    stroke-opacity: 0.55;
    stroke-width: 2;
    stroke-dasharray: 5 95;
  }

  @keyframes orbit {
    to {
      transform: rotate(270deg);
    }
  }

  .ring-num {
    font-size: var(--fs-lg);
    font-weight: 600;
  }

  .s-title {
    font-size: var(--fs-base);
    font-weight: 600;
  }

  .cancel {
    height: 28px;
    padding: 0 var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .again {
    position: absolute;
    top: var(--sp-2);
    left: var(--sp-2);
    width: 28px;
    height: 28px;
    color: var(--muted);
  }

  /* The check lands with a small spring, and a ring leaves it. */
  .mark {
    position: relative;
    width: 64px;
    height: 64px;
    margin-bottom: var(--sp-2);
  }

  .tick,
  .burst {
    position: absolute;
    inset: 0;
    border-radius: 50%;
  }

  .tick {
    display: grid;
    place-items: center;
    background: var(--good);
    color: var(--on-accent);
    animation: land var(--dur-spring) var(--spring) both;
  }

  .burst {
    background: var(--good-soft);
    animation: burst calc(var(--dur-spring) * 1.5) var(--ease-out) both;
  }

  @keyframes land {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @keyframes burst {
    from {
      opacity: 1;
      transform: scale(0.8);
    }
    to {
      opacity: 0;
      transform: scale(2.2);
    }
  }

  .file {
    max-width: 100%;
    color: var(--muted);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file .mono {
    font-size: var(--fs-xs);
  }

  .pasted {
    color: var(--muted);
    font-size: var(--fs-xs);
  }

  .after {
    display: flex;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }

  .after button {
    gap: 6px;
    padding: 6px var(--sp-3);
    font-size: var(--fs-control);
  }
</style>
