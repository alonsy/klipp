<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import { library, toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import Reveal from "./Reveal.svelte";
  import { set, store } from "./store.svelte";

  let updates = $state<api.UpdateChannel | null>(null);
  // What switching found: the update card (in the sidebar) offers it.
  let channelNote = $state("");
  onMount(() => {
    api.updateChannel().then((u) => (updates = u), () => {});
  });

  const modes = [
    ["production", "Normal"],
    ["test", "Test"],
  ] as const;

  async function setChannel(channel: api.UpdateChannel["channel"]) {
    if (!updates) return;
    try {
      await api.setUpdateChannel({ ...updates, channel });
      updates = { ...updates, channel };
      channelNote = "Checking…";
      const found = await api.checkUpdate(true);
      channelNote = found
        ? `${found.version.includes("-test") ? "Test build" : "Klipp"} ${found.version} is ready: install it from the card on the left.`
        : channel === "production" && updates.testBuild
          ? "You'll move to the next normal release when it's out."
          : "You have the newest one.";
      // The sidebar's update card checks again and offers it.
      window.dispatchEvent(new Event("klipp:check-update"));
    } catch (e) {
      channelNote = `Couldn't check for updates: ${e}`;
    }
  }

  async function setFlag(change: Partial<Pick<api.UpdateChannel, "sendLogs" | "checks" | "autoInstall">>) {
    if (!updates) return;
    try {
      await api.setUpdateChannel({ ...updates, ...change });
      updates = { ...updates, ...change };
    } catch (e) {
      toast(`Couldn't save that: ${e}`);
    }
  }

  async function copyDetails() {
    try {
      const ok = await api.copyText(await api.supportDetails());
      toast(ok ? "Details copied. Paste them into your message or GitHub issue." : "Couldn't copy the details.");
    } catch (e) {
      toast(`Couldn't collect the details: ${e}`);
    }
  }
</script>

{#if updates}
  <div class="group">
    <h3 class="group-title">Updates</h3>
    <Field
      title="Mode"
      tip={updates.channel === "test"
        ? "Test: you get test builds first and help find problems before everyone else."
        : "Normal: updates that are ready for everyone. Switch to Test to try new things first."}
    >
      <Segmented value={updates.channel} label="Mode" options={modes} onchange={setChannel} />
    </Field>
    <Reveal open={!!channelNote}>
      <p class="note">{channelNote}</p>
    </Reveal>
    <Field title="Check for updates" tip="When this window opens, and once a day when you're not playing.">
      <Toggle checked={updates.checks} label="Check for updates" onchange={(v) => setFlag({ checks: v })} />
    </Field>
    <Field title="Install updates by themselves" tip="When you're not playing, or when Klipp starts.">
      <Toggle checked={updates.autoInstall && updates.checks} disabled={!updates.checks} label="Install updates by themselves" onchange={(v) => setFlag({ autoInstall: v })} />
    </Field>
  </div>

  <div class="group">
    <h3 class="group-title">Privacy</h3>
    <Field title="Send performance logs" hint="What Klipp did and how it ran, with your Windows user name left out. No clips, pictures or settings.">
      <Toggle checked={updates.sendLogs} label="Send performance logs" onchange={(v) => setFlag({ sendLogs: v })} />
    </Field>
  </div>
{/if}

<div class="group">
  <h3 class="group-title">Something wrong?</h3>
  <Field title="Copy details" tip="Your PC, settings and the recent log.">
    <button onclick={copyDetails}>Copy</button>
  </Field>
  <Field title="Report a problem or ask for something">
    <button onclick={() => api.openFile("https://github.com/alonsy/klipp/issues/new/choose")}>GitHub</button>
  </Field>
  {#if store.s?.auto_clip?.cs2}
    <Field title="CS2 timing log" tip="Writes every CS2 game update to cs2.log, to check how quickly events arrive.">
      <Toggle checked={store.s.auto_clip.cs2.debug_log} label="CS2 timing log" onchange={(v) => set(["auto_clip", "cs2", "debug_log"], v)} />
    </Field>
  {/if}
</div>

<div class="group">
  <h3 class="group-title">Files</h3>
  <div class="files">
    <button title="recorder.log" onclick={() => api.reveal(library.logDir)}>Log folder</button>
    <button title={library.clipsDir} onclick={() => api.reveal(library.clipsDir)}>Clips folder</button>
    <button title="settings.toml" onclick={() => api.openFile(library.settingsPath)}>Settings file</button>
  </div>
</div>

<style>
  .note {
    padding-bottom: var(--sp-2);
  }

  .files {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding-top: var(--sp-1);
  }
</style>
