<script lang="ts">
  // First-run setup: checks what this PC can record, then asks for audio,
  // video and saving choices with the best ones preselected, and starts
  // recording with them. Also reachable from Settings ("Run setup diagnostics
  // again"), and shown once after an update as just the PC test and video
  // (`pcOnly`).
  import { onMount, untrack } from "svelte";
  import * as api from "../api";
  import type { AudioDevice, Drive, SystemCheck } from "../api";
  import HotkeyInput from "../HotkeyInput.svelte";
  import HotkeyTest from "../HotkeyTest.svelte";
  import { DISCORD, routerInstalled } from "../discordTrack";
  import Icon from "../Icon.svelte";
  import Logo from "../Logo.svelte";
  import { DUR, easeOut, enter, leave, pageIn, pageOut, reduced } from "../motion";
  import { recommendPreset, type PcTest } from "../pctest";
  import { matchPreset, PRESETS, presetValues, type PresetId } from "../presets";
  import SystemMeters from "../SystemMeters.svelte";
  import { library, openPage, refreshRecorder, reload, toast } from "../state.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import WindowControls from "../WindowControls.svelte";

  let { rerun = false, pcOnly = false, ondone }: { rerun?: boolean; pcOnly?: boolean; ondone?: () => void } = $props();

  const allSteps = [
    { id: "welcome", name: "Welcome" },
    { id: "check", name: "Your PC" },
    { id: "audio", name: "Audio" },
    { id: "video", name: "Video" },
    { id: "saving", name: "Saving clips" },
  ] as const;
  type Step = (typeof allSteps)[number]["id"];
  const steps = $derived(pcOnly ? allSteps.filter((s) => s.id === "check" || s.id === "video") : allSteps);
  // The component lives for one run of setup, so the first step is fixed.
  let step = $state<Step>(untrack(() => (pcOnly ? "check" : "welcome")));
  let index = $derived(steps.findIndex((s) => s.id === step));
  /** The way steps travel: 1 forward, -1 back. */
  let dir = $state(1);

  let check = $state<SystemCheck | null>(null);
  let checkError = $state("");
  let devices = $state<AudioDevice[]>([]);
  let drives = $state<Drive[]>([]);
  /** The drive letter clips go on ("" until drives are known). */
  let drive = $state("");
  const chosenDrive = $derived(drives.find((d) => d.letter === drive));
  /** Setup run again with a different drive than the current clips folder's. */
  const driveChanged = $derived(rerun && !!drive && !library.clipsDir.toUpperCase().startsWith(drive));
  let finishing = $state(false);
  /** Recording has started: the last beat before the library. */
  let finished = $state(false);
  /** The save key test's result, shown under the key. */
  let keyNote = $state("");
  let keyBad = $state(false);
  let keyPhase = $state<"idle" | "ready" | "press" | "works" | "failed">("idle");
  /** The video step shows the fine-tuning instead of the presets. */
  let tune = $state(false);

  // Choices, preselected once the check is done.
  let c = $state({
    sonar: false,
    output: "",
    /** Discord on its own track (experimental). */
    discord: false,
    mic: true,
    micDevice: "",
    sound: true,
    cue: "",
    volume: 0.6,
    resolution: "native" as "native" | "1440p" | "1080p" | "720p",
    fps: 60,
    codec: "av1" as "av1" | "h264",
    quality: 30,
    max_bitrate_mbps: 40,
    capture: "auto" as "auto" | "light" | "wgc" | "duplication",
    performance: "off" as "off" | "on" | "plus",
    fast_encoder: false,
    /** "" = the main display. */
    monitor: "",
    hotkey: "F8",
    length: 60,
    autostart: true,
    cs2: true,
  });

  const playback = $derived(devices.filter((d) => d.kind === "playback").map((d) => d.name));
  const recording = $derived(devices.filter((d) => d.kind === "recording").map((d) => d.name));
  /** An audio router replays other apps, so Discord can't be left out of Game. */
  const router = $derived(routerInstalled(playback));
  const sonarFound = $derived(
    playback.some((n) => n.includes("Sonar - Gaming")) && playback.some((n) => n.includes("Sonar - Chat")),
  );

  onMount(() => {
    // Both take a moment; start them while Welcome is on screen.
    api.audioDevices().then((d) => (devices = d), () => {});
    api.drives().then((list) => {
      drives = list;
      // Run again: where clips are now. First run: the drive with the most room.
      const current = list.find((d) => library.clipsDir.toUpperCase().startsWith(d.letter));
      const roomiest = [...list].sort((a, b) => b.freeGb - a.freeGb)[0];
      drive = (rerun ? current : roomiest)?.letter ?? current?.letter ?? "";
    }, () => {});
    api.systemCheck().then(
      (r) => {
        check = r;
        recommend(r);
      },
      (e) => (checkError = String(e)),
    );
    if (rerun) {
      api.readSettings().then((s) => {
        c.hotkey = s.replay?.hotkey ?? c.hotkey;
        c.length = s.replay?.length_secs ?? c.length;
        c.cue = s.cue?.device ?? "";
        c.sound = s.cue?.enabled ?? true;
        c.monitor = s.video?.monitor ?? "";
        c.volume = Number(s.cue?.volume ?? 0.6);
      }, () => {});
    }
    if (pcOnly) {
      // Only the video is asked for: start from what's set now.
      api.readSettings().then((s) => {
        c.monitor = s.video?.monitor ?? "";
        c.length = s.replay?.length_secs ?? c.length;
      }, () => {});
    }
  });

  /** Set once the audio choices were changed by hand, so detection stops deciding. */
  let touchedAudio = false;

  $effect(() => {
    // Sonar splits game, chat and mic into devices: record them as separate tracks.
    if (sonarFound && !c.sonar && devices.length > 0 && !touchedAudio) {
      c.sonar = true;
      const headset = likelyHeadset(playback);
      if (headset && !c.cue) c.cue = headset;
    }
  });

  $effect(() => {
    // Saved settings may name a device by part of its name ("Arctis Nova Pro");
    // show the full name it stands for, as Settings → Recording does.
    if (c.cue && playback.length > 0 && !playback.includes(c.cue)) {
      const full = playback.find((n) => n.toLowerCase().includes(c.cue.toLowerCase()));
      if (full) c.cue = full;
    }
  });

  /** The real headset or speakers, skipping virtual devices (Sonar, cables, mixers)
   * that Klipp might be recording: where the save sound won't end up in clips. */
  function likelyHeadset(names: string[]): string | undefined {
    const virtual = /sonar|cable|vb-audio|virtual|voicemeeter|broadcast|steam streaming|wave link|nahimic|vaio/i;
    const real = names.filter((n) => !virtual.test(n));
    return real.find((n) => /headset|headphone|arctis|hyperx|astro|razer|corsair|logitech|steelseries|sennheiser|beyerdynamic/i.test(n)) ?? real[0];
  }

  /** Encoder works at this height (the probe opened it). */
  function encodes(codec: "av1" | "h264", height: number) {
    return !!check?.encoders.some((e) => e.codec === codec && e.height === height && e.ok);
  }

  const nativeH = $derived(check?.monitor?.height ?? 1080);
  const heightOf = (r: string) => (r === "native" ? nativeH : Number(r.replace("p", "")));
  const resolutions = $derived(
    (["native", "1440p", "1080p", "720p"] as const).filter((r) => r === "native" || heightOf(r) < nativeH),
  );
  const canAv1 = $derived(encodes("av1", heightOf(c.resolution)));
  const canH264 = $derived(encodes("h264", heightOf(c.resolution)));
  const canRecord = $derived(!!check && check.encoders.some((e) => e.ok));
  /** Built-in graphics (shares the PC's memory): the recording GPU has under 1 GB of its own. */
  const integrated = $derived.by(() => {
    const gpu = check?.gpus.find((g) => g.name === check?.recordingGpu);
    return !!gpu && gpu.vendor !== "nvidia" && gpu.vramMb < 1024;
  });
  const refresh = $derived(check?.monitor?.refreshHz ?? 60);
  const screen = $derived({ height: nativeH, refreshHz: refresh });
  const preset = $derived(matchPreset(c, screen));
  /** The preset the check chose, marked among the others. */
  let picked = $state<PresetId | null>(null);

  /** The check measured what recording costs this PC (newer recorders): it is the PC test. */
  function asPcTest(r: SystemCheck | null): PcTest | null {
    return r?.system ? ({ testVersion: 1, klippVersion: "", at: new Date().toISOString(), ...r } as unknown as PcTest) : null;
  }
  const pcTest = $derived(asPcTest(check));

  /** The preset this PC runs smoothly with room left for a game. The probe
   * recorded a few seconds at each candidate; 120 fps needs about twice the
   * memory for the buffer, so not on PCs with little of it. */
  function recommendedPreset(r: SystemCheck): PresetId {
    const h = r.monitor?.height ?? 1080;
    const gpu = r.gpus.find((g) => g.name === r.recordingGpu);
    const builtIn = !!gpu && gpu.vendor !== "nvidia" && gpu.vramMb < 1024;
    const rec = r.recommended;
    if (!rec || rec.height < Math.min(h, 1440)) return "plus";
    if (rec.fps === 120 && (r.monitor?.refreshHz ?? 60) >= 120 && r.ramMb >= 12000 && !builtIn) return "high";
    return builtIn ? "performance" : "normal";
  }

  function applyPreset(id: PresetId) {
    Object.assign(c, presetValues(id, screen));
    // Keep the codec possible at the chosen size.
    c.codec = encodes("av1", heightOf(c.resolution)) || !encodes("h264", heightOf(c.resolution)) ? "av1" : "h264";
  }

  function recommend(r: SystemCheck) {
    // Newer recorders measure what recording costs (the PC test); older ones only say what ran smoothly.
    const test = asPcTest(r);
    picked = test ? recommendPreset(test, { height: r.monitor?.height ?? 1080, refreshHz: r.monitor?.refreshHz ?? 60 }) : recommendedPreset(r);
    applyPreset(picked);
    c.codec = encodes("av1", heightOf(c.resolution)) ? "av1" : "h264";
    c.cs2 = r.cs2;
    // F8 on desktops; laptops often need Fn for F keys, and Alt+F8 is easy to reach with it.
    if (!rerun) c.hotkey = r.laptop ? "Alt+F8" : "F8";
    // Little memory: a shorter buffer.
    if (r.ramMb > 0 && r.ramMb < 8000) c.length = 30;
  }

  $effect(() => {
    // Keep the codec possible at the chosen size.
    if (check && c.codec === "av1" && !canAv1 && canH264) c.codec = "h264";
  });

  /** Rough game bitrate (Mbps): ~28 at 1440p60 in AV1, measured with CS2. */
  const mbps = $derived.by(() => {
    const h = heightOf(c.resolution);
    const pixels = ((h * 16) / 9) * h;
    return 28 * Math.pow(pixels / (2560 * 1440), 0.75) * Math.pow(c.fps / 60, 0.6) * (c.codec === "h264" ? 1.5 : 1);
  });
  const ramForBuffer = $derived(Math.round((c.length * mbps) / 8));
  const perMinute = $derived(Math.round((60 * mbps) / 8));

  function next() {
    dir = 1;
    if (index < steps.length - 1) step = steps[index + 1].id;
  }
  function back() {
    dir = -1;
    if (index > 0) step = steps[index - 1].id;
  }

  // The PC test on screen. The probe says nothing until it's done, so the ring
  // follows how long it usually takes (about 20 s, counted from when it started
  // behind Welcome), never quite arrives, and sweeps home when the answer comes.
  const startedAt = performance.now();
  const estimate = (ms: number) => Math.min(0.96, 1 - Math.exp(-ms / 9000));
  /** The ring, 0 to 1. */
  let meter = $state(0);
  /** The answer is in and the ring has landed: the result shows. */
  let revealed = $state(false);
  const SWEEP = DUR.slow * 2;

  $effect(() => {
    if (step !== "check" || revealed || checkError) return;
    const done = !!check;
    const from = done ? untrack(() => meter) : 0;
    const start = performance.now();
    // Frames stop while the window is covered; the timer lands it anyway.
    const land = done ? setTimeout(() => ((meter = 1), (revealed = true)), reduced() ? 0 : SWEEP + DUR.base) : 0;
    let frame = requestAnimationFrame(function draw(now) {
      meter = done ? from + (1 - from) * easeOut(Math.min(1, (now - start) / SWEEP)) : estimate(now - startedAt);
      frame = requestAnimationFrame(draw);
    });
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(land);
    };
  });

  const testPhase = $derived(
    check ? "Done"
    : meter < 0.15 ? "Finding your graphics card"
    : meter < 0.4 ? "Trying its video encoders"
    : meter < 0.8 ? "Recording a few seconds at each setting"
    : "Measuring what it costs",
  );

  const presetName = $derived(PRESETS.find((p) => p.id === preset)?.name ?? "Custom");
  const sizeName = (r: string) => (r === "native" ? `${nativeH}p` : r);
  const games = $derived(
    [check?.cs2 && "Counter-Strike 2", check?.league && "League of Legends", check?.rocketLeague && "Rocket League"].filter(Boolean).join(", "),
  );

  function sources() {
    if (c.sonar) {
      return [
        { name: "Game", device: "Sonar - Gaming", kind: "loopback" },
        { name: "Chat", device: "Sonar - Chat", kind: "loopback" },
        ...(c.mic ? [{ name: "Mic", device: "Sonar - Microphone", kind: "capture" }] : []),
      ];
    }
    return [
      { name: c.discord && !router ? "Game" : "PC audio", device: c.output, kind: "loopback" },
      ...(c.discord && !router ? [{ name: "Chat", device: "", kind: DISCORD }] : []),
      ...(c.mic ? [{ name: "Mic", device: c.micDevice, kind: "capture" }] : []),
    ];
  }

  /** The video choices, as settings. */
  function videoSettings() {
    return (["codec", "resolution", "fps", "quality", "max_bitrate_mbps", "capture", "performance", "fast_encoder", "monitor"] as const).map((k) => ({
      key: ["video", k],
      value: c[k],
    }));
  }

  /** After an update (pcOnly): the video settings only, or none. */
  async function finishPc(use: boolean) {
    finishing = true;
    try {
      if (use) await api.writeSettings(videoSettings());
      if (use) toast("Klipp records with the new video settings.");
      ondone?.();
      openPage("clips");
    } catch (e) {
      toast(`Couldn't save the video settings: ${e}`);
    } finally {
      finishing = false;
    }
  }

  async function finish() {
    finishing = true;
    try {
      if (chosenDrive) {
        await api.createFolder(chosenDrive.folder);
        await api.writeSettings([{ key: ["storage", "clips_dir"], value: chosenDrive.folder }]);
      }
      await api.writeSettings([
        { key: ["audio", "sources"], value: sources() },
        { key: ["cue", "enabled"], value: c.sound },
        { key: ["cue", "device"], value: c.cue },
        { key: ["cue", "volume"], value: c.volume },
        ...videoSettings(),
        { key: ["replay", "hotkey"], value: c.hotkey },
        { key: ["replay", "length_secs"], value: c.length },
        { key: ["auto_clip", "cs2", "enabled"], value: c.cs2 },
      ]);
      await api.setAutostart(c.autostart).catch(() => {});
      await api.startRecorder();
      setTimeout(refreshRecorder, 1500);
      await reload();
      // A moment to say it's running, then the library.
      finished = true;
      await new Promise((r) => setTimeout(r, reduced() ? 0 : DUR.spring * 3));
      toast(`Klipp is recording. Press ${c.hotkey} to save the last ${c.length} seconds.`);
      ondone?.();
      openPage("clips");
    } catch (e) {
      toast(`Couldn't finish setup: ${e}`);
    } finally {
      finishing = false;
    }
  }

  /** For a message or a GitHub issue when this PC can't record. */
  async function copyDetails() {
    try {
      const ok = await api.copyText(await api.supportDetails());
      toast(ok ? "Details copied. Paste them into a message or a GitHub issue." : "Couldn't copy the details.");
    } catch (e) {
      toast(`Couldn't collect the details: ${e}`);
    }
  }

  function fmtLength(s: number) {
    return s >= 60 ? `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} s` : ""}` : `${s} s`;
  }
</script>

<!-- The Buffer ring: `value` fills it from beside the dot, round to the dot. -->
{#snippet ring(size: number, value: number, kind = "")}
  <svg class="ring {kind}" viewBox="0 0 100 100" width={size} height={size} aria-hidden="true">
    <path class="track" pathLength="100" d="M80.9 41.7 A32 32 0 1 1 58.3 19.1" />
    <path class="arc" pathLength="100" d="M80.9 41.7 A32 32 0 1 1 58.3 19.1" style:stroke-dashoffset={100 - value * 100} />
    <circle class="dot" cx="79.7" cy="20.3" r="9" />
  </svg>
{/snippet}

{#snippet fact(name: string, value: string, kind = "", tip = "", delay = 0)}
  <div class="fact enter {kind}" title={tip || undefined} style:--delay="{delay}ms">
    <dt>{name}</dt>
    <dd>{#if kind}<Icon name={kind === "bad" ? "warning" : "info"} size={14} />{/if}{value}</dd>
  </div>
{/snippet}

{#snippet help()}
  <p class="help">
    Tell us about it and we'll look into it.
    <button class="copy" onclick={copyDetails}><Icon name="copy" size={15} />Copy details</button>
  </p>
{/snippet}

<div class="setup">
  <div class="glow" class:on={step === "welcome" && !finished} aria-hidden="true"></div>

  <header data-tauri-drag-region>
    <span class="brand" class:hidden={step === "welcome" || finished}><Logo size={20} /></span>
    <span
      class="progress"
      class:hidden={step === "welcome" || finished}
      role="progressbar"
      aria-label="Setup"
      aria-valuemin={1}
      aria-valuemax={steps.length}
      aria-valuenow={index + 1}
      aria-valuetext={steps[index]?.name}
    >
      <span class="bar" style:--p={(index + 1) / steps.length}></span>
    </span>
    {#if rerun}<button class="ghost skip" onclick={() => openPage("settings")}>Cancel</button>{/if}
    <WindowControls />
  </header>

  <main class="stage stack">
    {#key finished ? "done" : step}
      <div class="view" style:--from="{dir * 28}px" out:leave={{ x: -dir * 20 }}>
        {#if finished}
          <div class="page center">
            {@render ring(96, 1, "rec")}
            <h1>Klipp is recording</h1>
            <p class="lead">Press <kbd>{c.hotkey}</kbd> to save the last {fmtLength(c.length)}.</p>
          </div>
        {:else if step === "welcome"}
          <div class="page center">
            {@render ring(88, 1, "draw")}
            <h1 class="big">{rerun ? "Set up Klipp again" : "Welcome to Klipp"}</h1>
            <p class="lead">Klipp keeps the last moments of your screen ready to save.</p>
            <ul class="promises">
              <li title="A few MB of memory besides the clip buffer"><Icon name="bolt" size={15} />Light</li>
              <li><Icon name="drive" size={15} />Stays on your PC, no account</li>
              <li title="Nothing touches your games"><Icon name="crosshair" size={15} />Safe with anti-cheat</li>
            </ul>
            <button class="primary go" onclick={next}>Get started</button>
            <p class="eta">About a minute</p>
          </div>
        {:else if step === "check"}
          <div class="stack fill">
            {#if checkError}
              <div class="page center" in:pageIn out:pageOut>
                <div class="seal bad">{@render ring(72, 1, "bad")}<Icon name="warning" size={24} /></div>
                <h1>Couldn't check this PC</h1>
                <p class="lead selectable">{checkError}</p>
                {@render help()}
              </div>
            {:else if !revealed}
              <div class="page center" in:pageIn out:pageOut>
                <div class="seal">
                  {@render ring(136, meter, "live")}
                  <span class="pct mono">{Math.round(meter * 100)}<small>%</small></span>
                </div>
                <h1>Testing your PC</h1>
                <div class="stack phase" aria-live="polite">
                  {#key testPhase}
                    <p class="lead" in:enter={{ y: 6 }} out:leave={{ y: -6 }}>{testPhase}</p>
                  {/key}
                </div>
                <p class="eta" class:hidden={!!check}>{pcOnly ? "New in this version. " : ""}About 20 seconds</p>
              </div>
            {:else if check}
              <div class="page center result" in:pageIn out:pageOut>
                {#if canRecord}
                  <div class="seal ok">{@render ring(72, 1)}<Icon name="check" size={24} /></div>
                  <h1>Ready to record</h1>
                  <p class="choice">
                    <span class="by">{preset === picked ? "Picked for your PC" : "Your choice"}</span>
                    <b>{presetName}</b>
                    <span class="mono">{sizeName(c.resolution)} · {c.fps} fps · {c.codec === "av1" ? "AV1" : "H.264"}</span>
                  </p>
                {:else}
                  <div class="seal bad">{@render ring(72, 1, "bad")}<Icon name="warning" size={24} /></div>
                  <h1>This PC can't record yet</h1>
                  <p class="lead">
                    {check.encoder
                      ? "Klipp couldn't start this card's video encoder. A newer graphics driver often fixes that."
                      : "This graphics card has no video encoder Klipp can use. Klipp records with NVIDIA, AMD and Intel graphics."}
                  </p>
                  {@render help()}
                {/if}
                <dl class="facts">
                  {@render fact("Graphics card", `${check.recordingGpu ?? check.gpus[0]?.name ?? "Not found"}${check.encoder ? ` · ${check.encoder}` : ""}`, canRecord ? "" : "bad")}
                  {#if canRecord}
                    {@render fact(
                      "Encoding",
                      encodes("av1", nativeH) ? "AV1, small files at high quality" : "H.264",
                      encodes("av1", nativeH) ? "" : "warn",
                      encodes("av1", nativeH) ? "" : "AV1 needs a GeForce RTX 40, Radeon RX 7000 or 700M, Intel Arc or newer",
                      48,
                    )}
                  {/if}
                  {#if check.monitor}
                    {@render fact("Screen", `${check.monitor.width} × ${check.monitor.height} at ${check.monitor.refreshHz} Hz`, "", "", 96)}
                  {/if}
                  {@render fact("Memory", `${Math.round(check.ramMb / 1024)} GB${check.ramMb >= 8000 ? "" : ", so the buffer starts at 30 seconds"}`, check.ramMb >= 8000 ? "" : "warn", "", 144)}
                  {#if check.freeGb != null}
                    {@render fact("Storage", `${check.freeGb} GB free`, check.freeGb >= 20 ? "" : "warn", `For clips in ${check.clipsDir}`, 192)}
                  {/if}
                  {#if games}
                    {@render fact("Auto-clips", games, "", "Kills, goals, steals and more can be saved by themselves", 240)}
                  {/if}
                </dl>
                {#if integrated}<p class="eta">Built-in graphics: 1080p is suggested so recording keeps up at 60 fps.</p>{/if}
              </div>
            {/if}
          </div>
        {:else if step === "audio"}
          <div class="page">
            <div class="title">
              <h1>Audio</h1>
              <p class="lead">Each source is its own track, so you can turn a loud teammate down later.</p>
            </div>
            {#if sonarFound}
              <div class="row">
                <span class="what wide"><b>SteelSeries Sonar found</b><span>Record Sonar's Game, Chat and Mic as three separate tracks.</span></span>
                <Toggle checked={c.sonar} label="Use Sonar's tracks" onchange={(v) => ((c.sonar = v), (touchedAudio = true))} />
              </div>
            {/if}
            {#if !c.sonar}
              <label class="row">
                <span class="what"><b>What you hear</b><span>Game and desktop sound</span></span>
                <select bind:value={c.output}>
                  <option value="">Windows default</option>
                  {#each playback as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
              {#if !router}
                <div class="row" title="Voice chat on its own track, to turn it down later or leave it out when you share">
                  <span class="what wide"><b>Separate Discord and Windows sounds <span class="exp">Experimental</span></b></span>
                  <Toggle checked={c.discord} label="Try separating Discord and Windows sounds" onchange={(v) => (c.discord = v)} />
                </div>
              {/if}
            {/if}
            <div class="row">
              <span class="what"><b>Microphone</b><span>Your voice</span></span>
              {#if c.mic && !c.sonar}
                <select aria-label="Microphone" bind:value={c.micDevice} in:enter={{ y: 0, duration: DUR.base }}>
                  <option value="">Windows default</option>
                  {#each recording as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              {:else}
                <span></span>
              {/if}
              <Toggle checked={c.mic} label="Record the microphone" onchange={(v) => (c.mic = v)} />
            </div>
            <div class="row">
              <span class="what" title="A short sound when a clip is saved, so you know it worked"><b>Save sound</b><span>{c.sonar ? "Pick your headset, not a Sonar device" : "Klipp keeps it out of your clips"}</span></span>
              {#if c.sound}
                <select aria-label="Save sound plays on" bind:value={c.cue} in:enter={{ y: 0, duration: DUR.base }}>
                  <option value="">Windows default</option>
                  {#each playback as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              {:else}
                <span></span>
              {/if}
              <Toggle checked={c.sound} label="Play a sound when a clip is saved" onchange={(v) => (c.sound = v)} />
            </div>
            {#if c.sound}
              <div class="row sub" in:enter={{ y: -4, duration: DUR.base }}>
                <span class="what"><span>Volume</span></span>
                <span class="ctl">
                  <Slider min={0} max={1} step={0.05} value={c.volume} label="Save sound volume" oninput={(v) => (c.volume = v)} />
                  <span class="value mono">{Math.round(c.volume * 100)}%</span>
                </span>
                <button class="icon ghost" aria-label="Play the save sound" title="Play it" onclick={() => api.playSound(c.cue, c.volume).catch((e) => toast(`Couldn't play it: ${e}`))}
                  ><Icon name="play" size={15} /></button
                >
              </div>
            {/if}
          </div>
        {:else if step === "video"}
          <div class="page wide">
            <div class="title">
              <h1>Video</h1>
              <p class="lead">Picked for your PC. Change it if you like.</p>
            </div>
            <div class="split">
              <div class="side stack">
                {#key tune}
                  <div class="pane" in:enter={{ x: tune ? 16 : -16, y: 0 }} out:leave={{ x: tune ? -12 : 12 }}>
                    {#if !tune}
                      <div class="presets" role="radiogroup" aria-label="Preset">
                        {#each PRESETS as p (p.id)}
                          {@const v = presetValues(p.id, screen)}
                          <button class="preset" class:on={preset === p.id} role="radio" aria-checked={preset === p.id} onclick={() => applyPreset(p.id)}>
                            <span class="pname">{p.name}</span>
                            {#if picked === p.id}<span class="best">Best fit</span>{/if}
                            <span class="pspec mono">{sizeName(v.resolution)} · {v.fps} fps</span>
                          </button>
                        {/each}
                        {#if preset === "custom"}
                          <div class="preset on custom" role="radio" aria-checked="true" aria-disabled="true">
                            <span class="pname">Custom</span>
                            <span class="pspec">Your own mix</span>
                          </div>
                        {/if}
                      </div>
                      <p class="phint">{preset === "custom" ? "Your own mix of the fine-tuning settings." : PRESETS.find((p) => p.id === preset)?.hint}</p>
                      <button class="ghost link" onclick={() => (tune = true)}><Icon name="sliders" size={15} />Fine-tune</button>
                    {:else}
                      <button class="ghost link top" onclick={() => (tune = false)}><Icon name="back" size={15} />Presets</button>
                      <div class="tuned" title={c.resolution === "native" ? `Your screen's size, ${check?.monitor?.width ?? ""} × ${nativeH}` : "Scaled down: smaller files and less memory"}>
                        <span class="name">Resolution</span>
                        <Segmented
                          value={c.resolution}
                          label="Resolution"
                          options={resolutions.map((r) => [r, r === "native" ? `Native${encodes("av1", heightOf(r)) || encodes("h264", heightOf(r)) ? "" : " ✕"}` : r] as const)}
                          onchange={(v) => (c.resolution = v)}
                        />
                      </div>
                      <div class="tuned" title={refresh < 120 ? `Your screen runs at ${refresh} Hz, so 120 fps isn't offered` : "60 is smooth; 120 doubles the detail in fast moments"}>
                        <span class="name">Frame rate</span>
                        <Segmented
                          value={c.fps}
                          label="Frame rate"
                          options={refresh >= 120 ? [[30, "30 fps"], [60, "60 fps"], [120, "120 fps"]] : [[30, "30 fps"], [60, "60 fps"]]}
                          onchange={(v) => (c.fps = v)}
                        />
                      </div>
                      <div class="tuned" title={canAv1 ? "AV1 makes files about half the size" : "Your graphics card records H.264"}>
                        <span class="name">Codec</span>
                        <Segmented
                          value={c.codec}
                          label="Codec"
                          options={canAv1 ? [["av1", "AV1"], ["h264", "H.264"]] : [["h264", "H.264"]]}
                          onchange={(v) => (c.codec = v)}
                        />
                      </div>
                    {/if}
                  </div>
                {/key}
              </div>
              <div class="cost">
                {#if pcTest}
                  <SystemMeters test={pcTest} choice={c} lengthSecs={c.length} {screen} audioTracks={1 + sources().length} />
                {:else}
                  <p class="estimate"><Icon name="info" size={16} />In games this is about <b class="mono">{perMinute} MB</b> per minute of clips.</p>
                {/if}
                {#if (check?.monitors.length ?? 0) > 1}
                  <label class="tuned" title="The one your games run on">
                    <span class="name">Screen</span>
                    <select bind:value={c.monitor}>
                      <option value="">Main display ({check?.monitors[0]?.name})</option>
                      {#each check?.monitors ?? [] as m (m.id)}<option value={m.id}>{m.name} · {m.width} × {m.height}</option>{/each}
                    </select>
                  </label>
                {/if}
              </div>
            </div>
          </div>
        {:else if step === "saving"}
          <div class="page">
            <div class="title">
              <h1>Saving clips</h1>
              <p class="lead">One key saves the last moments, from anywhere, even in fullscreen games.</p>
            </div>
            <div class="key">
              <HotkeyInput big phase={keyPhase} value={c.hotkey} onchange={(h) => (c.hotkey = h)} />
              <span class="what"
                ><b>Shortcut</b><span class:bad={keyBad} aria-live="polite">{keyNote || "Test it to check it reaches Klipp."}</span></span
              >
              <HotkeyTest hotkey={c.hotkey} bind:note={keyNote} bind:bad={keyBad} bind:phase={keyPhase} cue={c.cue} volume={c.volume} sound={c.sound} />
            </div>
            {#if drives.length > 0}
              <label class="row">
                <span class="what">
                  <b>Save clips on</b>
                  <span>
                    {chosenDrive ? `Klipp makes a folder there: ${chosenDrive.folder}` : ""}
                    {driveChanged ? " Clips already saved stay where they are; Settings → Storage can move them." : ""}
                  </span>
                </span>
                <select class="both" bind:value={drive}>
                  {#each drives as d (d.letter)}
                    <option value={d.letter}>{d.letter} {d.label || (d.system ? "Windows" : "Drive")} · {d.freeGb} GB free</option>
                  {/each}
                </select>
              </label>
            {/if}
            <div class="row">
              <span class="what"><b>Clip length</b><span>Kept in memory: about {ramForBuffer} MB in games</span></span>
              <span class="ctl both">
                <Slider min={10} max={300} step={5} value={c.length} label="Clip length" oninput={(v) => (c.length = v)} />
                <span class="value mono">{fmtLength(c.length)}</span>
              </span>
            </div>
            {#if check?.cs2}
              <div class="row">
                <span class="what wide"><b>CS2 auto-clips</b><span>Headshots and multi-kills, saved by themselves</span></span>
                <Toggle checked={c.cs2} label="CS2 auto-clips" onchange={(v) => (c.cs2 = v)} />
              </div>
            {/if}
            <div class="row">
              <span class="what wide"><b>Start with Windows</b><span>So it's recording when the moment happens</span></span>
              <Toggle checked={c.autostart} label="Start with Windows" onchange={(v) => (c.autostart = v)} />
            </div>
          </div>
        {/if}
      </div>
    {/key}
  </main>

  <!-- Always there, so the page doesn't shift when its buttons arrive. -->
  <footer>
    {#if step !== "welcome" && !finished}
      <div class="actions" in:enter={{ y: 8 }} out:leave>
        {#if index > 0}<button class="ghost" onclick={back}><Icon name="back" size={16} />Back</button>{/if}
        {#if pcOnly && step === "check"}<button class="ghost" onclick={() => finishPc(false)}>Skip</button>{/if}
        <span class="grow"></span>
        {#if (step === "saving" || (pcOnly && step === "video")) && !canRecord}
          <span class="cant"><Icon name="warning" size={15} />This PC can't record yet</span>
        {/if}
        {#if pcOnly && step === "video"}
          <button class="ghost" disabled={finishing} onclick={() => finishPc(false)}>Keep my settings</button>
          <button class="primary go" disabled={finishing || !canRecord} onclick={() => finishPc(true)}>Use these settings</button>
        {:else if step === "saving"}
          <button class="primary go" disabled={finishing || !canRecord} onclick={finish}>
            <Icon name="record" size={17} />{finishing ? "Starting…" : "Start recording"}
          </button>
        {:else}
          <button class="primary go" class:hidden={step === "check" && !check && !checkError} disabled={step === "check" && !check && !checkError} onclick={next}>Continue</button>
        {/if}
      </div>
    {/if}
  </footer>
</div>

<style>
  .setup {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  /* Warmth behind the welcome only; it goes as the work starts. */
  .glow {
    position: absolute;
    inset: 0;
    background: radial-gradient(60% 60% at 50% 34%, var(--accent-soft), transparent 70%);
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--dur-slow) var(--ease-out);
  }

  .glow.on {
    opacity: 0.55;
    transition-duration: calc(var(--dur-slow) * 3);
  }

  header {
    position: relative;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    height: 44px;
    padding-left: var(--sp-5);
  }

  .brand {
    margin-right: auto;
    pointer-events: none;
    transition: opacity var(--dur) var(--ease-out);
  }

  .hidden,
  button.hidden:disabled {
    opacity: 0;
  }

  .skip {
    margin-right: var(--sp-2);
    font-size: var(--fs-control);
    color: var(--muted);
  }

  header > :global(.controls) {
    align-self: flex-start;
  }

  /* The one sign of how far along: a slim line that glides. */
  .progress {
    position: absolute;
    top: 21px;
    left: calc(50% - 80px);
    width: 160px;
    height: 3px;
    overflow: hidden;
    border-radius: var(--r-full);
    background: var(--surface-2);
    pointer-events: none;
    transition: opacity var(--dur) var(--ease-out);
  }

  .bar {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--text);
    transform-origin: left;
    transform: scaleX(var(--p));
    transition: transform calc(var(--dur-spring) * 1.5) var(--ease-out);
  }

  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    grid-template-rows: minmax(0, 1fr);
  }

  .view {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden auto;
    padding: 0 var(--sp-6);
  }

  .fill {
    flex: 1;
  }

  .page {
    --col: 640px;
    width: 100%;
    max-width: var(--col);
    margin: clamp(var(--sp-3), calc(16vh - 64px), 72px) auto var(--sp-3);
  }

  .page.wide {
    --col: 900px;
  }

  .page.center {
    --col: 720px;
    display: flex;
    flex-direction: column;
    align-items: center;
    margin: auto;
    text-align: center;
  }

  /* Steps arrive from the way you're going, one piece just after another,
     as the old one finishes leaving. */
  .view .page > :global(*) {
    animation: step-in var(--dur-slow) var(--ease-out) 70ms backwards;
  }

  .view .page > :global(:nth-child(2)) {
    animation-delay: 100ms;
  }
  .view .page > :global(:nth-child(3)) {
    animation-delay: 130ms;
  }
  .view .page > :global(:nth-child(4)) {
    animation-delay: 160ms;
  }
  .view .page > :global(:nth-child(5)) {
    animation-delay: 190ms;
  }
  .view .page > :global(:nth-child(n + 6)) {
    animation-delay: 220ms;
  }

  /* The test's answer comes up in place: nothing is travelling. */
  .page.result {
    --from: 0px;
  }

  @keyframes step-in {
    from {
      opacity: 0;
      transform: translateX(var(--from, 28px));
    }
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--fs-2xl);
    font-weight: 700;
    letter-spacing: -0.03em;
    line-height: 1.15;
  }

  h1.big {
    font-size: 38px;
  }

  .lead {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 1.5;
  }

  .selectable {
    user-select: text;
  }

  .title {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-bottom: var(--sp-5);
  }

  .center h1 {
    margin-top: var(--sp-5);
  }

  .center .lead {
    margin-top: var(--sp-2);
    max-width: 480px;
  }

  .eta {
    margin: var(--sp-3) 0 0;
    color: var(--faint);
    font-size: var(--fs-sm);
  }

  /* The ring. */
  .ring {
    display: block;
    overflow: visible;
  }

  .track,
  .arc {
    fill: none;
    stroke-width: 12;
    stroke-linecap: round;
  }

  .track {
    stroke: var(--surface-2);
  }

  .arc {
    stroke: var(--text);
    stroke-dasharray: 100;
  }

  .dot {
    fill: var(--accent);
    transform-box: fill-box;
    transform-origin: center;
  }

  .ring.bad .arc {
    stroke: var(--danger);
  }

  .ring.bad .dot {
    fill: var(--danger);
  }

  /* Welcome: the ring draws itself, the dot lands, then it breathes. */
  .ring.draw .arc {
    animation: ring-draw calc(var(--dur-spring) * 2) var(--ease-out) backwards;
  }

  .ring.draw .dot {
    animation:
      dot-land var(--dur-spring) var(--spring) calc(var(--dur-spring) * 1.2) backwards,
      breathe 3.2s ease-in-out calc(var(--dur-spring) * 3) infinite;
  }

  .ring.live .dot,
  .ring.rec .dot {
    animation: breathe 1.4s ease-in-out infinite;
  }

  @keyframes ring-draw {
    from {
      stroke-dashoffset: 100;
    }
  }

  @keyframes dot-land {
    from {
      opacity: 0;
      transform: scale(0.2);
    }
  }

  @keyframes breathe {
    50% {
      opacity: 0.45;
      transform: scale(0.82);
    }
  }

  .promises {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--sp-2) var(--sp-5);
    list-style: none;
    margin: var(--sp-5) 0 0;
    padding: 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .promises li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .center > .go {
    margin-top: var(--sp-6);
  }

  .go {
    min-width: 148px;
    padding: 10px var(--sp-5);
  }

  /* The PC test. */
  .seal {
    position: relative;
    display: grid;
    place-items: center;
  }

  .seal > :global(*) {
    grid-area: 1 / 1;
  }

  .seal.ok {
    color: var(--good);
  }

  .seal.bad {
    color: var(--danger);
  }

  .seal.ok .track,
  .seal.ok .arc {
    stroke: var(--text);
  }

  .pct {
    font-size: var(--fs-xl);
    font-weight: 600;
    color: var(--text);
  }

  .pct small {
    margin-left: 1px;
    font-size: var(--fs-sm);
    font-weight: 400;
    color: var(--faint);
  }

  .phase {
    margin-top: var(--sp-1);
    min-height: 26px;
  }

  .choice {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--sp-1) var(--sp-3);
    margin: var(--sp-3) 0 0;
    padding: var(--sp-2) var(--sp-4);
    border-radius: var(--r-full);
    background: var(--surface);
    font-size: var(--fs-control);
  }

  .choice .by {
    color: var(--faint);
  }

  .choice b {
    font-size: var(--fs-md);
    font-weight: 650;
  }

  .choice .mono {
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--sp-4) var(--sp-5);
    width: 100%;
    margin: var(--sp-6) 0 0;
    text-align: left;
  }

  .fact {
    min-width: 0;
  }

  .fact dt {
    margin-bottom: 2px;
    color: var(--faint);
    font-size: var(--fs-sm);
  }

  .fact dd {
    display: flex;
    align-items: baseline;
    gap: var(--sp-1);
    margin: 0;
    font-size: var(--fs-control);
    line-height: 1.4;
  }

  .fact dd :global(.icon) {
    flex: none;
    align-self: center;
  }

  .fact.warn dd {
    color: var(--warn);
  }

  .fact.bad dd {
    color: var(--danger);
  }

  .help {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2) var(--sp-3);
    margin: var(--sp-4) 0 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .copy {
    gap: var(--sp-2);
    font-size: var(--fs-control);
  }

  /* Rows: a name, a control, a switch. Selects and switches line up down the page. */
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 272px 38px;
    align-items: center;
    gap: var(--sp-4);
    min-height: 52px;
    padding: var(--sp-1) 0;
  }

  .row.sub {
    min-height: 40px;
    margin-top: calc(var(--sp-1) * -1);
  }

  .row .wide {
    grid-column: 1 / 3;
  }

  .row .both {
    grid-column: 2 / 4;
  }

  .row select {
    width: 100%;
    min-width: 0;
  }

  .row button.icon {
    width: 34px;
    justify-self: end;
    margin-right: 2px;
  }

  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .what b {
    font-weight: 600;
  }

  .what span {
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .what span.bad {
    color: var(--danger);
  }

  .what b .exp {
    margin-left: var(--sp-1);
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .ctl {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-width: 0;
  }

  .ctl :global(.slider) {
    flex: 1;
    min-width: 0;
  }

  .value {
    flex: none;
    min-width: 40px;
    text-align: right;
    font-size: var(--fs-sm);
    color: var(--muted);
  }

  /* The key, the star of its step. */
  .key {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    margin-bottom: var(--sp-3);
    padding: var(--sp-3) var(--sp-4) var(--sp-3) var(--sp-3);
    border-radius: var(--r-md);
    background: var(--sunken);
  }

  .key .what {
    flex: 1;
  }

  /* Video: the choice on the left, what it costs on the right, side by side so
     the meters answer as you click. */
  .split {
    display: grid;
    grid-template-columns: 304px minmax(0, 1fr);
    align-items: start;
    gap: var(--sp-5);
  }

  .cost {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
    min-width: 0;
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    min-width: 0;
  }

  .presets {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .preset {
    justify-content: flex-start;
    gap: var(--sp-2);
    min-height: 40px;
    padding: var(--sp-2) var(--sp-3);
    background: transparent;
    text-align: left;
  }

  .preset:hover {
    background: var(--hover);
  }

  .preset.on,
  .preset.on:hover {
    background: var(--surface-3);
  }

  .preset.on .pname {
    color: var(--text);
  }

  .preset.custom {
    display: flex;
    align-items: center;
    border-radius: var(--r-sm);
    cursor: default;
  }

  .pname {
    font-weight: 600;
    font-size: var(--fs-control);
    color: var(--muted);
  }

  .best {
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .pspec {
    margin-left: auto;
    color: var(--faint);
    font-size: var(--fs-xs);
  }

  .phint {
    margin: 0;
    padding: 0 var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }

  .link {
    align-self: flex-start;
    padding: 5px var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .link.top {
    margin-left: calc(var(--sp-3) * -1);
    margin-bottom: calc(var(--sp-1) * -1);
  }

  .tuned {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }

  .tuned .name {
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .tuned select {
    width: 100%;
    max-width: 320px;
  }

  .tuned :global(.segmented) {
    display: grid;
    width: 100%;
  }

  .estimate {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .estimate b {
    color: var(--text);
    font-weight: 500;
  }

  footer {
    position: relative;
    flex-shrink: 0;
    height: 72px;
    padding: var(--sp-3) var(--sp-5) var(--sp-4);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 100%;
  }

  .grow {
    flex: 1;
  }

  .cant {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    margin-right: var(--sp-2);
    color: var(--danger);
    font-size: var(--fs-control);
  }
</style>
