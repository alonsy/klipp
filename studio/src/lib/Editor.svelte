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
  import { DUR, enter, leave, unfold } from "./motion";
  import { EDITOR_KEYS } from "./shortcuts";
  import { deleteClips, openShort, prefs, setClipEdit, setFavorite, setPref, shownThumbs, toast, ui } from "./state.svelte";
  import Timeline, { type Lane } from "./Timeline.svelte";
  import { swallowPress } from "./ui/clicks";
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
  /** The speeds the speed slider snaps to. */
  const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2];
  const NORMAL = SPEEDS.indexOf(1);

  let video: HTMLVideoElement;
  let stage: HTMLDivElement;
  let length = $state(initial.duration);
  let inPoint = $state(saved.in ?? 0);
  let outPoint = $state(saved.out ?? initial.duration);
  let current = $state(saved.in ?? 0);
  let playing = $state(false);
  // Off when a clip opens; L turns it on.
  let loop = $state(false);
  let volumes = $state(separate.map((_, i) => saved.volumes?.[i] ?? 1));
  let muted = $state(separate.map((_, i) => saved.muted?.[i] ?? false));
  let solo = $state(separate.map(() => false));
  let master = $state(saved.master ?? 1);
  let masterMuted = $state(false);
  let flash = $state<"play" | "pause" | null>(null);
  let flashes = $state(0);
  let flashTimer = 0;
  let savedNote = $state(false);
  /** The More menu in the top strip. */
  let menu = $state(false);
  let menuEl = $state<HTMLDivElement | null>(null);
  /** The panel over the right of the picture: what's in the clip, or the shortcuts. */
  let panel = $state<"moments" | "keys" | null>(null);
  /** How loud you hear it here. Separate from the clip's own levels, never exported. */
  let previewVolume = $state(prefs.previewVolume);
  let previewMuted = $state(false);
  let previewLevel = $derived(previewMuted ? 0 : previewVolume);
  let volumeIcon = $derived(previewLevel === 0 ? "mute" : previewLevel < 0.5 ? "volumeLow" : "volume");
  /** How fast you watch it here, as a step of SPEEDS. Never exported; each clip opens at 1×. */
  let speedStep = $state(NORMAL);
  let speed = $derived(SPEEDS[speedStep]);
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
      clearTimeout(flashTimer);
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

  $effect(() => {
    // Picture and sound both follow the speed; the pitch does too, as it does in the mixer.
    video.preservesPitch = false;
    video.playbackRate = video.defaultPlaybackRate = speed;
    if (audioReady) mixer?.setRate(speed);
  });

  function loaded() {
    length = video.duration;
    // Saved trim points from an older file length stay inside the clip.
    outPoint = Math.min(saved.out ?? length, length);
    inPoint = Math.min(inPoint, Math.max(0, outPoint - 0.5));
    seek(inPoint);
    video.play().catch(() => {});
  }

  function play(from?: number) {
    if (from != null) seek(from);
    else if (video.currentTime >= outPoint - 0.05 || video.currentTime < inPoint - 0.05) seek(inPoint);
    video.play().catch(() => {});
  }

  function toggle() {
    if (video.paused) play();
    else video.pause();
    // Answers on the picture too, from the keyboard as much as from a click.
    flash = video.paused ? "pause" : "play";
    flashes += 1;
    clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => (flash = null), 450);
  }

  function clickVideo() {
    toggle();
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
        setClipEdit(initial.path, untouched ? null : edit);
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
      // Kept from a focused slider, which would jump to its own end too (2× speed, no volume).
      case "Home":
        e.preventDefault();
        seek(inPoint);
        break;
      case "End":
        e.preventDefault();
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
      case "?":
        openPanel("keys");
        break;
      case "Escape":
        if (menu) menu = false;
        else if (panel) panel = null;
        else if (!document.fullscreenElement) close();
        break;
    }
  }

  function openPanel(which: "moments" | "keys") {
    menu = false;
    panel = panel === which ? null : which;
  }

  function outsideMenu(e: PointerEvent) {
    if (!menu || menuEl?.contains(e.target as Node)) return;
    menu = false;
    swallowPress(e);
  }

  let subtitle = $derived(
    [clip.map ? mapName(clip.map) : clip.game, clip.mode ? modeName(clip.mode) : "", when(clip.created)].filter(Boolean).join(" · "),
  );
  let badge = $derived(clip.label ?? (clip.kind === "auto" ? "kill" : "manual"));
  let markers = $derived(mergeMarkers(clip.events));
  let moments = $derived([...clip.events].filter((e) => e.kind !== "kill" || !clip.events.some((o) => o !== e && Math.abs(o.atSecs - e.atSecs) < 0.3 && o.kind !== "kill")).sort((a, b) => a.atSecs - b.atSecs));
  let trimmed = $derived(inPoint > 0.01 || outPoint < length - 0.01);
</script>

<svelte:window onkeydown={onKey} onpointerdowncapture={outsideMenu} />

<section class="editor">
  <!-- Also the title bar: empty space drags the window. -->
  <header data-tauri-drag-region>
    <button class="ghost icon" onclick={close} title="Back to clips (Esc)" aria-label="Back to clips"><Icon name="back" size={18} /></button>
    <div class="title" data-tauri-drag-region>
      <span class="badge" class:hot={isMultiKill(badge)}><Icon event={badge} size={20} /></span>
      <h1>{clipTitle(clip)}</h1>
      <span class="sub">{subtitle}</span>
    </div>
    <span class="saved" class:show={savedNote}><Icon name="check" size={13} /> Saved</span>
    <button class="ghost short" onclick={() => openShort(clip, "editor")} title="Make a vertical short">
      <Icon name="phone" size={16} />Short{#if clip.camera}<Icon name="camera" size={14} />{/if}
    </button>
    <button class="ghost icon" class:fav={clip.favorite} onclick={() => setFavorite([clip.path], !clip.favorite)} title={clip.favorite ? "Remove from favorites" : "Add to favorites"} aria-label={clip.favorite ? "Remove from favorites" : "Add to favorites"}>
      <Icon name="star" size={17} />
    </button>
    <div class="more" bind:this={menuEl}>
      <button class="ghost icon" aria-label="More" aria-haspopup="menu" aria-expanded={menu} title="More" onclick={() => (menu = !menu)}><Icon name="more" size={18} /></button>
      {#if menu}
        <div class="menu" role="menu" in:unfold out:leave={{ y: -4 }}>
          <button
            class="ghost"
            role="menuitem"
            onclick={() => {
              menu = false;
              api.reveal(clip.path);
            }}><Icon name="folder" size={16} />Show in folder</button
          >
          <button class="ghost" role="menuitem" onclick={() => openPanel("keys")}><Icon name="keyboard" size={16} />Shortcuts<kbd>?</kbd></button>
          <button
            class="ghost danger"
            role="menuitem"
            onclick={() => {
              menu = false;
              deleteClips([clip.path]);
            }}><Icon name="trash" size={16} />Delete</button
          >
        </div>
      {/if}
    </div>
    <ExportMenu path={clip.path} {inPoint} {outPoint} tracks={separate} volumes={exportVolumes} mixVolume={masterLevel} />
    <WindowControls />
  </header>

  <div class="body">
    <div class="stage" class:fs={isFullscreen} class:idle={isFullscreen && fsIdle && playing} bind:this={stage} onpointermove={wake} role="presentation">
      <!-- svelte-ignore a11y_media_has_caption, a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <video
        bind:this={video}
        data-hero
        poster={clip.thumb ? api.fileUrl(clip.thumb) : shownThumbs.get(clip.path)}
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
      {#key flashes}
        {#if flash}
          <div class="flash"><Icon name={flash} size={26} /></div>
        {/if}
      {/key}
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
                <Icon event={m.kind} size={18} />{#if isHot(m.kind)}<span class="mono">{eventName(m.kind, clip.game)}</span>{/if}
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
            <div class="fs-speed">
              <button class="fs-rate mono" onclick={() => (speedStep = NORMAL)} title="Back to 1×">{speed}×</button>
              <Slider min={0} max={SPEEDS.length - 1} value={speedStep} label="Playback speed" oninput={(i) => (speedStep = i)} />
            </div>
            <span class="fs-spacer"></span>
            <span class="fs-title">{clipTitle(clip)}</span>
            <button class="fs-btn" onclick={fullscreen} aria-label="Exit fullscreen" title="Exit fullscreen (F)"><Icon name="exitFullscreen" size={21} /></button>
          </div>
        </div>
      {/if}
    </div>

    {#if panel}
      <aside class="side" in:enter={{ x: 16, y: 0 }} out:leave={{ x: 12 }}>
        <div class="side-head">
          <h2>{panel === "moments" ? "In this clip" : "Shortcuts"}</h2>
          <button class="ghost icon" onclick={() => (panel = null)} aria-label="Close" title="Close (Esc)"><Icon name="x" size={15} /></button>
        </div>
        {#if panel === "moments"}
          <div class="moments">
            {#if killSummary(clip)}<p class="summary">{killSummary(clip)}</p>{/if}
            <ul>
              {#each moments as e, i (i)}
                <li>
                  <button class="ghost moment" onclick={() => seek(Math.max(0, e.atSecs - 1))}>
                    <Icon event={e.kind} size={18} />
                    <span class="m-name">{eventName(e.kind, clip.game)}{#if e.detail}<span class="m-detail">{` · ${weaponName(e.detail)}`}</span>{/if}</span>
                    <span class="m-time mono">{timecode(e.atSecs)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        {:else}
          <ul class="keys">
            {#each EDITOR_KEYS as [combo, what] (what)}
              <li>
                <span>{what}</span>
                <span class="combo">{#each combo as k (k)}<kbd>{k}</kbd>{/each}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </aside>
    {/if}
  </div>

  <div class="transport">
    <div class="t-left">
      <span class="time mono"><span class="now">{timecode(current)}</span><span class="total">{timecode(length)}</span></span>
      <button class="ghost icon" onclick={setStart} title="Start the clip here (I)" aria-label="Set start"><Icon name="setStart" size={18} /></button>
      <button class="ghost icon" onclick={setEnd} title="End the clip here (O)" aria-label="Set end"><Icon name="setEnd" size={18} /></button>
      <button class="ghost icon" class:on={loop} aria-pressed={loop} onclick={toggleLoop} title="Loop the trimmed part (L)" aria-label="Loop"><Icon name="loop" size={17} /></button>
      {#if trimmed}
        <button class="ghost kept" in:enter={{ y: 4, duration: DUR.base }} onclick={resetRange} title="{timecode(inPoint)} – {timecode(outPoint)} · Reset trim" aria-label="Reset trim"><span class="mono">{(outPoint - inPoint).toFixed(1)} s</span><Icon name="x" size={13} /></button>
      {/if}
    </div>
    <div class="t-center">
      <button class="ghost icon" onclick={() => step(-1)} title="Previous frame (,)" aria-label="Previous frame"><Icon name="prevFrame" size={17} /></button>
      <button class="play" class:playing onclick={toggle} aria-label={playing ? "Pause" : "Play"} title="Play / pause (Space)">
        <span class="glyph to-play"><Icon name="play" size={18} /></span>
        <span class="glyph to-pause"><Icon name="pause" size={18} /></span>
      </button>
      <button class="ghost icon" onclick={() => step(1)} title="Next frame (.)" aria-label="Next frame"><Icon name="nextFrame" size={17} /></button>
    </div>
    <div class="t-right">
      <div class="speed">
        <button
          class="ghost rate mono"
          class:on={speedStep !== NORMAL}
          aria-label="Playback speed {speed}×"
          data-tip={speedStep !== NORMAL ? "Back to 1×" : undefined}
          onclick={() => (speedStep = NORMAL)}>{speed}×</button
        >
        <div class="speed-pop">
          <Slider min={0} max={SPEEDS.length - 1} value={speedStep} label="Playback speed" oninput={(i) => (speedStep = i)} />
          <span>Only here, not the export</span>
        </div>
      </div>
      <div class="preview" title="Preview volume, not the clip's levels (M)">
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
    media={() => video}
    {noAudio}
    eventsOpen={panel === "moments"}
    onevents={() => openPanel("moments")}
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
    position: relative;
    z-index: 30;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex-shrink: 0;
    height: 44px;
    padding-left: var(--sp-2);
  }

  /* Window controls sit in the corner, like a title bar's. */
  header > :global(.controls) {
    align-self: flex-start;
    margin-left: var(--sp-2);
  }

  /* So the title drags the window too. */
  .title > * {
    pointer-events: none;
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: 1;
    min-width: 0;
    margin-left: var(--sp-1);
  }

  .badge {
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }

  h1 {
    margin: 0;
    flex-shrink: 0;
    font-family: var(--font-display);
    font-size: var(--fs-base);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .sub {
    min-width: 0;
    margin-left: var(--sp-1);
    color: var(--faint);
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .saved {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    margin-right: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-sm);
    white-space: nowrap;
    opacity: 0;
    pointer-events: none; /* It's on the title bar: the window drags through it. */
    transition: opacity var(--dur-slow) var(--ease-out);
  }

  .saved.show {
    opacity: 1;
  }

  .fav {
    color: var(--accent);
  }

  .short {
    flex-shrink: 0;
    gap: 6px;
    height: 34px;
    margin-right: var(--sp-1);
    padding: 0 var(--sp-3) 0 var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .short:hover {
    color: var(--text);
  }

  .fav :global(path) {
    fill: currentColor;
  }

  .more {
    position: relative;
    margin-right: var(--sp-1);
  }

  .menu {
    position: absolute;
    top: calc(100% + var(--sp-2));
    right: 0;
    display: flex;
    flex-direction: column;
    width: 196px;
    padding: var(--sp-1);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--shadow-2);
    transform-origin: top right;
  }

  .menu button {
    justify-content: flex-start;
    gap: var(--sp-3);
    padding: 7px var(--sp-3);
  }

  .menu kbd {
    margin-left: auto;
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .stage {
    position: absolute;
    inset: 0;
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
    width: 64px;
    height: 64px;
    margin: -32px 0 0 -32px;
    border-radius: 50%;
    background: var(--glass);
    color: var(--text);
    pointer-events: none;
    animation: flash var(--dur-spring) var(--ease-out) forwards;
  }

  @keyframes flash {
    from {
      opacity: 0;
      transform: scale(0.7);
    }
    22% {
      opacity: 1;
      transform: scale(1);
    }
    to {
      opacity: 0;
      transform: scale(1.18);
    }
  }

  /* Floats over the picture, so opening it never resizes the video. */
  .side {
    position: absolute;
    top: var(--sp-3);
    right: var(--sp-3);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 272px;
    max-height: calc(100% - 2 * var(--sp-3));
    padding: var(--sp-2) var(--sp-2) var(--sp-3) var(--sp-4);
    border-radius: var(--r-md);
    background: var(--glass);
    backdrop-filter: blur(20px);
    box-shadow: var(--shadow-2);
    overflow-y: auto;
  }

  .side-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .side-head button {
    width: 28px;
    height: 28px;
  }

  h2 {
    margin: 0;
    font-size: var(--fs-base);
    font-weight: 600;
  }

  .moments {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-left: calc(-1 * var(--sp-2));
  }

  .summary {
    margin: 0 0 0 var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .moment {
    justify-content: flex-start;
    gap: var(--sp-2);
    width: 100%;
    padding: 6px var(--sp-2);
    font-size: var(--fs-sm);
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
    font-size: var(--fs-sm);
  }

  .keys {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-right: var(--sp-2);
    font-size: var(--fs-sm);
    color: var(--muted);
  }

  .keys li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }

  .combo {
    display: flex;
    gap: var(--sp-1);
    flex-shrink: 0;
  }

  .transport {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--sp-3);
    flex-shrink: 0;
    height: 52px;
    padding: 0 18px 0 16px;
  }

  .t-left,
  .t-right {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }

  .t-right {
    justify-content: flex-end;
  }

  .t-center {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }

  /* As wide as the track names below it, so the buttons start where the timeline does. */
  .time {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    flex-shrink: 0;
    width: 126px;
    margin-right: 6px;
    white-space: nowrap;
  }

  .now {
    font-size: var(--fs-base);
    font-weight: 500;
  }

  .total {
    color: var(--faint);
    font-size: var(--fs-xs);
  }

  .play {
    position: relative;
    width: 38px;
    height: 38px;
    margin: 0 var(--sp-1);
    padding: 0;
    border-radius: 50%;
    background: var(--text);
    color: var(--on-accent);
  }

  .play:hover {
    background: var(--text);
    transform: scale(1.05);
  }

  .play:active {
    transform: scale(0.94);
  }

  /* Both shapes are there; one turns into the other. */
  .glyph {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    transition:
      opacity var(--dur-fast) var(--ease-out),
      transform var(--dur) var(--ease-out);
  }

  .to-pause,
  .playing .to-play {
    opacity: 0;
    transform: scale(0.6);
  }

  .playing .to-pause {
    opacity: 1;
    transform: none;
  }

  .transport button.on {
    color: var(--accent);
    background: var(--accent-soft);
  }

  /* How long the kept part is; a click undoes the trim. */
  .kept {
    gap: var(--sp-1);
    height: 28px;
    margin-left: var(--sp-2);
    padding: 0 var(--sp-2);
    color: var(--accent);
    background: var(--accent-soft);
    font-size: var(--fs-sm);
    white-space: nowrap;
  }

  .kept:hover {
    color: var(--text);
  }

  .preview {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .preview :global(.slider) {
    width: 84px;
  }

  .speed {
    position: relative;
    display: flex;
    align-items: center;
  }

  /* Wide enough for "0.25×", so nothing moves as the speed changes. */
  .rate {
    width: 52px;
    height: 34px;
    padding: 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  /* Quiet until it's looked for: the slider rises out of the number. */
  .speed-pop {
    position: absolute;
    bottom: calc(100% + var(--sp-2));
    left: 50%;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 172px;
    margin-left: -86px;
    padding: var(--sp-3);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--shadow-2);
    color: var(--faint);
    font-size: var(--fs-xs);
    white-space: nowrap;
    transform-origin: bottom center;
    opacity: 0;
    visibility: hidden;
    transform: translateY(4px) scale(0.98);
    transition:
      opacity var(--dur-fast) var(--ease-out),
      transform var(--dur-fast) var(--ease-out),
      visibility 0s linear var(--dur-fast);
  }

  /* Bridges the gap, so moving up from the number keeps it open. */
  .speed-pop::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    height: var(--sp-2);
  }

  .speed:hover .speed-pop,
  .speed:focus-within .speed-pop {
    opacity: 1;
    visibility: visible;
    transform: none;
    transition:
      opacity var(--dur) var(--ease-out),
      transform var(--dur-spring) var(--spring),
      visibility 0s;
  }

  .speed :global(.slider) {
    width: 100%;
  }

  @media (max-height: 700px) {
    .transport {
      height: 46px;
    }

    .play {
      width: 34px;
      height: 34px;
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
    background: linear-gradient(transparent, color-mix(in srgb, var(--stage) 80%, transparent));
    color: var(--text);
    transition: opacity var(--dur-slow) var(--ease-out);
  }

  .stage.idle .fs-controls {
    opacity: 0;
    pointer-events: none;
  }

  .fs-markers {
    position: relative;
    height: 26px;
    margin-bottom: var(--sp-2);
  }

  .fs-marker {
    position: absolute;
    bottom: 0;
    transform: translateX(-50%);
    gap: var(--sp-1);
    height: 28px;
    padding: 0 var(--sp-2);
    border-radius: var(--r-full);
    background: var(--glass);
    color: var(--text);
    font-size: var(--fs-xs);
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
    background: var(--line-strong);
    transition: transform var(--dur-fast) var(--ease-out);
  }

  .fs-bar:hover .fs-track {
    transform: scaleY(1.5);
  }

  .fs-trim {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--line-strong);
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
    background: var(--text);
    box-shadow: var(--shadow-1);
    pointer-events: none;
  }

  .fs-hover {
    position: absolute;
    bottom: 22px;
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: var(--r-xs);
    background: var(--glass);
    font-size: var(--fs-xs);
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
    border-radius: 50%;
    background: transparent;
    color: var(--text);
  }

  .fs-btn:hover {
    background: var(--press);
  }

  .fs-time {
    font-size: var(--fs-md);
  }

  .fs-time span {
    color: var(--muted);
  }

  .fs-volume {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .fs-volume :global(.slider) {
    width: 120px;
  }

  .fs-speed {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .fs-rate {
    width: 64px;
    height: 46px;
    padding: 0;
    border-radius: var(--r-full);
    background: transparent;
    color: var(--text);
    font-size: var(--fs-md);
  }

  .fs-rate:hover {
    background: var(--press);
  }

  .fs-speed :global(.slider) {
    width: 96px;
  }

  .fs-spacer {
    flex: 1;
  }

  .fs-title {
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
</style>
