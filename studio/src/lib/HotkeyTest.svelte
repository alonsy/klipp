<script lang="ts">
  // Tries the save key for real: Test, then press it within 10 s. A key
  // another app holds, or an F key that needs Fn on a laptop, shows up here
  // instead of mid-game. Nothing is saved. The result goes to `note`, for the
  // line under the setting's name; `phase` lets the key itself react.
  import * as api from "./api";
  import Icon from "./Icon.svelte";

  let {
    hotkey,
    note = $bindable(""),
    bad = $bindable(false),
    cue = "",
    volume = 0.6,
    sound = true,
    phase = $bindable("idle"),
  }: {
    hotkey: string;
    note?: string;
    bad?: boolean;
    cue?: string;
    volume?: number;
    sound?: boolean;
    phase?: "idle" | "ready" | "press" | "works" | "failed";
  } = $props();

  let testing = $state(false);
  /** The key the shown result is for. */
  let tested = $state("");

  function show(text: string, isBad = false) {
    note = text;
    bad = isBad;
  }

  async function test() {
    const key = hotkey;
    testing = true;
    tested = key;
    phase = "ready";
    show("Getting ready…");
    try {
      await api.borrowHotkey();
      phase = "press";
      show(`Press ${key} now.`);
      const result = await api.testHotkey(key);
      phase = result === "works" ? "works" : "failed";
      if (result === "works") {
        show(`${key} works.`);
        if (sound) api.playSound(cue, volume).catch(() => {});
      } else if (result === "taken") {
        show(`Another app uses ${key}. Pick another key.`, true);
      } else if (result === "nothing") {
        show(`Nothing arrived from ${key}. On a laptop, hold Fn with it, or pick another key.`, true);
      } else {
        show(`${key} can't be a save key. Pick another one.`, true);
      }
    } catch (e) {
      phase = "failed";
      show(`Couldn't test the key: ${e}`, true);
    } finally {
      testing = false;
    }
  }

  $effect(() => {
    // A result is about the key it was for.
    if (hotkey !== tested && !testing) {
      show("");
      phase = "idle";
    }
  });
</script>

<button class="test" class:press={phase === "press"} class:works={phase === "works"} disabled={testing} onclick={test}>
  {#if phase === "press"}
    <span class="pulse"></span>Press it
    <span class="drain" aria-hidden="true"></span>
  {:else if phase === "ready"}
    <span class="spinner"></span>Test
  {:else if phase === "works"}
    <span class="tick"><Icon name="check" size={15} /></span>Works
  {:else}
    Test
  {/if}
</button>

<style>
  .test {
    position: relative;
    min-width: 92px;
    overflow: hidden;
  }

  /* Waiting for a press is the liveliest moment here, not a dimmed button. */
  .test:disabled {
    opacity: 1;
  }

  .test.press {
    background: var(--accent-soft);
    color: var(--text);
  }

  .test.works {
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
    animation: pulse 1s var(--ease-out) infinite alternate;
  }

  /* The ten seconds the test waits. */
  .drain {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    animation: drain 10s linear forwards;
  }

  @keyframes tick {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @keyframes pulse {
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
