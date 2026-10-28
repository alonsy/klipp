<script lang="ts">
  import Icon from "../Icon.svelte";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);
  const a = $derived(s.auto_clip);
  const cs2 = $derived(s.auto_clip.cs2);
  const lol = $derived(s.auto_clip.league);
  const rl = $derived(s.auto_clip.rocket_league);
  let pre = $state(Number(store.s!.auto_clip.pre_roll_secs));
  let post = $state(Number(store.s!.auto_clip.post_roll_secs));
  let gap = $state(Number(store.s!.auto_clip.merge_gap_secs));

  /** The game whose page is open, or null for the boxes. */
  let open = $state<string | null>(null);

  // How each game's moments reach Klipp, in a few words (no technical guide):
  // the long form on a game's page, the short one on its box.
  const EVENTS = "Recorded via game events · Anti-cheat safe";
  const EVENTS_SHORT = "Game events · Anti-cheat safe";
  const games = [
    { id: "cs2", name: "Counter-Strike 2", icon: "headshot", how: EVENTS, short: EVENTS_SHORT, sees: "Headshots, multi-kills, clutches" },
    { id: "league", name: "League of Legends", icon: "objective", how: EVENTS, short: EVENTS_SHORT, sees: "Multi-kills, first bloods, steals, aces" },
    { id: "rocket_league", name: "Rocket League", icon: "goal", how: EVENTS, short: EVENTS_SHORT, sees: "Goals, epic saves, demolitions" },
  ] as const;
  const current = $derived(games.find((g) => g.id === open));

  const events = [
    ["headshot", "headshot", "Every headshot", "A clip for each headshot kill."],
    ["any_kill", "kill", "Every kill", "Lots of clips; good for practice sessions."],
    ["knife_or_zeus_kill", "knife", "Knife and Zeus kills", "The humiliating ones."],
    ["death", "death", "Deaths", "See what got you."],
    ["mvp", "mvp", "Round MVP", "When you're the round's MVP."],
  ] as const;

  const leagueEvents = [
    ["first_blood", "firstblood", "First blood", "When the game's first kill is yours."],
    ["steal", "steal", "Steals", "A Baron, Dragon or Herald you take from the other team."],
    ["ace", "teamace", "Aces", "Your team takes down all five, with you in the fight."],
    ["objective", "objective", "Objectives", "Every dragon, Baron, Herald, turret and inhibitor you take part in."],
    ["any_kill", "kill", "Every kill", "Lots of clips; good for practice."],
    ["death", "death", "Deaths", "See what got you."],
  ] as const;

  const rocketEvents = [
    ["goal", "goal", "Goals", "Every goal you score; aerials and overtime winners are named."],
    ["epic_save", "epicsave", "Epic saves", "The saves the game calls epic."],
    ["demolition", "demolition", "Demolitions", "Every car you demolish."],
    ["save", "save", "Every save", "Not just the epic ones."],
    ["assist", "assist", "Assists", "Goals you set up."],
  ] as const;
</script>

{#if current}
  {@const on = s.auto_clip[current.id].enabled}
  <button class="ghost back" onclick={() => (open = null)}><Icon name="back" size={15} />Auto-clips</button>
  <div class="game-head">
    <span class="mark"><Icon event={current.icon} size={26} /></span>
    <div class="head-text">
      <span class="game">{current.name}</span>
      <span class="how"><Icon name="shield" size={13} />{current.how}</span>
    </div>
    <Toggle checked={on} label="{current.name} auto-clips" onchange={(v) => set(["auto_clip", current.id, "enabled"], v)} />
  </div>

  {#if current.id === "cs2"}
    <p class="note">Restart CS2 once after turning it on.</p>
    <div class="group" class:disabled={!a.enabled || !on}>
      <Field title="Multi-kills" hint="Clip kills in quick succession, starting from this many.">
        <Segmented
          value={cs2.multi_kill_min}
          label="Multi-kills"
          options={[[0, "Off"], [2, "2K"], [3, "3K"], [4, "4K"], [5, "ACE"]]}
          onchange={(v) => set(["auto_clip", "cs2", "multi_kill_min"], v)}
        />
      </Field>
      <Field
        title="What counts as a 2K"
        hint={cs2.multi_kill_mode === "round"
          ? "Kills anywhere in the same round, like the scoreboard."
          : `Kills within ${gap} s of each other: the highlight you'd actually want to watch.`}
      >
        <Segmented
          value={cs2.multi_kill_mode}
          label="What counts as a multi-kill"
          options={[["streak", "Close together"], ["round", "Same round"]]}
          onchange={(v) => set(["auto_clip", "cs2", "multi_kill_mode"], v)}
        />
      </Field>
      {#each events as [key, icon, title, hint] (key)}
        <Field {title} {hint}>
          <span class="ev"><Icon event={icon} size={20} /></span>
          <Toggle checked={cs2[key]} label={title} onchange={(v) => set(["auto_clip", "cs2", key], v)} />
        </Field>
      {/each}
    </div>
    <p class="note">Round wins, losses and defuses are marked on the timeline of every clip.</p>
  {:else if current.id === "league"}
    <div class="group" class:disabled={!a.enabled || !on}>
      <Field title="Multi-kills" hint="Clip kills the game counts as a multi-kill, starting from this one.">
        <Segmented
          value={lol.multi_kill_min}
          label="League multi-kills"
          options={[[0, "Off"], [2, "Double"], [3, "Triple"], [4, "Quadra"], [5, "Penta"]]}
          onchange={(v) => set(["auto_clip", "league", "multi_kill_min"], v)}
        />
      </Field>
      {#each leagueEvents as [key, icon, title, hint] (key)}
        <Field {title} {hint}>
          <span class="ev"><Icon event={icon} size={20} /></span>
          <Toggle checked={lol[key]} label={title} onchange={(v) => set(["auto_clip", "league", key], v)} />
        </Field>
      {/each}
    </div>
    <p class="note">Assists, objectives and the result are marked on the timeline of every clip.</p>
  {:else if current.id === "rocket_league"}
    <p class="note">Restart Rocket League once after turning it on.</p>
    <div class="group" class:disabled={!a.enabled || !on}>
      {#each rocketEvents as [key, icon, title, hint] (key)}
        <Field {title} {hint}>
          <span class="ev"><Icon event={icon} size={20} /></span>
          <Toggle checked={rl[key]} label={title} onchange={(v) => set(["auto_clip", "rocket_league", key], v)} />
        </Field>
      {/each}
    </div>
    <p class="note">Assists, saves and the result are marked on the timeline of every clip.</p>
  {/if}
{:else}
  {#if !a.enabled}
    <div class="banner">
      <Icon name="info" size={17} />
      <span class="grow">Game auto-clips are off, so none of these games are clipped.</span>
      <button onclick={() => set(["auto_clip", "enabled"], true)}>Turn on</button>
    </div>
  {/if}

  <div class="tiles" class:disabled={!a.enabled}>
    {#each games as g (g.id)}
      {@const on = s.auto_clip[g.id].enabled}
      <div class="tile" class:on>
        <button class="open" onclick={() => (open = g.id)} aria-label="{g.name} settings">
          <span class="mark"><Icon event={g.icon} size={24} /></span>
          <span class="name">{g.name}</span>
          <span class="sees">{g.sees}</span>
          <span class="how"><Icon name="shield" size={12} />{g.short}</span>
        </button>
        <span class="edit" aria-hidden="true"><Icon name="pencil" size={14} /></span>
        <span class="switch"><Toggle checked={on} label="{g.name} auto-clips" onchange={(v) => set(["auto_clip", g.id, "enabled"], v)} /></span>
      </div>
    {/each}
  </div>

  <span class="label group-title">Timing, for every game</span>
  <div class="group" class:disabled={!a.enabled}>
    <Field title="Keep before" hint="Seconds before the first moment of a clip.">
      <Slider min={0} max={30} value={pre} label="Keep before" oninput={(v) => (pre = v)} onchange={(v) => set(["auto_clip", "pre_roll_secs"], v)} />
      <span class="value">{pre} s</span>
    </Field>
    <Field title="Keep after" hint="Seconds after the last one.">
      <Slider min={0} max={15} value={post} label="Keep after" oninput={(v) => (post = v)} onchange={(v) => set(["auto_clip", "post_roll_secs"], v)} />
      <span class="value">{post} s</span>
    </Field>
    <Field title="Join moments within" hint="Moments this close together become one clip: a 2K followed by a third kill is one 3K clip.">
      <Slider min={3} max={30} value={gap} label="Join moments within" oninput={(v) => (gap = v)} onchange={(v) => set(["auto_clip", "merge_gap_secs"], v)} />
      <span class="value">{gap} s</span>
    </Field>
  </div>
{/if}

<style>
  .disabled {
    opacity: 0.5;
  }

  .grow {
    flex: 1;
  }

  .banner {
    align-items: center;
  }

  .banner button {
    padding: 4px 12px;
    font-size: 12.5px;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: 12px;
  }

  .tile {
    position: relative;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line);
    transition:
      border-color 0.15s,
      transform 0.15s var(--ease);
  }

  .tile:hover {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    transform: translateY(-1px);
  }

  .open {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    width: 100%;
    padding: 16px 16px 14px;
    border: none;
    border-radius: inherit;
    background: none;
    text-align: left;
  }

  .open:hover {
    background: none;
  }

  .mark {
    display: inline-flex;
    padding: 8px;
    border-radius: 10px;
    background: var(--surface-3);
    opacity: 0.55;
    transition: opacity 0.15s;
  }

  .tile.on .mark {
    opacity: 1;
  }

  .name {
    margin-top: 4px;
    font-size: 14.5px;
    font-weight: 650;
    color: var(--text);
  }

  .sees {
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.4;
  }

  .how {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 4px;
    font-size: 11px;
    color: var(--faint);
  }

  .switch {
    position: absolute;
    top: 14px;
    right: 14px;
  }

  .edit {
    position: absolute;
    top: 50px;
    right: 16px;
    color: var(--muted);
    opacity: 0;
    transition: opacity 0.15s;
    pointer-events: none;
  }

  .tile:hover .edit {
    opacity: 1;
  }

  .back {
    align-self: flex-start;
    gap: 6px;
    padding: 4px 10px 4px 6px;
    margin-bottom: -8px;
    color: var(--muted);
    font-size: 13px;
  }

  .game-head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .head-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
  }

  .game {
    font-size: 17px;
    font-weight: 650;
  }

  .game-head .mark {
    opacity: 1;
  }

  .note {
    margin: 0 4px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--muted);
  }

  .ev {
    display: inline-flex;
    opacity: 0.85;
  }
</style>
