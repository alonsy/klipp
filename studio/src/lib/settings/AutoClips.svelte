<script lang="ts">
  import Icon from "../Icon.svelte";
  import { enter } from "../motion";
  import Field from "../ui/Field.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);
  const a = $derived(s.auto_clip);
  const cs2 = $derived(s.auto_clip.cs2);
  const lol = $derived(s.auto_clip.league);
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

  /** One game's moments: settings key, icon, name, and a tooltip when the name doesn't say it all. */
  type Moment = readonly [key: string, icon: string, title: string, tip?: string];

  const events: readonly Moment[] = [
    ["headshot", "headshot", "Every headshot"],
    ["any_kill", "kill", "Every kill", "Lots of clips; good for practice sessions."],
    ["knife_or_zeus_kill", "knife", "Knife and Zeus kills"],
    ["death", "death", "Deaths"],
    ["mvp", "mvp", "Round MVP"],
  ];

  const leagueEvents: readonly Moment[] = [
    ["first_blood", "firstblood", "First blood", "When the game's first kill is yours."],
    ["steal", "steal", "Steals", "A Baron, Dragon or Herald you take from the other team."],
    ["ace", "teamace", "Aces", "Your team takes down all five, with you in the fight."],
    ["objective", "objective", "Objectives", "Every dragon, Baron, Herald, turret and inhibitor you take part in."],
    ["any_kill", "kill", "Every kill", "Lots of clips; good for practice."],
    ["death", "death", "Deaths"],
  ];

  const rocketEvents: readonly Moment[] = [
    ["goal", "goal", "Goals", "Every goal you score; aerials and overtime winners are named."],
    ["epic_save", "epicsave", "Epic saves", "The saves the game calls epic."],
    ["demolition", "demolition", "Demolitions"],
    ["save", "save", "Every save"],
    ["assist", "assist", "Assists"],
  ];
</script>

{#snippet moments(game: string, list: readonly Moment[])}
  <div class="moments">
    {#each list as [key, icon, title, tip] (key)}
      <label class="moment" data-tip={tip}>
        <Icon event={icon} size={18} />
        <span class="m-name">{title}</span>
        <Toggle checked={s.auto_clip[game][key]} label={title} onchange={(v) => set(["auto_clip", game, key], v)} />
      </label>
    {/each}
  </div>
{/snippet}

{#key open}
  <div class="view" in:enter={{ x: open ? 16 : -16, y: 0 }}>
    {#if current}
      {@const on = s.auto_clip[current.id].enabled}
      <button class="ghost back" onclick={() => (open = null)}><Icon name="back" size={15} />Auto-clips</button>
      <div class="game-head">
        <Icon event={current.icon} size={26} />
        <div class="head-text">
          <span class="game">{current.name}</span>
          <span class="how"><Icon name="shield" size={13} />{current.how}</span>
        </div>
        <Toggle checked={on} label="{current.name} auto-clips" onchange={(v) => set(["auto_clip", current.id, "enabled"], v)} />
      </div>

      {#if current.id === "cs2"}
        <p class="note">Restart CS2 once after turning it on.</p>
        <div class="group" class:disabled={!a.enabled || !on}>
          <Field title="Multi-kills" tip="Clip kills in quick succession, starting from this many.">
            <Segmented
              value={cs2.multi_kill_min}
              label="Multi-kills"
              options={[[0, "Off"], [2, "2K"], [3, "3K"], [4, "4K"], [5, "ACE"]]}
              onchange={(v) => set(["auto_clip", "cs2", "multi_kill_min"], v)}
            />
          </Field>
          <Field
            title="What counts as a 2K"
            tip={cs2.multi_kill_mode === "round"
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
          {@render moments("cs2", events)}
        </div>
        <p class="note">Round wins, losses and defuses are marked on the timeline of every clip.</p>
      {:else if current.id === "league"}
        <div class="group" class:disabled={!a.enabled || !on}>
          <Field title="Multi-kills" tip="Clip kills the game counts as a multi-kill, starting from this one.">
            <Segmented
              value={lol.multi_kill_min}
              label="League multi-kills"
              options={[[0, "Off"], [2, "Double"], [3, "Triple"], [4, "Quadra"], [5, "Penta"]]}
              onchange={(v) => set(["auto_clip", "league", "multi_kill_min"], v)}
            />
          </Field>
          {@render moments("league", leagueEvents)}
        </div>
        <p class="note">Assists, objectives and the result are marked on the timeline of every clip.</p>
      {:else if current.id === "rocket_league"}
        <p class="note">Restart Rocket League once after turning it on.</p>
        <div class="group" class:disabled={!a.enabled || !on}>
          {@render moments("rocket_league", rocketEvents)}
        </div>
        <p class="note">Assists, saves and the result are marked on the timeline of every clip.</p>
      {/if}
    {:else}
      {#if !a.enabled}
        <div class="banner">
          <Icon name="info" size={16} />
          <span class="grow">Game auto-clips are off, so none of these games are clipped.</span>
          <button class="turn-on" onclick={() => set(["auto_clip", "enabled"], true)}>Turn on</button>
        </div>
      {/if}

      <div class="tiles" class:disabled={!a.enabled}>
        {#each games as g (g.id)}
          {@const on = s.auto_clip[g.id].enabled}
          <div class="tile" class:on>
            <button class="open" title={g.short} onclick={() => (open = g.id)} aria-label="{g.name} settings">
              <span class="mark"><Icon event={g.icon} size={24} /></span>
              <span class="name">{g.name}</span>
              <span class="sees">{g.sees}</span>
            </button>
            <span class="switch"><Toggle checked={on} label="{g.name} auto-clips" onchange={(v) => set(["auto_clip", g.id, "enabled"], v)} /></span>
          </div>
        {/each}
      </div>

      <div class="group" class:disabled={!a.enabled}>
        <h3 class="group-title">Timing, for every game</h3>
        <Field title="Keep before" tip="Seconds before the first moment of a clip.">
          <Slider min={0} max={30} value={pre} label="Keep before" oninput={(v) => (pre = v)} onchange={(v) => set(["auto_clip", "pre_roll_secs"], v)} />
          <span class="value">{pre} s</span>
        </Field>
        <Field title="Keep after" tip="Seconds after the last one.">
          <Slider min={0} max={15} value={post} label="Keep after" oninput={(v) => (post = v)} onchange={(v) => set(["auto_clip", "post_roll_secs"], v)} />
          <span class="value">{post} s</span>
        </Field>
        <Field title="Join moments within" tip="Moments this close together become one clip: a 2K followed by a third kill is one 3K clip.">
          <Slider min={3} max={30} value={gap} label="Join moments within" oninput={(v) => (gap = v)} onchange={(v) => set(["auto_clip", "merge_gap_secs"], v)} />
          <span class="value">{gap} s</span>
        </Field>
      </div>
    {/if}
  </div>
{/key}

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    min-width: 0;
  }

  .disabled {
    opacity: 0.5;
  }

  .grow {
    flex: 1;
  }

  .turn-on {
    padding: 4px var(--sp-3);
    font-size: var(--fs-sm);
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: var(--sp-3);
    margin-bottom: var(--sp-2);
  }

  .tile {
    position: relative;
    border-radius: var(--r-md);
    background: var(--surface);
    transition: background var(--dur-fast);
  }

  .tile:hover {
    background: var(--surface-2);
  }

  .open {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-start;
    gap: var(--sp-1);
    width: 100%;
    height: 100%;
    padding: var(--sp-4);
    border-radius: inherit;
    background: none;
    text-align: left;
  }

  .open:hover {
    background: none;
  }

  .open:active {
    transform: none;
  }

  .mark {
    display: inline-flex;
    align-items: center;
    height: 26px;
    margin-bottom: var(--sp-2);
    opacity: 0.5;
    transition: opacity var(--dur) var(--ease-out);
  }

  .tile.on .mark {
    opacity: 1;
  }

  .name {
    font-weight: 600;
  }

  .sees {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.4;
  }

  .switch {
    position: absolute;
    top: var(--sp-4);
    right: var(--sp-4);
    display: flex;
  }

  .back {
    align-self: flex-start;
    gap: 6px;
    margin: calc(-1 * var(--sp-2)) 0 calc(-1 * var(--sp-1)) calc(-1 * var(--sp-2));
    padding: 4px 10px 4px 6px;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .game-head {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .head-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .game {
    font-size: var(--fs-lg);
    font-weight: 650;
  }

  .how {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--faint);
    font-size: var(--fs-sm);
  }

  /* Two columns of switches: the whole row toggles. */
  .moments {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    column-gap: var(--sp-6);
    margin-top: var(--sp-1);
  }

  .moment {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
    cursor: pointer;
  }

  .m-name {
    flex: 1;
    min-width: 0;
    font-weight: 500;
  }
</style>
