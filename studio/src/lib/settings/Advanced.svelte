<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import { library, toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";
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
    <Field
      title="Mode"
      hint={updates.channel === "test"
        ? "Test: you get test builds first and help find problems before everyone else."
        : "Normal: updates that are ready for everyone. Switch to Test to try new things first."}
    >
      <Segmented value={updates.channel} label="Mode" options={modes} onchange={setChannel} />
    </Field>
    {#if channelNote}<p class="note">{channelNote}</p>{/if}
    <Field title="Send performance logs" hint="What Klipp did and how it ran, with your Windows user name left out. No clips, pictures or settings.">
      <Toggle checked={updates.sendLogs} label="Send performance logs" onchange={(v) => setFlag({ sendLogs: v })} />
    </Field>
    <Field title="Install updates by themselves" hint="When you're not playing, or when Klipp starts.">
      <Toggle checked={updates.autoInstall && updates.checks} disabled={!updates.checks} label="Install updates by themselves" onchange={(v) => setFlag({ autoInstall: v })} />
    </Field>
    <Field title="Check for updates" hint="When this window opens, and once a day when you're not playing.">
      <Toggle checked={updates.checks} label="Check for updates" onchange={(v) => setFlag({ checks: v })} />
    </Field>
  </div>
{/if}

<div class="group links">
  <button class="ghost link" onclick={() => api.reveal(library.logDir)}><Icon name="folder" size={17} />Open the log folder<span>recorder.log</span></button>
  <button class="ghost link" onclick={() => api.reveal(library.clipsDir)}><Icon name="film" size={17} />Open the clips folder<span>{library.clipsDir}</span></button>
  <button class="ghost link" onclick={() => api.openFile(library.settingsPath)}><Icon name="sliders" size={17} />Open the settings file<span>settings.toml</span></button>
</div>

<span class="label group-title">Something wrong?</span>
<div class="group links">
  <button class="ghost link" onclick={copyDetails}><Icon name="copy" size={17} />Copy details<span>your PC, settings and the recent log</span></button>
  <button class="ghost link" onclick={() => api.openFile("https://github.com/alonsy/klipp/issues/new/choose")}
    ><Icon name="warning" size={17} />Report a problem or ask for something<span>GitHub</span></button
  >
</div>

{#if store.s?.auto_clip?.cs2}
  <div class="group">
    <Field title="CS2 timing log" hint="Writes every CS2 game update to cs2.log, to check how quickly events arrive.">
      <Toggle checked={store.s.auto_clip.cs2.debug_log} label="CS2 timing log" onchange={(v) => set(["auto_clip", "cs2", "debug_log"], v)} />
    </Field>
  </div>
{/if}

<style>
  .note {
    margin: 0;
    padding: 0 18px 14px;
    font-size: 12px;
    color: var(--muted);
  }

  .links {
    display: flex;
    flex-direction: column;
    padding: 6px;
  }

  .link {
    justify-content: flex-start;
    gap: 12px;
    padding: 10px 12px;
    text-align: left;
  }

  .link span {
    margin-left: auto;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--faint);
  }
</style>
