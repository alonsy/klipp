<script lang="ts">
  // The Content Creation Center (Content Creator Mode): the
  // camera that records with every clip, your socials for shorts, and the
  // clips to make shorts from.
  import { onMount } from "svelte";
  import * as api from "../api";
  import type { CameraStatus, Clip, CreatorSettings } from "../api";
  import { clipTitle, duration, when } from "../format";
  import Icon from "../Icon.svelte";
  import { DUR, enter, pageOut, stagger } from "../motion";
  import Reveal from "../settings/Reveal.svelte";
  import { library, openShort, prefs, recorder, setPref, toast, ui } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { drawBadge, PLATFORMS, tileCss, type PlatformId, type Social } from "./socials";

  const TABS = [["clips", "Clips"], ["camera", "Camera"], ["socials", "Socials"]] as const;

  // A new tab comes in from the side it's on, as in Settings.
  let before = TABS.findIndex(([id]) => id === ui.creatorTab);
  let side = $state(0);
  $effect.pre(() => {
    const index = TABS.findIndex(([id]) => id === ui.creatorTab);
    side = Math.sign(index - before);
    before = index;
  });

  let settings = $state<CreatorSettings | null>(null);
  let cameras = $state<string[] | null>(null);
  let camerasError = $state("");
  let status = $state<CameraStatus>({ state: "off" });

  onMount(() => {
    api.creatorSettings().then(
      (s) => (settings = s),
      (e) => toast(`Couldn't read the camera settings: ${e}`),
    );
    loadCameras();
    const poll = () =>
      api.cameraStatus().then(
        (s) => (status = s),
        () => {},
      );
    poll();
    const timer = setInterval(poll, 2000);
    return () => clearInterval(timer);
  });

  async function loadCameras() {
    cameras = null;
    camerasError = "";
    try {
      cameras = await api.cameras();
    } catch (e) {
      cameras = [];
      camerasError = String(e);
    }
  }

  async function change(patch: Partial<CreatorSettings>) {
    if (!settings) return;
    settings = { ...settings, ...patch };
    try {
      await api.setCreatorSettings(settings);
    } catch (e) {
      toast(`Couldn't save that: ${e}`);
    }
  }

  const QUALITIES = [
    ["1080/30", "1080p · 30 fps"],
    ["1080/60", "1080p · 60 fps"],
    ["720/30", "720p · 30 fps"],
    ["720/60", "720p · 60 fps"],
  ] as const;
  let quality = $derived(settings ? `${settings.height <= 720 ? 720 : 1080}/${settings.fps >= 50 ? 60 : 30}` : "1080/30");

  /** The camera picked, or the first one when none is (that's what the recorder uses). */
  let chosen = $derived(settings?.camera || cameras?.[0] || "");

  let statusLine = $derived.by(() => {
    if (!settings?.enabled) return null;
    if (status.state === "recording") {
      const shared = status.shared ? " · shared with another app" : "";
      return { tone: "good", text: `Recording ${status.camera} · ${status.width}×${status.height} · ${Math.round(status.fps ?? 0)} fps${shared}` };
    }
    if (status.state === "starting") return { tone: "", text: `Starting ${status.camera || "the camera"}…` };
    if (status.state === "error") return { tone: "bad", text: status.error || "The camera didn't start." };
    if (!recorder.running) return { tone: "bad", text: "Klipp isn't recording, so the camera is off." };
    return { tone: "", text: "It starts within a few seconds, whenever Klipp records." };
  });

  function social(id: PlatformId): Social {
    return prefs.socials[id] ?? { handle: "", on: false };
  }

  function setSocial(id: PlatformId, patch: Partial<Social>) {
    setPref("socials", { ...prefs.socials, [id]: { ...social(id), ...patch } });
  }

  let badge = $derived(drawBadge(prefs.socials, 0.75)?.toDataURL() ?? null);

  let withCamera = $derived(library.clips.filter((c) => c.camera));
  let shown = $derived((prefs.creatorCameraOnly && withCamera.length > 0 ? withCamera : library.clips).slice(0, 48));

  function thumb(clip: Clip) {
    return api.thumbnail(clip, () => true);
  }
</script>

<section class="center">
  <header>
    <div class="title">
      <h1 title="Your camera recorded with every clip, then any clip made into a vertical short for TikTok, Reels or YouTube Shorts.">Content Creation</h1>
      <span class="tag">Experimental</span>
    </div>
    <Segmented label="Content Creation" value={ui.creatorTab} options={TABS} onchange={(v) => (ui.creatorTab = v)} />
  </header>

  <div class="stack body">
    {#key ui.creatorTab}
      <div class="scroll" in:enter={{ x: side * 14, y: 0, delay: 50, duration: DUR.base }} out:pageOut>
        {#if ui.creatorTab === "clips"}
          <div class="page wide">
            {#if library.clips.length === 0}
              <div class="empty">
                <Icon name="phone" size={22} />
                <p>No clips yet. Save one with <kbd>{recorder.hotkey}</kbd>, then make it a short here.</p>
              </div>
            {:else}
              {#if withCamera.length === 0}
                <p class="note">
                  <span class="grow">No clip has your camera yet. Any clip can still be a short.</span>
                  <button class="ghost set-up" onclick={() => (ui.creatorTab = "camera")}><Icon name="camera" size={15} />Set up the camera</button>
                </p>
              {:else}
                <label class="only">
                  <input type="checkbox" checked={prefs.creatorCameraOnly} onchange={(e) => setPref("creatorCameraOnly", e.currentTarget.checked)} />
                  Only clips with your camera
                </label>
              {/if}
              <div class="grid">
                {#each shown as clip, i (clip.path)}
                  <button class="card" aria-label="Make a short of {clipTitle(clip)}" in:enter={{ delay: stagger(i) }} onclick={() => openShort(clip)}>
                    <span class="thumb">
                      {#await thumb(clip)}
                        <span class="waiting skeleton"></span>
                      {:then url}
                        {#if url}<img src={url} alt="" draggable="false" />{/if}
                      {/await}
                      <span class="shade"></span>
                      <span class="go"><Icon name="phone" size={20} /></span>
                      {#if clip.short}<span class="made">Short</span>{/if}
                      <span class="meta">
                        <span class="line">
                          <span class="name">{clipTitle(clip)}</span>
                          {#if clip.camera}<span class="cam"><Icon name="camera" size={14} /></span>{/if}
                        </span>
                        <span class="sub">{clip.game} · {when(clip.created)}{#if clip.durationSecs != null}&nbsp;· <span class="mono">{duration(clip.durationSecs)}</span>{/if}</span>
                      </span>
                    </span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {:else if ui.creatorTab === "camera"}
          <div class="page">
            <div class="group">
              <Field title="Content Creator Mode" tip="Records your camera next to the screen, as its own track in every clip: hotkey clips and auto-clips." hint="The camera stays on whenever Klipp records.">
                <Toggle checked={settings?.enabled ?? false} disabled={!settings} label="Content Creator Mode" onchange={(v) => change({ enabled: v })} />
              </Field>
              <Reveal open={!!settings?.enabled}>
                <Field title="Camera" hint={camerasError || (cameras && cameras.length === 0 ? "No camera found. Plug one in, or start your phone camera app." : undefined)}>
                  <button class="ghost icon" onclick={loadCameras} title="Look for cameras again" aria-label="Look for cameras again"><Icon name="refresh" size={16} /></button>
                  <select value={chosen} aria-label="Camera" disabled={!cameras || cameras.length === 0} onchange={(e) => change({ camera: e.currentTarget.value })}>
                    {#if !cameras}<option value={chosen}>Looking…</option>{/if}
                    {#each cameras ?? [] as name (name)}<option value={name}>{name}</option>{/each}
                  </select>
                </Field>
                <Field title="Quality" tip="The best your camera offers up to this. Higher costs a little more memory for the replay buffer.">
                  <select
                    value={quality}
                    aria-label="Camera quality"
                    onchange={(e) => {
                      const [h, f] = e.currentTarget.value.split("/").map(Number);
                      change({ height: h, fps: f });
                    }}
                  >
                    {#each QUALITIES as [v, name] (v)}<option value={v}>{name}</option>{/each}
                  </select>
                </Field>
                {#if statusLine}
                  <p class="status {statusLine.tone}">
                    <span class="dot"></span>
                    <span>{statusLine.text}</span>
                  </p>
                {/if}
              </Reveal>
            </div>
          </div>
        {:else}
          <div class="page socials">
            <div class="group">
              {#each PLATFORMS as p (p.id)}
                {@const s = social(p.id)}
                <div class="social" class:off={!s.on}>
                  <span class="logo" style="background: {tileCss(p)}">
                    <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path d={p.path} fill={p.glyph} /></svg>
                  </span>
                  <span class="pname">{p.name}</span>
                  <input
                    value={s.handle}
                    placeholder="@yourname"
                    spellcheck="false"
                    aria-label="{p.name} name"
                    onchange={(e) => setSocial(p.id, { handle: e.currentTarget.value.trim(), on: e.currentTarget.value.trim() ? true : s.on })}
                  />
                  <Toggle checked={s.on} label="Show {p.name} on shorts" onchange={(v) => setSocial(p.id, { on: v })} />
                </div>
              {/each}
            </div>
            {#if badge}
              <div class="preview" in:enter={{ duration: DUR.base }}>
                <h3 class="group-title">On your shorts</h3>
                <div class="board"><img src={badge} alt="Your socials as shorts show them" /></div>
              </div>
            {:else}
              <p class="note">Add a name and it shows on your shorts. The same name on several platforms shares one line.</p>
            {/if}
          </div>
        {/if}
      </div>
    {/key}
  </div>
</section>

<style>
  .center {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    min-height: 36px;
    padding: 0 var(--sp-5) var(--sp-3);
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--fs-lg);
    font-weight: 600;
    letter-spacing: -0.03em;
    white-space: nowrap;
  }

  /* As Field's badge. */
  .tag {
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .body {
    flex: 1;
  }

  .scroll {
    overflow-y: auto;
    scrollbar-gutter: stable;
  }

  .page {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    max-width: calc(680px + 2 * var(--sp-5));
    padding: var(--sp-1) var(--sp-5) var(--sp-6);
  }

  .page.wide {
    max-width: none;
  }

  /* Rows and space, as in Settings. */
  .group {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .group :global(.field) {
    min-height: 44px;
    padding: var(--sp-1) 0;
  }

  .group :global(.field .title) {
    font-weight: 500;
  }

  .group :global(.field select) {
    width: 240px;
  }

  .group-title {
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--faint);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-control);
    line-height: 1.45;
    user-select: text;
  }

  .status .dot {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--faint);
  }

  .status.good {
    color: var(--text);
  }

  .status.good .dot {
    background: var(--good);
  }

  .status.bad {
    color: var(--danger);
  }

  .status.bad .dot {
    background: var(--danger);
  }

  .socials {
    gap: var(--sp-5);
  }

  .social {
    display: grid;
    grid-template-columns: 28px 88px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
  }

  .logo {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    transition: opacity var(--dur) var(--ease-out);
  }

  .social.off .logo {
    opacity: 0.5;
  }

  .pname {
    font-weight: 500;
    transition: color var(--dur) var(--ease-out);
  }

  .social.off .pname {
    color: var(--muted);
  }

  .board {
    display: flex;
    justify-content: flex-start;
    padding: var(--sp-4);
    border-radius: var(--r-md);
    background: repeating-conic-gradient(var(--surface) 0 25%, var(--sunken) 0 50%) 0 0 / 16px 16px;
  }

  .board img {
    display: block;
    max-width: 100%;
  }

  .only {
    display: inline-flex;
    align-items: center;
    align-self: flex-end;
    gap: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-control);
    cursor: pointer;
  }

  .note {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 34px;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .set-up {
    flex-shrink: 0;
    gap: 6px;
    padding: 5px 10px;
    font-size: var(--fs-control);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    padding: 96px var(--sp-5);
    color: var(--muted);
    text-align: center;
  }

  .empty p {
    margin: 0;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: var(--sp-3);
  }

  /* Drawn like a clip in the library: the name on the picture, the rest on hover. */
  .card {
    display: block;
    padding: 0;
    border-radius: var(--r-md);
    background: transparent;
    text-align: left;
  }

  .card:hover {
    background: transparent;
  }

  .card:active {
    transform: none;
  }

  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--surface);
    outline: 2px solid transparent;
    outline-offset: 2px;
    transition:
      transform var(--dur) var(--ease-out),
      outline-color var(--dur-fast);
  }

  .card:hover .thumb {
    transform: translateY(-2px);
  }

  .card:active .thumb {
    transform: scale(0.99);
  }

  .card:focus-visible {
    outline: none;
  }

  .card:focus-visible .thumb {
    outline-color: var(--text);
  }

  .thumb img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--dur-slow) var(--ease-out);
  }

  .card:hover img {
    transform: scale(1.04);
  }

  .waiting {
    position: absolute;
    inset: 0;
    border-radius: 0;
  }

  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, color-mix(in srgb, var(--stage) 72%, transparent), transparent 42%);
  }

  .shade::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, color-mix(in srgb, var(--stage) 82%, transparent), color-mix(in srgb, var(--stage) 18%, transparent) 70%);
    opacity: 0;
    transition: opacity var(--dur);
  }

  .card:hover .shade::after,
  .card:focus-visible .shade::after {
    opacity: 1;
  }

  .go {
    position: absolute;
    left: 50%;
    top: 50%;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin: -26px 0 0 -22px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    opacity: 0;
    transform: scale(0.88);
    transition:
      opacity var(--dur-fast),
      transform var(--dur) var(--ease-out);
  }

  .card:hover .go,
  .card:focus-visible .go {
    opacity: 1;
    transform: none;
  }

  .made {
    position: absolute;
    top: var(--sp-2);
    right: var(--sp-2);
    padding: 2px var(--sp-2);
    border-radius: var(--r-xs);
    background: var(--glass);
    font-size: var(--fs-xs);
    font-weight: 600;
  }

  .meta {
    position: absolute;
    left: var(--sp-3);
    right: var(--sp-3);
    bottom: 9px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    transform: translateY(20px);
    transition: transform var(--dur) var(--ease-out);
  }

  .card:hover .meta,
  .card:focus-visible .meta {
    transform: none;
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 20px;
  }

  .name {
    flex: 1;
    min-width: 0;
    padding-left: 2px;
    margin-left: -2px;
    font-weight: 600;
    font-size: var(--fs-control);
    text-shadow: 0 1px 2px color-mix(in srgb, var(--stage) 60%, transparent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cam {
    display: inline-flex;
    flex-shrink: 0;
    filter: drop-shadow(0 1px 2px color-mix(in srgb, var(--stage) 60%, transparent));
  }

  .sub {
    height: 18px;
    color: var(--muted);
    font-size: var(--fs-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    opacity: 0;
    transition: opacity var(--dur-fast);
  }

  .card:hover .sub,
  .card:focus-visible .sub {
    opacity: 1;
  }

  @media (min-width: 1160px) {
    .socials {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 300px;
      align-items: start;
      gap: var(--sp-6);
      max-width: calc(1000px + 2 * var(--sp-5));
    }
  }
</style>
