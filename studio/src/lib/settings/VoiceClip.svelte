<script lang="ts">
  // "Klipp that": the switch, and teaching it a voice. Teaching plays like the
  // key test in setup: the button is alive while it waits for you, and each of
  // the three takes lands where you can see it.
  import { onDestroy, onMount } from "svelte";
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import { DUR, enter, leave } from "../motion";
  import { toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const TAKES = [1, 2, 3];

  let voice = $state<api.VoiceSettings>({ enabled: false, personal: [] });
  let teaching = $state(false);
  let line = $state("");
  let takes = $state(0);
  let phase = $state<"idle" | "ready" | "listen" | "heard" | "retry" | "done" | "failed">("idle");
  /** Starts the take's countdown again, also for a second try at the same take. */
  let listens = $state(0);
  let stopListening: (() => void) | undefined;
  let settle = 0;

  const retries: Record<string, string> = {
    quiet: "Too quiet. Once more, a bit louder.",
    short: "Klipp only caught part of it. Say the whole phrase.",
    long: "That was more than the phrase. Say only “Klipp that”.",
  };

  function onStep(s: api.VoiceTeachStep) {
    if (s.step === "listen") {
      takes = (s.take ?? 1) - 1;
      listens += 1;
      phase = "listen";
      line = "Say “Klipp that” now";
    } else if (s.step === "heard") {
      takes = s.take ?? takes;
      phase = "heard";
      line = "Got it.";
    } else if (s.step === "retry") {
      phase = "retry";
      line = retries[s.why ?? ""] ?? "Once more.";
    } else if (s.step === "failed") {
      phase = "failed";
      line = s.why === "no microphone" ? "No microphone is set up. Add one in Settings → Audio." : "Klipp couldn't hear it. Check your microphone and try again.";
    } else if (s.step === "done") {
      takes = TAKES.length;
      phase = "done";
      line = "Klipp knows how you say it. It's listening now.";
      clearTimeout(settle);
      settle = window.setTimeout(() => (phase = "idle"), 2400);
    }
  }

  async function teach() {
    clearTimeout(settle);
    teaching = true;
    takes = 0;
    phase = "ready";
    line = "Get ready…";
    try {
      await api.voiceTeach();
      voice = await api.voiceSettings();
    } catch (e) {
      phase = "failed";
      line = "";
      toast(`Couldn't teach it: ${e}`);
    } finally {
      teaching = false;
      finished();
    }
  }

  /** The recorder went quiet: "done" and "failed" stay on show, anything else is over. */
  function finished() {
    if (phase !== "done" && phase !== "failed") phase = "idle";
  }

  function setEnabled(enabled: boolean) {
    voice.enabled = enabled;
    api.setVoiceEnabled(enabled).catch((e) => toast(`Couldn't change it: ${e}`));
  }

  onMount(async () => {
    voice = await api.voiceSettings();
    stopListening = await api.onVoiceTeach(onStep);
  });
  onDestroy(() => {
    clearTimeout(settle);
    stopListening?.();
  });

  let showTakes = $derived(phase !== "idle" && phase !== "failed");
  /** The take being said right now. */
  let live = $derived(phase === "listen" || phase === "retry" ? takes + 1 : 0);
  let rest = $derived(voice.personal.length ? "Done. Teach it again if it misses you." : "Say it three times, so it works with your accent.");
</script>

<Field title="Say “Klipp that”" badge="Test build" tip="Saves a clip when you say it." hint="Klipp listens on this PC only; nothing is kept or sent.">
  <Toggle checked={voice.enabled} label="Say Klipp that" onchange={setEnabled} />
</Field>
<Field title="Teach Klipp your voice" hint={line || rest}>
  {#if showTakes}
    <div class="takes" role="img" aria-label="{takes} of {TAKES.length}" in:enter={{ x: 8, y: 0, duration: DUR.base }} out:leave={{ x: 8 }}>
      {#each TAKES as n (n)}
        <span class="take" class:got={n <= takes} class:live={n === live} class:miss={n === live && phase === "retry"}>
          {#if n <= takes}<span class="tick"><Icon name="check" size={14} /></span>{:else}<span class="n mono">{n}</span>{/if}
        </span>
      {/each}
    </div>
  {/if}
  <button class="teach" class:listening={phase === "listen"} class:works={phase === "done"} disabled={teaching} onclick={teach}>
    {#if phase === "listen"}
      <span class="pulse"></span>Say it
      {#key listens}<span class="drain" aria-hidden="true"></span>{/key}
    {:else if phase === "ready"}
      <span class="spinner"></span>Ready
    {:else if phase === "heard"}
      <span class="tick"><Icon name="check" size={15} /></span>Got it
    {:else if phase === "retry"}
      Again
    {:else if phase === "done"}
      <span class="tick"><Icon name="check" size={15} /></span>Done
    {:else}
      {voice.personal.length || phase === "failed" ? "Teach again" : "Teach"}
    {/if}
  </button>
</Field>

<style>
  .takes {
    display: flex;
    gap: var(--sp-1);
  }

  .take {
    position: relative;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--faint);
    font-size: var(--fs-xs);
    transition:
      background var(--dur) var(--ease-out),
      color var(--dur-fast);
  }

  .take.got {
    background: var(--good-soft);
    color: var(--good);
  }

  .take.live {
    background: var(--accent-soft);
    color: var(--text);
  }

  .take.miss {
    background: var(--warn-soft);
  }

  /* The take you're on: it breathes until it's heard. */
  .take.live::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    box-shadow: inset 0 0 0 2px var(--accent);
    animation: breathe 1s var(--ease-out) infinite alternate;
  }

  .take.miss::after {
    box-shadow: inset 0 0 0 2px var(--warn);
    animation: none;
  }

  .teach {
    position: relative;
    min-width: 104px;
    overflow: hidden;
  }

  /* Waiting for you is the liveliest moment here, not a dimmed button. */
  .teach:disabled {
    opacity: 1;
  }

  .teach.listening {
    background: var(--accent-soft);
    color: var(--text);
  }

  .teach.works {
    background: var(--good-soft);
    color: var(--good);
  }

  .tick {
    display: inline-flex;
    animation: tick var(--dur-spring) var(--spring) backwards;
  }

  .pulse {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
    animation: breathe 1s var(--ease-out) infinite alternate;
  }

  /* The two and a half seconds the take lasts. */
  .drain {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    animation: drain 2.5s linear forwards;
  }

  @keyframes tick {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @keyframes breathe {
    to {
      opacity: 0.35;
      transform: scale(0.8);
    }
  }

  @keyframes drain {
    to {
      transform: scaleX(0);
    }
  }
</style>
