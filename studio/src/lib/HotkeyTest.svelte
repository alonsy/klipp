<script lang="ts">
  // Tries the save key for real: Test, then press it within 10 s. A key
  // another app holds, or an F key that needs Fn on a laptop, shows up here
  // instead of mid-game. Nothing is saved. The result goes to `note`, for the
  // line under the setting's name.
  import * as api from "./api";

  let {
    hotkey,
    note = $bindable(""),
    bad = $bindable(false),
    cue = "",
    volume = 0.6,
    sound = true,
  }: { hotkey: string; note?: string; bad?: boolean; cue?: string; volume?: number; sound?: boolean } = $props();

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
    show("Getting ready…");
    try {
      await api.borrowHotkey();
      show(`Press ${key} now.`);
      const result = await api.testHotkey(key);
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
      show(`Couldn't test the key: ${e}`, true);
    } finally {
      testing = false;
    }
  }

  $effect(() => {
    // A result is about the key it was for.
    if (hotkey !== tested && !testing) show("");
  });
</script>

<button disabled={testing} onclick={test}>{testing ? "Testing…" : "Test"}</button>
