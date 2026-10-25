<script lang="ts">
  // First-run setup: checks what this PC can record, then asks for audio,
  // video and saving choices with the best ones preselected, and starts
  // recording with them. Also reachable from Settings ("Run setup again").
  import { onMount } from "svelte";
  import * as api from "../api";
  import type { AudioDevice, Drive, SystemCheck } from "../api";
  import HotkeyInput from "../HotkeyInput.svelte";
  import HotkeyTest from "../HotkeyTest.svelte";
  import { DISCORD, routerInstalled } from "../discordTrack";
  import Icon from "../Icon.svelte";
  import Logo from "../Logo.svelte";
  import { library, openPage, refreshRecorder, reload, toast } from "../state.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import WindowControls from "../WindowControls.svelte";

  let { rerun = false }: { rerun?: boolean } = $props();

  const steps = [
    { id: "welcome", name: "Welcome" },
    { id: "check", name: "Your PC" },
    { id: "audio", name: "Audio" },
    { id: "video", name: "Video" },
    { id: "saving", name: "Saving clips" },
  ] as const;
  type Step = (typeof steps)[number]["id"];
  let step = $state<Step>("welcome");
  let index = $derived(steps.findIndex((s) => s.id === step));

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
  /** The save key test's result, shown under the key. */
  let keyNote = $state("");
  let keyBad = $state(false);

  // Choices, preselected once the check is done.
  let c = $state({
    sonar: false,
    output: "",
    chat: false,
    chatDevice: "",
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
    // show the full name it stands for, as Settings → Sounds does.
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

  function recommend(r: SystemCheck) {
    const h = r.monitor?.height ?? 1080;
    const gpu = r.gpus.find((g) => g.name === r.recordingGpu);
    const builtIn = !!gpu && gpu.vendor !== "nvidia" && gpu.vramMb < 1024;
    // 4K and up records at 1440p by default: sharp, at a third of the memory.
    // Built-in graphics record 1080p, which they can keep up with at 60 fps.
    c.resolution = builtIn && h > 1080 ? "1080p" : h > 1440 ? "1440p" : "native";
    c.fps = 60;
    // The probe recorded a few seconds at each candidate: its pick runs
    // smoothly with most of the card left for a game. 120 fps needs about
    // twice the memory for the buffer, so not on PCs with little of it.
    if (r.recommended) {
      c.resolution = r.recommended.height === h ? "native" : (`${r.recommended.height}p` as typeof c.resolution);
      c.fps = r.recommended.fps === 120 && r.ramMb >= 12000 ? 120 : 60;
    }
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
    if (index < steps.length - 1) step = steps[index + 1].id;
  }
  function back() {
    if (index > 0) step = steps[index - 1].id;
  }

  function sources() {
    if (c.sonar) {
      return [
        { name: "Game", device: "Sonar - Gaming", kind: "loopback" },
        { name: "Chat", device: "Sonar - Chat", kind: "loopback" },
        ...(c.mic ? [{ name: "Mic", device: "Sonar - Microphone", kind: "capture" }] : []),
      ];
    }
    return [
      { name: c.chat || c.discord ? "Game" : "PC audio", device: c.output, kind: "loopback" },
      ...(c.discord && !router ? [{ name: "Chat", device: "", kind: DISCORD }] : []),
      ...(c.chat && !c.discord ? [{ name: "Chat", device: c.chatDevice, kind: "loopback" }] : []),
      ...(c.mic ? [{ name: "Mic", device: c.micDevice, kind: "capture" }] : []),
    ];
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
        { key: ["video", "codec"], value: c.codec },
        { key: ["video", "resolution"], value: c.resolution },
        { key: ["video", "fps"], value: c.fps },
        { key: ["video", "monitor"], value: c.monitor },
        { key: ["replay", "hotkey"], value: c.hotkey },
        { key: ["replay", "length_secs"], value: c.length },
        { key: ["auto_clip", "cs2", "enabled"], value: c.cs2 },
      ]);
      await api.setAutostart(c.autostart).catch(() => {});
      await api.startRecorder();
      setTimeout(refreshRecorder, 1500);
      await reload();
      toast(`Klipp is recording. Press ${c.hotkey} to save the last ${c.length} seconds.`);
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

<div class="setup">
  <header data-tauri-drag-region>
    <span class="brand"><Logo size={22} /></span>
    {#if rerun}<button class="ghost skip" onclick={() => openPage("settings")}>Cancel</button>{/if}
    <WindowControls />
  </header>

  <div class="body">
    <ol class="rail">
      {#each steps as s, i (s.id)}
        <li class:done={i < index} class:current={i === index}>
          <span class="dot">{#if i < index}<Icon name="check" size={13} />{:else}{i + 1}{/if}</span>
          {s.name}
        </li>
      {/each}
    </ol>

    <section class="page">
      {#key step}
        <div class="content">
          {#if step === "welcome"}
            <div class="hero-mark"><Logo size={44} /></div>
            <h1>{rerun ? "Set up Klipp again" : "Welcome to Klipp"}</h1>
            <p class="lead">
              Klipp keeps the last moments of your screen ready to save. Let's check what your PC can record and choose your audio. It takes about a minute.
            </p>
            <ul class="promises">
              <li><Icon name="bolt" size={18} />Light: a few MB of memory besides the clip buffer</li>
              <li><Icon name="drive" size={18} />Everything stays on your PC, no account</li>
              <li><Icon name="crosshair" size={18} />Safe with anti-cheat: nothing touches your games</li>
            </ul>
          {:else if step === "check"}
            <h1>Your PC</h1>
            <p class="lead">Klipp tested your graphics card's encoders for real, at your screen's size.</p>
            {#if checkError}
              <p class="bad"><Icon name="warning" size={18} />Couldn't check this PC: {checkError}</p>
              <button class="copy" onclick={copyDetails}><Icon name="copy" size={15} />Copy details</button>
            {:else if !check}
              <div class="checking"><span class="spinner"></span>Checking your graphics card…</div>
            {:else}
              <ul class="checks">
                <li class:ok={canRecord} class:bad={!canRecord}>
                  <span class="mark"><Icon name={canRecord ? "check" : "warning"} size={16} /></span>
                  <div>
                    <b>Graphics card</b>
                    <span>{check.recordingGpu ?? check.gpus[0]?.name ?? "Not found"}{check.encoder ? ` · ${check.encoder}` : ""}</span>
                    {#if !canRecord}
                      <span class="note">
                        {check.encoder
                          ? "Klipp couldn't start this card's video encoder. A newer graphics driver often fixes that."
                          : "This graphics card has no video encoder Klipp can use. Klipp records with NVIDIA, AMD and Intel graphics."}
                      </span>
                    {/if}
                  </div>
                </li>
                {#if canRecord}
                  <li class:ok={encodes("av1", nativeH)} class:warn={!encodes("av1", nativeH)}>
                    <span class="mark"><Icon name={encodes("av1", nativeH) ? "check" : "info"} size={16} /></span>
                    <div>
                      <b>Video encoding</b>
                      <span>
                        {encodes("av1", nativeH)
                          ? "AV1: small files at high quality"
                          : "H.264 (AV1 needs a GeForce RTX 40, Radeon RX 7000 or 700M, Intel Arc or newer)"}
                      </span>
                      {#if integrated}<span>Built-in graphics: 1080p is suggested so recording keeps up at 60 fps.</span>{/if}
                    </div>
                  </li>
                {/if}
                {#if check.monitor}
                  <li class="ok">
                    <span class="mark"><Icon name="check" size={16} /></span>
                    <div><b>Screen</b><span>{check.monitor.width} × {check.monitor.height} at {check.monitor.refreshHz} Hz</span></div>
                  </li>
                {/if}
                <li class:ok={check.ramMb >= 8000} class:warn={check.ramMb < 8000}>
                  <span class="mark"><Icon name={check.ramMb >= 8000 ? "check" : "info"} size={16} /></span>
                  <div>
                    <b>Memory</b>
                    <span>{Math.round(check.ramMb / 1024)} GB{check.ramMb >= 8000 ? ", plenty for a 1-minute buffer" : ", so the buffer starts at 30 seconds"}</span>
                  </div>
                </li>
                {#if check.freeGb != null}
                  <li class:ok={check.freeGb >= 20} class:warn={check.freeGb < 20}>
                    <span class="mark"><Icon name={check.freeGb >= 20 ? "check" : "info"} size={16} /></span>
                    <div><b>Storage</b><span>{check.freeGb} GB free for clips in {check.clipsDir}</span></div>
                  </li>
                {/if}
                {#if check.cs2}
                  <li class="ok">
                    <span class="mark"><Icon name="check" size={16} /></span>
                    <div><b>Counter-Strike 2</b><span>Found: headshots and multi-kills can be saved automatically</span></div>
                  </li>
                {/if}
                {#if check.league}
                  <li class="ok">
                    <span class="mark"><Icon name="check" size={16} /></span>
                    <div><b>League of Legends</b><span>Found: multi-kills, first bloods and steals can be saved automatically</span></div>
                  </li>
                {/if}
                {#if check.rocketLeague}
                  <li class="ok">
                    <span class="mark"><Icon name="check" size={16} /></span>
                    <div><b>Rocket League</b><span>Found: goals, epic saves and demolitions can be saved automatically</span></div>
                  </li>
                {/if}
              </ul>
              {#if !canRecord}
                <p class="help">Tell us about it and we'll look into it: <button class="copy" onclick={copyDetails}><Icon name="copy" size={15} />Copy details</button></p>
              {/if}
            {/if}
          {:else if step === "audio"}
            <h1>Audio</h1>
            <p class="lead">Each source is its own track, so you can turn a loud teammate down later.</p>
            {#if sonarFound}
              <div class="option" class:on={c.sonar}>
                <div>
                  <b>SteelSeries Sonar found</b>
                  <span>Record Sonar's Game, Chat and Mic as three separate tracks.</span>
                </div>
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
                <div class="row">
                  <span class="what"
                    ><b>Discord on its own track <span class="exp">Experimental</span></b><span
                      >Voice chat apart from the game, to turn it down later or leave it out when you share</span
                    ></span
                  >
                  <Toggle checked={c.discord} label="Discord on its own track" onchange={(v) => ((c.discord = v), v && (c.chat = false))} />
                </div>
              {/if}
              {#if !c.discord}
              <div class="row">
                <span class="what"><b>Separate chat track</b><span>If voice chat plays on its own device</span></span>
                <Toggle checked={c.chat} label="Separate chat track" onchange={(v) => (c.chat = v)} />
              </div>
              {/if}
              {#if c.chat && !c.discord}
                <label class="row sub">
                  <span class="what"><span>Chat device</span></span>
                  <select bind:value={c.chatDevice}>
                    <option value="">Windows default</option>
                    {#each playback as name (name)}<option value={name}>{name}</option>{/each}
                  </select>
                </label>
              {/if}
            {/if}
            <div class="row">
              <span class="what"><b>Microphone</b><span>Your voice, on its own track</span></span>
              <Toggle checked={c.mic} label="Record the microphone" onchange={(v) => (c.mic = v)} />
            </div>
            {#if c.mic && !c.sonar}
              <label class="row sub">
                <span class="what"><span>Microphone</span></span>
                <select bind:value={c.micDevice}>
                  <option value="">Windows default</option>
                  {#each recording as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
            {/if}
            <div class="row">
              <span class="what"><b>Save sound</b><span>A short sound when a clip is saved, so you know it worked</span></span>
              <Toggle checked={c.sound} label="Play a sound when a clip is saved" onchange={(v) => (c.sound = v)} />
            </div>
            {#if c.sound}
              <label class="row sub">
                <span class="what"
                  ><span>{c.sonar ? "Plays on: pick your headset, not a Sonar device" : "Plays on (Klipp keeps it out of your clips)"}</span></span
                >
                <select bind:value={c.cue}>
                  <option value="">Windows default</option>
                  {#each playback as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
              <div class="row sub">
                <span class="what"><span>Volume</span></span>
                <Slider min={0} max={1} step={0.05} value={c.volume} label="Save sound volume" oninput={(v) => (c.volume = v)} />
                <span class="value mono">{Math.round(c.volume * 100)}%</span>
                <button class="icon" aria-label="Play the save sound" title="Play it" onclick={() => api.playSound(c.cue, c.volume).catch((e) => toast(`Couldn't play it: ${e}`))}
                  ><Icon name="play" size={15} /></button
                >
              </div>
            {/if}
          {:else if step === "video"}
            <h1>Video</h1>
            <p class="lead">Preselected for your screen and graphics card. Options it can't record are marked.</p>
            {#if (check?.monitors.length ?? 0) > 1}
              <label class="row">
                <span class="what"><b>Screen</b><span>The one your games run on</span></span>
                <select bind:value={c.monitor}>
                  <option value="">Main display ({check?.monitors[0]?.name})</option>
                  {#each check?.monitors ?? [] as m (m.id)}<option value={m.id}>{m.name} · {m.width} × {m.height}</option>{/each}
                </select>
              </label>
            {/if}
            <div class="row">
              <span class="what"><b>Resolution</b><span>{c.resolution === "native" ? `Your screen's size, ${check?.monitor?.width ?? ""} × ${nativeH}` : "Scaled down: smaller files and less memory"}</span></span>
              <Segmented
                value={c.resolution}
                label="Resolution"
                options={resolutions.map((r) => [r, r === "native" ? `Native${encodes("av1", heightOf(r)) || encodes("h264", heightOf(r)) ? "" : " ✕"}` : r] as const)}
                onchange={(v) => (c.resolution = v)}
              />
            </div>
            <div class="row">
              <span class="what"><b>Frame rate</b><span>{refresh < 120 ? `Your screen runs at ${refresh} Hz, so 120 fps isn't offered` : "60 is smooth; 120 doubles the detail in fast moments"}</span></span>
              <Segmented
                value={c.fps}
                label="Frame rate"
                options={refresh >= 120 ? [[30, "30"], [60, "60"], [120, "120"]] : [[30, "30"], [60, "60"]]}
                onchange={(v) => (c.fps = v)}
              />
            </div>
            <div class="row">
              <span class="what"><b>Codec</b><span>{canAv1 ? "AV1 makes files about half the size" : "Your graphics card records H.264"}</span></span>
              <Segmented
                value={c.codec}
                label="Codec"
                options={canAv1 ? [["av1", "AV1"], ["h264", "H.264"]] : [["h264", "H.264"]]}
                onchange={(v) => (c.codec = v)}
              />
            </div>
            <div class="estimate">
              <Icon name="info" size={16} />
              <span>In games this is about <b class="mono">{perMinute} MB</b> per minute of clips.</span>
            </div>
          {:else if step === "saving"}
            <h1>Saving clips</h1>
            <p class="lead">One key saves the last moments, from anywhere, even in fullscreen games.</p>
            {#if drives.length > 0}
              <label class="row">
                <span class="what">
                  <b>Save clips on</b>
                  <span>
                    {chosenDrive ? `Klipp makes a folder there: ${chosenDrive.folder}` : ""}
                    {driveChanged ? " Clips already saved stay where they are; Settings → Storage can move them." : ""}
                  </span>
                </span>
                <select bind:value={drive}>
                  {#each drives as d (d.letter)}
                    <option value={d.letter}>{d.letter} {d.label || (d.system ? "Windows" : "Drive")} · {d.freeGb} GB free</option>
                  {/each}
                </select>
              </label>
            {/if}
            <div class="row">
              <span class="what">
                <b>Save hotkey</b>
                <span class:bad={keyBad}>{keyNote || "Press Test, then the key, to check it reaches Klipp."}</span>
              </span>
              <HotkeyInput value={c.hotkey} onchange={(h) => (c.hotkey = h)} />
              <HotkeyTest hotkey={c.hotkey} bind:note={keyNote} bind:bad={keyBad} cue={c.cue} volume={c.volume} sound={c.sound} />
            </div>
            <div class="row">
              <span class="what"><b>Clip length</b><span>Kept in memory: about {ramForBuffer} MB in games</span></span>
              <Slider min={10} max={300} step={5} value={c.length} label="Clip length" oninput={(v) => (c.length = v)} />
              <span class="value mono">{fmtLength(c.length)}</span>
            </div>
            {#if check?.cs2}
              <div class="row">
                <span class="what"><b>CS2 auto-clips</b><span>Headshots and multi-kills, saved by themselves</span></span>
                <Toggle checked={c.cs2} label="CS2 auto-clips" onchange={(v) => (c.cs2 = v)} />
              </div>
            {/if}
            <div class="row">
              <span class="what"><b>Start with Windows</b><span>So it's recording when the moment happens</span></span>
              <Toggle checked={c.autostart} label="Start with Windows" onchange={(v) => (c.autostart = v)} />
            </div>
          {/if}
        </div>
      {/key}

      <footer>
        {#if index > 0}<button class="ghost" onclick={back}><Icon name="back" size={16} />Back</button>{/if}
        <span class="grow"></span>
        {#if step === "saving"}
          <button class="primary go" disabled={finishing || !canRecord} onclick={finish}>
            <Icon name="record" size={17} />{finishing ? "Starting…" : "Start recording"}
          </button>
        {:else}
          <button class="primary go" disabled={step === "check" && !check && !checkError} onclick={next}>
            {step === "welcome" ? "Get started" : "Continue"}
          </button>
        {/if}
      </footer>
    </section>
  </div>
</div>

<style>
  .setup {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: radial-gradient(90% 70% at 85% 0%, var(--accent-soft), transparent 60%), var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    height: 44px;
    padding-left: 24px;
  }

  .brand {
    margin-right: auto;
    pointer-events: none;
  }

  .skip {
    margin-right: 8px;
    font-size: 13px;
    color: var(--muted);
  }

  header > :global(.controls) {
    align-self: flex-start;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 240px 1fr;
  }

  .rail {
    list-style: none;
    margin: 0;
    padding: 48px 0 0 32px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .rail li {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--faint);
    font-weight: 550;
  }

  .rail li.current {
    color: var(--text);
  }

  .rail li.done {
    color: var(--muted);
  }

  .dot {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 1.5px solid var(--line-strong);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .current .dot {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }

  .done .dot {
    border-color: var(--accent);
    color: var(--accent);
  }

  .page {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 36px 56px 28px 24px;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    max-width: 680px;
    animation: in 0.25s var(--ease);
  }

  h1 {
    margin: 0 0 10px;
    font-family: var(--font-display);
    font-size: 30px;
    letter-spacing: -0.03em;
  }

  .lead {
    margin: 0 0 26px;
    max-width: 560px;
    color: var(--muted);
    font-size: 15px;
    line-height: 1.55;
  }

  .hero-mark {
    margin: 10px 0 26px;
  }

  .promises {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .promises li {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .promises :global(.icon) {
    color: var(--accent);
  }

  .checking {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--muted);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--accent);
    border-right-color: transparent;
    animation: spin 0.8s linear infinite;
  }

  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .checks li {
    display: flex;
    gap: 14px;
    padding: 14px 16px;
    border-radius: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    animation: in 0.3s var(--ease) backwards;
  }

  .checks li:nth-child(2) {
    animation-delay: 0.06s;
  }
  .checks li:nth-child(3) {
    animation-delay: 0.12s;
  }
  .checks li:nth-child(4) {
    animation-delay: 0.18s;
  }
  .checks li:nth-child(5) {
    animation-delay: 0.24s;
  }
  .checks li:nth-child(6) {
    animation-delay: 0.3s;
  }

  .checks div,
  .what {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .checks span,
  .what span {
    color: var(--muted);
    font-size: 13px;
  }

  .what span.bad {
    color: var(--danger);
  }

  .what b .exp {
    margin-left: 6px;
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--accent-soft);
    color: var(--accent);
    font: 600 10px/1.4 var(--font-mono);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    vertical-align: 1px;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--surface-3);
  }

  .ok .mark {
    background: color-mix(in srgb, var(--good) 18%, transparent);
    color: var(--good);
  }

  .warn .mark {
    background: color-mix(in srgb, #f2c84b 18%, transparent);
    color: #f2c84b;
  }

  .bad .mark,
  p.bad {
    color: var(--danger);
  }

  .note {
    color: var(--text) !important;
  }

  .help {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 16px 0 0;
    color: var(--muted);
    font-size: 13px;
  }

  .copy {
    gap: 7px;
    align-self: flex-start;
    font-size: 13px;
  }

  p.bad + .copy {
    margin-top: 12px;
  }

  /* Rows share the option card's side padding, so labels and switches line
     up with the card's whether or not a card is above them. */
  .option,
  .row {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--line);
  }

  .option {
    padding: 14px 16px;
    margin-bottom: 8px;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--surface);
  }

  .option.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .option > div {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .option span {
    color: var(--muted);
    font-size: 13px;
  }

  .row .what {
    flex: 1;
    min-width: 0;
  }

  .row.sub {
    padding-top: 0;
  }

  .row select {
    width: 300px;
  }

  .value {
    min-width: 64px;
    text-align: right;
    font-size: 12px;
  }

  .row :global(.slider) {
    width: 200px;
  }

  .estimate {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 18px;
    color: var(--muted);
    font-size: 13px;
  }

  .estimate b {
    color: var(--text);
    font-weight: 500;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 680px;
    padding-top: 20px;
  }

  .grow {
    flex: 1;
  }

  .go {
    min-width: 170px;
    padding: 10px 18px;
  }

  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
