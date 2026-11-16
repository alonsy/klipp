<script lang="ts">
  // The account on klipp.dev: optional. Signing in happens in the browser
  // (Klipp shows a code, the website asks to connect this PC), so no password
  // is ever typed here.
  import { onMount } from "svelte";
  import * as api from "../api";
  import { cloud, planName, refreshCloud } from "../cloud.svelte";
  import Icon from "../Icon.svelte";
  import { toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import Reveal from "./Reveal.svelte";

  /** The code being waited on, while the browser is open. */
  let link = $state<api.CloudLink | null>(null);
  let starting = $state(false);
  let leaving = $state(false);
  /** Why the last try to sign in ended without one. */
  let note = $state("");

  /** Space on klipp.dev, counted the way its plans are (2 GB is 2 × 1024³ bytes), so the numbers match the website's. */
  function size(bytes: number): string {
    const gb = bytes / 1024 ** 3;
    if (gb >= 1) return `${Number.isInteger(gb) || gb >= 10 ? gb.toFixed(0) : gb.toFixed(1)} GB`;
    const mb = bytes / 1024 ** 2;
    return mb >= 10 || mb === 0 ? `${Math.round(mb)} MB` : `${Math.max(0.1, mb).toFixed(1)} MB`;
  }

  const account = $derived(cloud.status?.account ?? null);
  const plan = $derived(account?.plan ?? null);
  const used = $derived(plan && plan.storage > 0 ? Math.min(1, plan.used / plan.storage) : 0);
  const site = $derived(account ? new URL(account.profile).origin : "https://klipp.dev");
  const quality = $derived(plan ? `${plan.maxHeight >= 2160 ? "4K" : `${plan.maxHeight}p`} at ${plan.maxFps} fps` : "");

  /** Which clips auto-backup takes, as chosen on klipp.dev. */
  const BACKUP: Record<string, string> = { favorites: "your favorites", auto: "every auto-clip", all: "every clip" };
  const backingUp = $derived(plan?.autoBackup && account ? (BACKUP[account.backup] ?? null) : null);
  const backedUp = $derived(cloud.status?.backedUp ?? 0);
  const backupHint = $derived.by(() => {
    if (!plan?.autoBackup) return "Comes with Plus and Pro: Klipp then puts the clips you save on your account by itself, as private clips. Your favorites, your auto-clips or every clip.";
    if (!backingUp) return "Off. Choose on klipp.dev which new clips Klipp puts on your account by itself: your favorites, every auto-clip or every clip.";
    const sofar = backedUp > 0 ? ` ${backedUp === 1 ? "1 clip" : `${backedUp} clips`} from this PC so far.` : "";
    return `On for ${backingUp} saved since you turned it on. They go up as private clips between games, never while you play.${sofar}`;
  });

  onMount(() => {
    refreshCloud();
    const off = api.onCloudLink((end) => {
      link = null;
      if (end.state === "linked") {
        note = "";
        toast(`Signed in as @${end.handle}`);
        refreshCloud();
      } else if (end.state === "denied") note = "You chose Deny on klipp.dev, so this PC wasn't connected.";
      else if (end.state === "expired") note = "The code ran out before it was approved. Start again when you're ready.";
      else if (end.state === "failed") note = `Signing in didn't work: ${end.error}`;
    });
    return () => void off.then((unlisten) => unlisten());
  });

  async function signIn() {
    starting = true;
    note = "";
    try {
      link = await api.cloudLink();
    } catch (e) {
      note = String(e);
    }
    starting = false;
  }

  function cancel() {
    api.cloudLinkCancel();
    link = null;
  }

  async function signOut() {
    leaving = true;
    try {
      const told = await api.cloudSignOut();
      toast(told ? "Signed out on this PC" : "Signed out here. klipp.dev couldn't be reached: remove this PC under your account's devices there too.");
    } catch (e) {
      toast(`Couldn't sign out: ${e}`);
    }
    leaving = false;
    await refreshCloud();
  }
</script>

{#if !cloud.status}
  <div class="group" aria-hidden="true">
    {#each [36, 52] as w, i (i)}<span class="skeleton text" style:width="{w}%"></span>{/each}
  </div>
{:else if !cloud.status.signedIn}
  <div class="group">
    <h3 class="group-title">Klipp account</h3>
    <p class="lead">
      With an account, a clip becomes a link that plays right in Discord, and your best moments have a place on klipp.dev. Klipp records, clips and edits the
      same without one.
    </p>
    <Reveal open={!link}>
      <Field title="Sign in" hint="Opens klipp.dev in your browser. You sign in there; no password is typed into Klipp.">
        <button class="primary" onclick={signIn} disabled={starting}>{starting ? "Opening…" : "Sign in"}</button>
      </Field>
    </Reveal>
    <Reveal open={!!link}>
      {#if link}
        <div class="linking">
          <p class="note">Your browser opened klipp.dev. Check that it shows this code, then choose <b>Connect this PC</b> there.</p>
          <div class="code mono" aria-label="Code {link.code.split('').join(' ')}">{link.code}</div>
          <div class="actions">
            <button onclick={() => link && api.openFile(link.url)}>Open the page again</button>
            <button class="ghost" onclick={cancel}>Cancel</button>
            <span class="waiting"><i class="pulse"></i>Waiting for you on klipp.dev</span>
          </div>
        </div>
      {/if}
    </Reveal>
    <Reveal open={!!note}><p class="banner"><Icon name="info" size={16} /><span>{note}</span></p></Reveal>
  </div>
{:else if account && plan}
  <div class="group">
    <h3 class="group-title">Klipp account</h3>
    <div class="who">
      <span class="handle">@{account.handle}</span>
      <span class="plan" class:paid={plan.id !== "free"}>{planName(plan.id)}</span>
    </div>
    {#if account.status !== "active"}
      <p class="banner"><Icon name="warning" size={16} /><span>This account can't upload right now. Your notifications on klipp.dev say why.</span></p>
    {/if}
    <div class="space">
      <div class="bar" role="img" aria-label="{size(plan.used)} of {size(plan.storage)} used">
        <div class="fill" class:full={used > 0.9} style:transform="scaleX({Math.max(used, 0.004)})"></div>
      </div>
      <p class="note"><b class="mono">{size(plan.used)}</b> of {size(plan.storage)} used on klipp.dev · clips up to {Math.round(plan.maxSeconds / 60)} minutes, {quality}</p>
    </div>
    <Field title="On klipp.dev">
      <button onclick={() => api.openFile(account.profile)}>Your profile</button>
      <button onclick={() => api.openFile(`${site}/library`)}>Your clips</button>
      {#if plan.id === "free"}<button onclick={() => api.openFile(`${site}/pricing`)}>Plans</button>{/if}
    </Field>
    <Field title="Auto-backup" hint={backupHint}>
      {#if plan.autoBackup}
        <button onclick={() => api.openFile(`${site}/settings/account`)}>{backingUp ? "Change on klipp.dev" : "Turn on at klipp.dev"}</button>
      {/if}
    </Field>
  </div>
  <div class="group">
    <Field title="Sign out" hint="Disconnects this PC. Your clips on klipp.dev stay.">
      <button onclick={signOut} disabled={leaving}>Sign out</button>
    </Field>
  </div>
{:else}
  <div class="group">
    <h3 class="group-title">Klipp account</h3>
    <p class="banner"><Icon name="warning" size={16} /><span>This PC is signed in, but klipp.dev couldn't be reached. {cloud.status.error}</span></p>
    <Field title="Try again">
      <button onclick={() => refreshCloud()} disabled={cloud.loading}>{cloud.loading ? "Checking…" : "Check again"}</button>
    </Field>
    <Field title="Sign out" hint="Disconnects this PC. Your clips on klipp.dev stay.">
      <button onclick={signOut} disabled={leaving}>Sign out</button>
    </Field>
  </div>
{/if}

<style>
  .lead {
    margin: 0 0 var(--sp-2);
    max-width: 62ch;
    color: var(--muted);
    line-height: 1.55;
  }

  .linking {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: var(--sp-2) 0 var(--sp-3);
  }

  .linking .note b {
    color: var(--text);
    font-weight: 600;
  }

  .code {
    align-self: flex-start;
    padding: var(--sp-3) var(--sp-5);
    border-radius: var(--r-md);
    background: var(--surface-2);
    font-size: 28px;
    font-weight: 600;
    letter-spacing: 0.12em;
    color: var(--text);
    user-select: all;
  }

  .actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }

  .waiting {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    margin-left: var(--sp-2);
    color: var(--faint);
    font-size: var(--fs-sm);
  }

  .pulse {
    width: 7px;
    height: 7px;
    border-radius: var(--r-full);
    background: var(--accent);
    animation: pulse 1.6s var(--ease-out) infinite;
  }

  .who {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-1) 0 var(--sp-3);
  }

  .handle {
    font-size: var(--fs-lg, 18px);
    font-weight: 600;
    color: var(--text);
  }

  .plan {
    padding: 2px var(--sp-2);
    border-radius: var(--r-full);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-sm);
    font-weight: 600;
  }

  .plan.paid {
    background: var(--accent-soft, var(--surface-3));
    color: var(--accent);
  }

  .space {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding-bottom: var(--sp-2);
  }

  .space .note b {
    color: var(--text);
    font-weight: 600;
  }

  .bar {
    height: 4px;
    border-radius: var(--r-full);
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform var(--dur-slow) var(--ease-out);
  }

  .fill.full {
    background: var(--warn);
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }
</style>
