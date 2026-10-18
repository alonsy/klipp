<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, untrack } from "svelte";
  import * as api from "./api";
  import type { Clip, Edit } from "./api";
  import { mixPeaks, TrackMixer, type Peaks } from "./audio";
  import ExportMenu from "./ExportMenu.svelte";
  import { clipTitle, eventName, isMultiKill, killSummary, mapName, modeName, timecode, weaponName, when } from "./format";
  import Icon from "./Icon.svelte";
  import { isHot, mergeMarkers } from "./markers";
  import { deleteClips, prefs, setFavorite, setPref, toast, ui } from "./state.svelte";
  import Timeline, { type Lane } from "./Timeline.svelte";
  import Slider from "./ui/Slider.svelte";
  import WindowControls from "./WindowControls.svelte";

  let { clip }: { clip: Clip } = $props();

  // App recreates the editor for each clip ({#key}), so the starting values are all it needs.
  const initial = untrack(() => ({ path: clip.path, duration: clip.durationSecs ?? 0, tracks: [...clip.tracks], edit: clip.edit ?? {} }));
  /** Track 0 is the premix ("Mix"); the others can be leveled one by one. */
  const separate = initial.tracks.length > 1 ? initial.tracks.slice(1) : [];
  const saved: Edit = initial.edit;
  const FRAME = 1 / 60;
  /** "Set end" plays this much before the new end, to hear and see the cut. */
  const END_PREVIEW = 2;

  let video: HTMLVideoElement;
  let stage: HTMLDivElement;
  let length = $state(initial.duration);
  let inPoint = $state(saved.in ?? 0);
  let outPoint = $state(saved.out ?? initial.duration);
  let current = $state(saved.in ?? 0);
  let playing = $state(false);
  let loop = $state(prefs.loop);
  let volumes = $state(separate.map((_, i) => saved.volumes?.[i] ?? 1));
  let muted = $state(separate.map((_, i) => saved.muted?.[i] ?? false));
  let solo = $state(separate.map(() => false));
  let master = $state(saved.master ?? 1);
  let masterMuted = $state(false);
  let flash = $state<"play" | "pause" | null>(null);
  let savedNote = $state(false);
  /** How loud you hear it here. Separate from the clip's own levels, never exported. */
  let previewVolume = $state(prefs.previewVolume);
  let previewMuted = $state(false);
  let previewLevel = $derived(previewMuted ? 0 : previewVolume);
  let volumeIcon = $derived(previewLevel === 0 ? "mute" : previewLevel < 0.5 ? "volumeLow" : "volume");
  // Fullscreen shows its own small controls, hidden while the mouse rests.
  let isFullscreen = $state(false);
  let fsIdle = $state(false);
  let fsHover = $state<number | null>(null);
  let fsBar = $state<HTMLDivElement | null>(null);
  let fsDragging = false;
  let fsTimer = 0;

  let mixer: TrackMixer | null = null;
  let audioReady = $state(false);
  let noAudio = $state(false);
  let peaks = $state<Peaks[]>([]);

  let anySolo = $derived(solo.some(Boolean));
  let effective = $derived(volumes.map((v, i) => (muted[i] || (anySolo && !solo[i]) ? 0 : v)));
  let masterLevel = $derived(masterMuted ? 0 : master);
  /** Levels the export uses: each track with the Mix level applied. */
  let exportVolumes = $derived(effective.map((v) => v * masterLevel));

  let lanes = $derived.by<Lane[]>(() => {
    const mix: Lane = {
      name: "Mix",
      peaks: !audioReady ? null : separate.length > 0 ? mixPeaks(peaks, effective) : (peaks[0] ?? null),
      gain: masterLevel,
      volume: master,
      muted: masterMuted,
      solo: false,
      off: masterMuted,
      mix: true,
    };
    return [
      mix,
      ...separate.map((name, i) => ({
        name,
        peaks: audioReady ? (peaks[i] ?? null) : null,
        gain: effective[i],
        volume: volumes[i],
        muted: muted[i],
        solo: solo[i],
        off: effective[i] === 0 || masterMuted,
        mix: false,
      })),
    ];
  });

  onMount(() => {
    let alive = true;
    const m = new TrackMixer();
    m.load(initial.path)
      .then((ok) => {
        if (!alive) return m.close();
        if (!ok) {
          // No audio we can mix: let the video play its own.
          noAudio = true;
          m.close();
          video.muted = false;
          return;
        }
        mixer = m;
        peaks = m.peaks;
        audioReady = true;
        applyLevels();
        if (!video.paused) m.play(video.currentTime);
      })
      .catch(() => {
        m.close();
        noAudio = true;
        if (alive) video.muted = false;
      });
    let frame = 0;
    const tick = () => {
      if (video && !video.paused) {
        current = video.currentTime;
        if (current >= outPoint - 0.01) {
          if (loop) {
            seek(inPoint);
          } else {
            video.pause();
            seek(outPoint);
          }
        }
        mixer?.follow(video.currentTime);
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    const onFullscreen = () => {
      isFullscreen = document.fullscreenElement === stage;
      // The page going fullscreen only fills the window; take the whole screen too.
      getCurrentWindow().setFullscreen(isFullscreen).catch(() => {});
      wake();
    };
    document.addEventListener("fullscreenchange", onFullscreen);
    return () => {
      alive = false;
      cancelAnimationFrame(frame);
      document.removeEventListener("fullscreenchange", onFullscreen);
      clearTimeout(fsTimer);
      flushSave();
      mixer?.close();
      mixer = null;
    };
  });

  function applyLevels() {
    if (!mixer) return;
    if (mixer.separate) effective.forEach((v, i) => mixer!.setVolume(i, v));
    else mixer.setVolume(0, 1);
    mixer.setMaster(masterLevel);
    mixer.setMonitor(previewLevel);
  }

  $effect(() => {
    // Re-run when any level changes.
    void effective;
    void masterLevel;
    void previewLevel;
    if (audioReady) applyLevels();
  });

  $effect(() => {
    if (video && noAudio) video.volume = Math.min(1, masterLevel * previewLevel);
  });

  function loaded() {
    length = video.duration;
    // Saved trim points from an older file length stay inside the clip.
    outPoint = Math.min(saved.out ?? length, length);
    inPoint = Math.min(inPoint, Math.max(0, outPoint - 0.5));
    seek(inPoint);
    if (prefs.autoplay) video.play().catch(() => {});
  }

  function play(from?: number) {
    if (from != null) seek(from);
    else if (video.currentTime >= outPoint - 0.05 || video.currentTime < inPoint - 0.05) seek(inPoint);
    video.play().catch(() => {});
  }

  function toggle() {
    if (video.paused) play();
    else video.pause();
  }

  function clickVideo() {
    toggle();
    flash = video.paused ? "pause" : "play";
    setTimeout(() => (flash = null), 450);
  }

  function seek(t: number) {
    video.currentTime = Math.min(Math.max(t, 0), length || t);
    current = video.currentTime;
  }

  function step(frames: number) {
    video.pause();
    seek(current + frames * FRAME);
  }

  // Edits are remembered per clip, a moment after the last change.
  let dirty = false;
  let saveTimer = 0;

  function changed() {
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = window.setTimeout(flushSave, 600);
  }

  function flushSave() {
    clearTimeout(saveTimer);
    if (!dirty) return;
    dirty = false;
    const r = (v: number) => Math.round(v * 1000) / 1000;
    const edit: Edit = { in: r(inPoint), out: r(outPoint), volumes: volumes.map(r), muted: [...muted], master: r(master) };
    const untouched = inPoint <= 0.01 && outPoint >= length - 0.01 && volumes.every((v) => v === 1) && !muted.some(Boolean) && master === 1;
    api
      .saveEdit(initial.path, untouched ? null : edit)
      .then(() => {
        clip.edit = untouched ? null : edit;
        savedNote = true;
        setTimeout(() => (savedNote = false), 1400);
      })
      .catch((e) => toast(`Couldn't save the edit: ${e}`));
  }

  function setRange(a: number, b: number) {
    inPoint = Math.max(0, a);
    outPoint = Math.min(length, b);
    changed();
  }

  /** Start here. Playback jumps to the new start and carries on from there. */
  function setStart() {
    const wasPlaying = !video.paused;
    setRange(Math.min(current, outPoint - 0.5), outPoint);
    seek(inPoint);
    if (wasPlaying) play(inPoint);
  }

  /** End here. While playing, replays the last moments up to the new end. */
  function setEnd() {
    const wasPlaying = !video.paused;
    setRange(inPoint, Math.max(current, inPoint + 0.5));
    if (wasPlaying) play(Math.max(inPoint, outPoint - END_PREVIEW));
  }

  function resetRange() {
    setRange(0, length);
  }

  // Scrubbing pauses; letting go resumes, from the new start or near the new end.
  let resumeAfterDrag = false;

  function onDrag(what: "seek" | "in" | "out", phase: "start" | "end") {
    if (phase === "start") {
      resumeAfterDrag = !video.paused;
      video.pause();
      return;
    }
    if (what === "in") seek(inPoint);
    if (resumeAfterDrag) {
      if (what === "in") play(inPoint);
      else if (what === "out") play(Math.max(inPoint, outPoint - END_PREVIEW));
      else play(current);
    }
  }

  function onRange(a: number, b: number) {
    const movedIn = a !== inPoint;
    setRange(a, b);
    // Show the frame at the handle being dragged.
    seek(movedIn ? inPoint : outPoint);
  }

  function jumpMarker(dir: 1 | -1) {
    const times = clip.events.map((e) => e.atSecs).sort((a, b) => a - b);
    const t = dir > 0 ? times.find((t) => t > current + 0.05) : [...times].reverse().find((t) => t < current - 1.05);
    if (t != null) seek(Math.max(0, t - 1));
  }

  function setVolume(lane: number, v: number) {
    if (lane < 0) master = v;
    else volumes[lane] = v;
    changed();
  }

  function toggleMute(lane: number) {
    if (lane < 0) masterMuted = !masterMuted;
    else {
      muted[lane] = !muted[lane];
      changed();
    }
  }

  function toggleSolo(lane: number) {
    solo[lane] = !solo[lane];
  }

  function toggleLoop() {
    loop = !loop;
    setPref("loop", loop);
  }

  function close() {
    ui.clip = null;
  }

  function fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else stage.requestFullscreen().catch(() => {});
  }

  /** Shows the fullscreen controls for a moment. */
  function wake() {
    fsIdle = false;
    clearTimeout(fsTimer);
    fsTimer = window.setTimeout(() => (fsIdle = true), 2500);
  }

  const pct = (t: number) => (length > 0 ? (Math.min(Math.max(t, 0), length) / length) * 100 : 0);

  function fsTime(e: PointerEvent) {
    const r = fsBar!.getBoundingClientRect();
    return (Math.min(Math.max((e.clientX - r.left) / r.width, 0), 1)) * length;
  }

  function fsDown(e: PointerEvent) {
    fsDragging = true;
    fsBar!.setPointerCapture(e.pointerId);
    seek(fsTime(e));
  }

  function fsMove(e: PointerEvent) {
    fsHover = fsTime(e);
    wake();
    if (fsDragging) seek(fsTime(e));
  }

  function fsUp(e: PointerEvent) {
    fsDragging = false;
    if (fsBar?.hasPointerCapture(e.pointerId)) fsBar.releasePointerCapture(e.pointerId);
  }

  function setPreview(v: number) {
    previewVolume = v;
    if (v > 0) previewMuted = false;
  }

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" && (target as HTMLInputElement).type !== "range") return;
    if (target.tagName === "SELECT" || ui.closeNotice || e.ctrlKey || e.altKey) return;
    const big = e.shiftKey ? 5 : 1;
    switch (e.key) {
      case " ":
      case "k":
      case "K":
        e.preventDefault();
        toggle();
        break;
      case "i":
      case "I":
        setStart();
        break;
      case "o":
      case "O":
        setEnd();
        break;
      case "l":
      case "L":
        toggleLoop();
        break;
      case "ArrowLeft":
        e.preventDefault();
        seek(current - big);
        break;
      case "ArrowRight":
        e.preventDefault();
        seek(current + big);
        break;
      case ",":
        step(-1);
        break;
      case ".":
        step(1);
        break;
      case "[":
        jumpMarker(-1);
        break;
      case "]":
        jumpMarker(1);
        break;
      case "Home":
        seek(inPoint);
        break;
      case "End":
        seek(outPoint);
        break;
      case "f":
      case "F":
        fullscreen();
        break;
      case "m":
      case "M":
        previewMuted = !previewMuted;
        break;
      case "Escape":
        if (!document.fullscreenElement) close();
        break;
    }
  }

  let subtitle = $derived(
    [clip.map ? mapName(clip.map) : clip.game, clip.mode ? modeName(clip.mode) : "", when(clip.created)].filter(Boolean).join(" · "),
  );
  let badge = $derived(clip.label ?? (clip.kind === "auto" ? "kill" : "manual"));
  let markers = $derived(mergeMarkers(clip.events));
  let moments = $derived([...clip.events].filter((e) => e.kind !== "kill" || !clip.events.some((o) => o !== e && Math.abs(o.atSecs - e.atSecs) < 0.3 && o.kind !== "kill")).sort((a, b) => a.atSecs - b.atSecs));
</script>

<svelte:window onkeydown={onKey} />

<section class="editor">
  <!-- Also the title bar: empty space drags the window. -->
  <header data-tauri-drag-region>
    <button class="ghost icon" onclick={close} title="Back to clips (Esc)"><Icon name="back" size={20} /></button>
    <span class="badge" class:hot={isMultiKill(badge)}><Icon event={badge} size={22} /></span>
    <div class="title" data-tauri-drag-region>
      <h1>{clipTitle(clip)}</h1>
      <span class="sub">{subtitle}</span>
    </div>
    <span class="saved label" class:show={savedNote}><Icon name="check" size={13} /> Edit saved</span>
    <button class="ghost icon" class:fav={clip.favorite} onclick={() => setFavorite([clip.path], !clip.favorite)} title={clip.favorite ? "Remove from favorites" : "Add to favorites"}>
      <Icon name="star" size={19} />
    </button>
    <button class="ghost icon" onclick={() => api.reveal(clip.path)} title="Show in folder"><Icon name="folder" size={19} /></button>
    <button class="ghost icon danger" onclick={() => deleteClips([clip.path])} title="Delete"><Icon name="trash" size={19} /></button>
    <span class="divider"></span>
    <WindowControls />
  </header>

  <div class="body">
    <div class="stage" class:fs={isFullscreen} class:idle={isFullscreen && fsIdle && playing} bind:this={stage} onpointermove={wake} role="presentation">
      <!-- svelte-ignore a11y_media_has_caption, a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <video
        bind:this={video}
        src={api.fileUrl(clip.path)}
        preload="auto"
        muted
        onloadedmetadata={loaded}
        onplay={() => {
          playing = true;
          mixer?.play(video.currentTime);
        }}
        onpause={() => {
          playing = false;
          mixer?.pause();
        }}
        onseeked={() => {
          current = video.currentTime;
          if (!video.paused) mixer?.play(video.currentTime);
        }}
        onclick={clickVideo}
        ondblclick={fullscreen}
      ></video>
      {#if flash}
        <div class="flash"><Icon name={flash} size={30} /></div>
      {/if}
      {#if isFullscreen}
        <div class="fs-controls">
          <div class="fs-markers">
            {#each markers as m (m.at)}
              <button
                class="fs-marker"
                class:hot={isHot(m.kind)}
                style="left: {pct(m.at)}%"
                title="{timecode(m.at)} · {m.all.join(', ')}"
                onclick={() => seek(Math.max(0, m.at - 1))}
              >
                <Icon event={m.kind} size={18} />{#if isHot(m.kind)}<span class="mono">{eventName(m.kind)}</span>{/if}
              </button>
            {/each}
          </div>
          <div
            class="fs-bar"
            bind:this={fsBar}
            onpointerdown={fsDown}
            onpointermove={fsMove}
            onpointerup={fsUp}
            onpointercancel={fsUp}
            onpointerleave={() => (fsHover = null)}
            role="slider"
            aria-label="Position"
            aria-valuemin={0}
            aria-valuemax={length}
            aria-valuenow={current}
            tabindex="-1"
          >
            <div class="fs-track">
              <div class="fs-trim" style="left: {pct(inPoint)}%; width: {pct(outPoint) - pct(inPoint)}%"></div>
              <div class="fs-fill" style="width: {pct(current)}%"></div>
              {#each markers as m (m.at)}<span class="fs-tick" style="left: {pct(m.at)}%"></span>{/each}
            </div>
            <span class="fs-knob" style="left: {pct(current)}%"></span>
            {#if fsHover != null}<span class="fs-hover mono" style="left: {pct(fsHover)}%">{timecode(fsHover)}</span>{/if}
          </div>
          <div class="fs-row">
            <button class="fs-btn" onclick={toggle} aria-label={playing ? "Pause" : "Play"}><Icon name={playing ? "pause" : "play"} size={24} /></button>
            <span class="fs-time mono">{timecode(current)} <span>/ {timecode(length)}</span></span>
            <div class="fs-volume">
              <button class="fs-btn" onclick={() => (previewMuted = !previewMuted)} aria-label="Mute preview"><Icon name={volumeIcon} size={21} /></button>
              <Slider min={0} max={1} step={0.02} value={previewLevel} label="Preview volume" oninput={setPreview} onchange={(v) => setPref("previewVolume", v)} />
            </div>
            <span class="fs-spacer"></span>
            <span class="fs-title">{clipTitle(clip)}</span>
            <button class="fs-btn" onclick={fullscreen} aria-label="Exit fullscreen" title="Exit fullscreen (F)"><Icon name="exitFullscreen" size={21} /></button>
          </div>
        </div>
      {/if}
    </div>

    <aside class="side">
      <div class="moments">
        <span class="label">In this clip</span>
        {#if killSummary(clip)}<p class="summary">{killSummary(clip)}</p>{/if}
        <ul>
          {#each moments as e, i (i)}
            <li>
              <button class="ghost moment" onclick={() => seek(Math.max(0, e.atSecs - 1))}>
                <Icon event={e.kind} size={18} />
                <span class="m-name">{eventName(e.kind)}{#if e.detail}<span class="m-detail"> · {weaponName(e.detail)}</span>{/if}</span>
                <span class="m-time mono">{timecode(e.atSecs)}</span>
              </button>
            </li>
          {:else}
            <li class="empty">No game events. Saved with the hotkey.</li>
          {/each}
        </ul>
      </div>
    </aside>
  </div>

  <div class="transport">
    <div class="t-left">
      <button onclick={setStart} title="Start the clip here (I)"><Icon name="setStart" size={17} />Set start <kbd>I</kbd></button>
      <button onclick={setEnd} title="End the clip here (O)"><Icon name="setEnd" size={17} />Set end <kbd>O</kbd></button>
      <button class="ghost" class:on={loop} onclick={toggleLoop} title="Loop the trimmed part (L)"><Icon name="loop" size={17} /><span class="word">Loop</span></button>
      {#if inPoint > 0.01 || outPoint < length - 0.01}
        <button class="ghost" onclick={resetRange} title="Reset trim"><Icon name="x" size={15} /><span class="word">Reset</span></button>
      {/if}
    </div>
    <div class="t-center">
      <span class="time mono now">{timecode(current)}</span>
      <button class="ghost icon" onclick={() => step(-1)} title="Previous frame (,)"><Icon name="prevFrame" size={17} /></button>
      <button class="play" onclick={toggle} aria-label={playing ? "Pause" : "Play"} title="Play / pause (Space)">
        <Icon name={playing ? "pause" : "play"} size={20} />
      </button>
      <button class="ghost icon" onclick={() => step(1)} title="Next frame (.)"><Icon name="nextFrame" size={17} /></button>
      <span class="time mono total">{timecode(length)}</span>
    </div>
    <div class="t-right">
      <ExportMenu path={clip.path} {inPoint} {outPoint} tracks={separate} volumes={exportVolumes} mixVolume={masterLevel} />
      <span class="t-spacer"></span>
      <div class="preview" title="Preview volume: only how loud you hear it here, not the clip's levels (M mutes)">
        <button class="ghost icon" onclick={() => (previewMuted = !previewMuted)} aria-label="Mute preview"><Icon name={volumeIcon} size={18} /></button>
        <Slider min={0} max={1} step={0.02} value={previewLevel} label="Preview volume" oninput={setPreview} onchange={(v) => setPref("previewVolume", v)} />
      </div>
      <button class="ghost icon" onclick={fullscreen} title="Fullscreen (F)" aria-label="Fullscreen"><Icon name="fullscreen" size={18} /></button>
    </div>
  </div>

  <Timeline
    duration={length}
    {current}
    {inPoint}
    {outPoint}
    events={clip.events}
    {lanes}
    onseek={seek}
    onrange={onRange}
    ondrag={onDrag}
    onmarker={(t) => seek(Math.max(0, t - 1))}
    onvolume={setVolume}
    onmute={toggleMute}
    onsolo={toggleSolo}
  />
</section>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 62px;
    padding-left: 18px;
    border-bottom: 1px solid var(--line);
  }

  /* Window controls sit in the corner, like a title bar's. */
  header > :global(.controls) {
    align-self: flex-start;
  }

  .divider {
    width: 1px;
    height: 22px;
    margin: 0 4px;
    background: var(--line-strong);
  }

  /* So the title drags the window too. */
  .title > * {
    pointer-events: none;
  }

  .badge {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 11px;
    background: var(--surface);
  }

  .badge.hot {
    background: var(--accent);
    color: var(--on-accent);
  }

  .badge.hot :global(.icon) {
    --accent: var(--on-accent);
  }

  .title {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-right: auto;
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 18px;
    letter-spacing: -0.02em;
  }

  .sub {
    color: var(--muted);
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .saved {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--good);
    opacity: 0;
    transition: opacity 0.3s;
  }

  .saved.show {
    opacity: 1;
  }

  .fav {
    color: var(--accent);
  }

  .fav :global(path) {
    fill: currentColor;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 280px;
  }

  .stage {
    position: relative;
    min-width: 0;
    background: var(--stage);
  }

  /* Absolutely placed so a big video can't push the controls off screen. */
  video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    cursor: pointer;
  }

  .flash {
    position: absolute;
    left: 50%;
    top: 50%;
    display: grid;
    place-items: center;
    width: 72px;
    height: 72px;
    margin: -36px 0 0 -36px;
    border-radius: 50%;
    background: var(--glass);
    pointer-events: none;
    animation: flash 0.45s var(--ease) forwards;
  }

  @keyframes flash {
    from {
      opacity: 1;
      transform: scale(0.85);
    }
    to {
      opacity: 0;
      transform: scale(1.25);
    }
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 22px;
    padding: 18px;
    border-left: 1px solid var(--line);
    overflow-y: auto;
  }

  .moments {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .summary {
    margin: 0;
    color: var(--muted);
    font-size: 13px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .moment {
    justify-content: flex-start;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    text-align: left;
  }

  .m-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .m-detail,
  .m-time {
    color: var(--muted);
    font-size: 12px;
  }

  .empty {
    color: var(--faint);
    font-size: 13px;
  }

  .transport {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    padding: 10px 20px;
    border-top: 1px solid var(--line);
  }

  .t-left,
  .t-right {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .t-right {
    padding-left: 12px;
  }

  .t-spacer {
    flex: 1;
  }

  .t-center {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .time {
    width: 66px;
    font-size: 13px;
  }

  .now {
    text-align: right;
  }

  .total {
    color: var(--muted);
  }

  .play {
    width: 46px;
    height: 46px;
    margin: 0 4px;
    padding: 0;
    border-radius: 50%;
    background: var(--text);
    color: var(--bg);
    border: none;
  }

  .play:hover {
    background: #fff;
    transform: scale(1.04);
  }

  .transport button.on {
    color: var(--accent);
    background: var(--accent-soft);
  }

  .transport kbd {
    margin-left: 2px;
  }

  .transport button {
    white-space: nowrap;
  }

  .preview {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .preview :global(.slider) {
    width: 104px;
  }

  @media (max-width: 1420px) {
    .t-left kbd {
      display: none;
    }
  }

  @media (max-width: 1150px) {
    .t-left .word {
      display: none;
    }
  }

  /* Fullscreen: the stage fills the screen with its own small controls. */
  .stage.fs video {
    cursor: default;
  }

  .stage.idle,
  .stage.idle video {
    cursor: none;
  }

  .fs-controls {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 64px 40px 24px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.78));
    color: #fff;
    transition: opacity 0.3s;
  }

  .stage.idle .fs-controls {
    opacity: 0;
    pointer-events: none;
  }

  .fs-markers {
    position: relative;
    height: 26px;
    margin-bottom: 8px;
  }

  .fs-marker {
    position: absolute;
    bottom: 0;
    transform: translateX(-50%);
    gap: 4px;
    height: 28px;
    padding: 0 8px;
    border: none;
    border-radius: 8px;
    background: var(--glass);
    color: var(--text);
    font-size: 11px;
    font-weight: 700;
  }

  .fs-marker:hover {
    background: var(--surface-3);
  }

  .fs-marker.hot {
    background: var(--accent);
    color: var(--on-accent);
  }

  .fs-marker.hot :global(.icon) {
    --accent: var(--on-accent);
  }

  .fs-bar {
    position: relative;
    display: flex;
    align-items: center;
    height: 18px;
    cursor: pointer;
    touch-action: none;
  }

  .fs-track {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.2);
    transition: height 0.12s;
  }

  .fs-bar:hover .fs-track {
    height: 6px;
  }

  .fs-trim {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgba(255, 255, 255, 0.22);
  }

  .fs-fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: var(--accent);
  }

  .fs-tick {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--text);
  }

  .fs-knob {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 6px rgba(0, 0, 0, 0.5);
    pointer-events: none;
  }

  .fs-hover {
    position: absolute;
    bottom: 22px;
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--glass);
    font-size: 11px;
    pointer-events: none;
  }

  .fs-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }

  .fs-btn {
    width: 46px;
    height: 46px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: #fff;
  }

  .fs-btn:hover {
    background: rgba(255, 255, 255, 0.14);
  }

  .fs-time {
    font-size: 15px;
  }

  .fs-time span {
    color: rgba(255, 255, 255, 0.6);
  }

  .fs-volume {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .fs-volume :global(.slider) {
    width: 120px;
  }

  .fs-spacer {
    flex: 1;
  }

  .fs-title {
    font-size: 15px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
  }
</style>