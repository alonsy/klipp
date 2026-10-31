<script lang="ts" module>
  /** The list has been shown once: coming back to it doesn't play the entry again. */
  let introduced = false;
</script>

<script lang="ts">
  import { onDestroy, tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import type { TransitionConfig } from "svelte/transition";
  import * as api from "./api";
  import type { Arrival, Clip } from "./api";
  import ArrivingCard from "./ArrivingCard.svelte";
  import ClipCard from "./ClipCard.svelte";
  import { clock, day, duration, eventName, eventRank, mapName, modeName, size } from "./format";
  import Icon from "./Icon.svelte";
  import { DUR, easeOut, enter, leave, pageOut, pop, reduced, spring, stagger } from "./motion";
  import { groupSessions, type Session } from "./sessions";
  import {
    deleteClips,
    filters,
    incoming,
    inScope,
    library,
    libraryView,
    matches,
    openClip,
    prefs,
    recorder,
    reload,
    setFavorite,
    setPref,
    sorted,
    ui,
    type Kind,
    type Sort,
  } from "./state.svelte";
  import { ignoreDoubleClick, swallowPress } from "./ui/clicks";
  import Segmented from "./ui/Segmented.svelte";

  // Layout (px).
  const PAD = 24;
  const GAP = 12;
  const MIN_CARD = 240;
  const HEADER_H = 40;
  const SESSION_SPACE = 12;
  /** Room under the last row for the selection bar. */
  const SELECTION_H = 72;
  /** Below this list width the toolbar's words become icons. */
  const NARROW = 760;

  let visible = $derived(sorted(library.clips.filter(matches)));
  /** Sessions only make sense in time order. */
  let timeOrder = $derived(filters.sort === "newest" || filters.sort === "oldest");
  let grouped = $derived(prefs.groupSessions && timeOrder);
  let sessions = $derived<Session[]>(
    grouped
      ? groupSessions(visible, filters.sort === "newest")
      : [{ key: "", game: "", map: "", mode: "", start: 0, end: 0, clips: visible }],
  );
  /** Clips on their way, shown first while the newest are on top. */
  let waiting = $derived(
    filters.sort !== "newest" || filters.kind === "favorites"
      ? []
      : incoming.list.filter((a) => (!filters.game || a.game === filters.game) && (filters.kind === "all" || a.kind === filters.kind)),
  );
  let arriving = $state.raw<Arrival[]>([]);
  let holding: Clip[] | null = null;
  let held: ReturnType<typeof setTimeout>;

  // A placeholder stays until its clip is in the list, so the two swap in one
  // step: otherwise the cards rise into the gap and drop again a moment later.
  $effect.pre(() => {
    const now = waiting;
    const clips = library.clips;
    const landed = untrack(() => arriving).some((a) => !incoming.list.some((b) => b.id === a.id));
    if (landed && holding === null) {
      holding = clips;
      held = setTimeout(() => {
        holding = null;
        arriving = untrack(() => waiting);
      }, 800);
    } else if (!landed || clips !== holding) {
      clearTimeout(held);
      holding = null;
      arriving = now;
    }
  });
  onDestroy(() => clearTimeout(held));

  /** Clips in the order they're shown, for Shift-click ranges. */
  let flat = $derived(sessions.flatMap((s) => s.clips));

  // Event chips: what's in the current folder/kind/search, most notable first.
  const CHIP_EVENTS = /^(\d+k|ace|penta|headshot|knife|zeus|mvp|death|teamace|firstblood|steal|goal|epicsave|save|demolition)$/;
  let chips = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of library.clips) {
      if (!inScope(c)) continue;
      const kinds = new Set([...(c.label ? [c.label] : []), ...c.events.map((e) => e.kind)]);
      for (const k of kinds) if (CHIP_EVENTS.test(k)) m.set(k, (m.get(k) ?? 0) + 1);
    }
    return [...m].sort((a, b) => eventRank(b[0]) - eventRank(a[0]));
  });

  const selected = new SvelteSet<string>();
  let anchor: number | null = null;

  // Forget selections that were deleted or filtered away.
  $effect(() => {
    const shown = new Set(flat.map((c) => c.path));
    for (const p of [...selected]) if (!shown.has(p)) selected.delete(p);
  });

  // Rows of cards and session headers; only those in view are rendered.
  let scroller: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);
  let cols = $derived(Math.max(1, Math.floor((width - 2 * PAD + GAP) / (MIN_CARD + GAP))));
  // Whole pixels: text on a fractional position renders blurry and its edges get clipped.
  let cardW = $derived(Math.max(1, Math.floor((width - 2 * PAD - GAP * (cols - 1)) / cols)));
  let cardH = $derived(Math.round((cardW * 9) / 16));
  let narrow = $derived(width > 0 && width < NARROW);

  type Row =
    | { kind: "arriving"; key: string; y: number; h: number }
    | { kind: "header"; key: string; y: number; h: number; session: Session }
    | { kind: "cards"; key: string; y: number; h: number; clips: Clip[]; first: number };

  let layout = $derived.by(() => {
    const rows: Row[] = [];
    let y = grouped ? 0 : 4;
    let index = 0;
    if (arriving.length > 0) {
      const h = cardH + GAP + (grouped ? SESSION_SPACE : 0);
      rows.push({ kind: "arriving", key: "arriving", y, h });
      y += h;
    }
    for (const s of sessions) {
      if (grouped) {
        rows.push({ kind: "header", key: `h:${s.key}`, y, h: HEADER_H, session: s });
        y += HEADER_H;
      }
      for (let i = 0; i < s.clips.length; i += cols) {
        rows.push({ kind: "cards", key: `r:${s.key}:${i}`, y, h: cardH + GAP, clips: s.clips.slice(i, i + cols), first: index + i });
        y += cardH + GAP;
      }
      index += s.clips.length;
      y += grouped ? SESSION_SPACE : 0;
    }
    return { rows, height: y + PAD };
  });

  // Back from the editor: the same scroll position, moved only if the clip
  // you opened isn't in view anymore (then it's centered and glows briefly).
  let restored = false;
  $effect(() => {
    if (restored || width === 0 || height === 0 || library.loading) return;
    restored = true;
    const opened = libraryView.opened;
    libraryView.opened = null;
    let top = libraryView.scrollTop;
    const row = opened ? layout.rows.find((r) => r.kind === "cards" && r.clips.some((c) => c.path === opened)) : undefined;
    const reveal = !!row && (row.y < top || row.y + cardH > top + height);
    if (row && reveal) top = Math.max(0, Math.round(row.y - (height - cardH) / 2));
    // The list's full height reaches the page a moment later; scrolling
    // before that would stop short.
    tick().then(() => {
      if (!scroller) return;
      scroller.scrollTop = top;
      scrollTop = scroller.scrollTop;
      if (opened && reveal) {
        incoming.fresh = [opened];
        setTimeout(() => {
          if (incoming.fresh.length === 1 && incoming.fresh[0] === opened) incoming.fresh = [];
        }, 2600);
      }
    });
  });

  let inView = $derived(layout.rows.filter((r) => r.y + r.h >= scrollTop - 200 && r.y <= scrollTop + height + 200));
  // Cards are keyed by clip, not by row, so a filter or resize moves the
  // cards that stay (they glide to their new place) instead of rebuilding them.
  type Item =
    | { kind: "arriving"; key: string; y: number }
    | { kind: "header"; key: string; y: number; session: Session }
    | { kind: "card"; key: string; y: number; x: number; clip: Clip; index: number };
  let items = $derived(
    width === 0
      ? []
      : inView.flatMap((row): Item[] =>
          row.kind === "arriving"
            ? [{ kind: "arriving", key: row.key, y: row.y }]
            : row.kind === "header"
              ? [{ kind: "header", key: row.key, y: row.y, session: row.session }]
              : row.clips.map((clip, col) => ({ kind: "card", key: clip.path, y: row.y, x: PAD + col * (cardW + GAP), clip, index: row.first + col })),
        ),
  );

  // Cards mount and unmount all the time while scrolling, so they only animate
  // when the list itself changes: its first show (staggered) and a filter.
  // Not on the way back from the editor: that's the clip's moment.
  let listMotion: "intro" | "filter" | null = null;
  if (!introduced && !libraryView.opened) listMotion = "intro";
  introduced ||= !!libraryView.opened;

  $effect(() => {
    if (listMotion !== "intro" || library.loading || width === 0) return;
    introduced = true;
    const timer = setTimeout(() => (listMotion = null), DUR.spring * 2);
    return () => clearTimeout(timer);
  });

  let listKey = $derived([filters.game, filters.kind, filters.event, filters.search, filters.sort, grouped].join("\n"));
  let seenKey: string | null = null;
  let seenCount = 0;
  let settle: ReturnType<typeof setTimeout>;

  // Before the DOM changes: the cards leaving and arriving read `listMotion`.
  // A deleted or unfavorited clip moves the list too, without changing the filters.
  $effect.pre(() => {
    const key = listKey;
    const count = visible.length;
    const refiltered = seenKey !== null && seenKey !== key;
    const moved = refiltered || count !== seenCount;
    const first = seenKey === null;
    seenKey = key;
    seenCount = count;
    if (first || !moved || (listMotion === "intro" && !refiltered)) return;
    listMotion = "filter";
    clearTimeout(settle);
    settle = setTimeout(() => (listMotion = null), DUR.spring);
    if (!refiltered) return;
    // A new set of clips starts at its top.
    if (scroller) scroller.scrollTop = 0;
    scrollTop = libraryView.scrollTop = 0;
  });

  function appear(_: Element, { index }: { index: number }): TransitionConfig {
    if (reduced()) return { duration: 0 };
    if (listMotion === "intro") {
      return { delay: stagger(index), duration: DUR.slow, easing: easeOut, css: (t, u) => `opacity: ${t}; translate: 0 ${u * 10}px` };
    }
    if (listMotion === "filter") return { delay: DUR.fast / 2, duration: DUR.base, easing: easeOut, css: (t) => `opacity: ${t}` };
    return { duration: 0 };
  }

  function vanish(_: Element): TransitionConfig {
    // Leaving the page: a card that lingers would be in the shell's new picture.
    if (reduced() || listMotion !== "filter" || ui.clip || ui.page !== "clips") return { duration: 0 };
    return { duration: DUR.fast, easing: easeOut, css: (t) => `opacity: ${t}` };
  }

  let selectedClips = $derived(flat.filter((c) => selected.has(c.path)));
  let selectedSize = $derived(selectedClips.reduce((s, c) => s + c.sizeBytes, 0));
  let allFavorite = $derived(selectedClips.length > 0 && selectedClips.every((c) => c.favorite));

  let title = $derived(filters.game ?? (filters.kind === "favorites" ? "Favorites" : "All clips"));

  /** A plain click opens the clip; once something is selected, it changes the selection. */
  function clickCard(e: MouseEvent, i: number) {
    if (e.shiftKey || e.ctrlKey) select(e, i);
    else if (selected.size > 0) toggle(i);
    else open(flat[i], e);
  }

  function open(clip: Clip, e?: MouseEvent) {
    if (e) ignoreDoubleClick(e);
    openClip(clip);
  }

  function select(e: MouseEvent, i: number) {
    const path = flat[i].path;
    if (e.shiftKey && anchor != null) {
      if (!e.ctrlKey) selected.clear();
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      for (let k = a; k <= b; k++) selected.add(flat[k].path);
    } else if (e.ctrlKey) {
      toggle(i);
    } else {
      selected.clear();
      selected.add(path);
      anchor = i;
    }
  }

  function toggle(i: number) {
    const path = flat[i].path;
    if (selected.has(path)) selected.delete(path);
    else selected.add(path);
    anchor = i;
  }

  function selectSession(s: Session) {
    const all = s.clips.every((c) => selected.has(c.path));
    for (const c of s.clips) {
      if (all) selected.delete(c.path);
      else selected.add(c.path);
    }
  }

  // Drag a box over empty space to select (hold Ctrl to add to the selection).
  let drag = $state<{ x0: number; y0: number; x1: number; y1: number; base: string[] } | null>(null);

  function contentPoint(e: PointerEvent) {
    const r = scroller.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top + scroller.scrollTop };
  }

  function dragStart(e: PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".card, button")) return;
    const p = contentPoint(e);
    drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y, base: e.ctrlKey ? [...selected] : [] };
    if (!e.ctrlKey) selected.clear();
    scroller.setPointerCapture(e.pointerId);
  }

  function dragMove(e: PointerEvent) {
    if (!drag) return;
    const r = scroller.getBoundingClientRect();
    // Scroll when dragging near the top or bottom edge.
    if (e.clientY > r.bottom - 40) scroller.scrollTop += 24;
    else if (e.clientY < r.top + 40) scroller.scrollTop -= 24;
    const p = contentPoint(e);
    drag.x1 = p.x;
    drag.y1 = p.y;
    const [left, right] = [Math.min(drag.x0, drag.x1), Math.max(drag.x0, drag.x1)];
    const [top, bottom] = [Math.min(drag.y0, drag.y1), Math.max(drag.y0, drag.y1)];
    selected.clear();
    for (const p of drag.base) selected.add(p);
    for (const row of layout.rows) {
      if (row.kind !== "cards" || row.y > bottom || row.y + cardH < top) continue;
      row.clips.forEach((c, col) => {
        const x = PAD + col * (cardW + GAP);
        if (x < right && x + cardW > left) selected.add(c.path);
      });
    }
  }

  function dragEnd(e: PointerEvent) {
    if (!drag) return;
    drag = null;
    scroller.releasePointerCapture(e.pointerId);
  }

  // The toolbar's two popovers.
  let menu = $state<"filter" | "view" | null>(null);
  let menuEl = $state<HTMLElement>();
  let opener: HTMLElement | null = null;

  function toggleMenu(which: "filter" | "view", e: MouseEvent) {
    menu = menu === which ? null : which;
    if (!menu) return;
    opener = e.currentTarget as HTMLElement;
    tick().then(() => menuEl?.querySelector<HTMLElement>('[aria-checked="true"], button:not(:disabled)')?.focus({ preventScroll: true }));
  }

  function closeMenu(refocus = false) {
    menu = null;
    if (refocus) opener?.focus();
  }

  function outside(e: PointerEvent) {
    if (!menu || (e.target as HTMLElement).closest(".anchor")) return;
    closeMenu();
    swallowPress(e);
  }

  function onKey(e: KeyboardEvent) {
    if (menu) {
      if (e.key === "Escape") {
        e.preventDefault();
        closeMenu(true);
      } else if (menu === "view" && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        e.preventDefault();
        const items = [...(menuEl?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
        const step = e.key === "ArrowDown" ? 1 : -1;
        items[(items.indexOf(document.activeElement as HTMLElement) + step + items.length) % items.length]?.focus();
      }
      return;
    }
    const tag = (e.target as HTMLElement).tagName;
    if (tag === "INPUT" || tag === "SELECT") return;
    if (e.ctrlKey && e.key.toLowerCase() === "a") {
      e.preventDefault();
      for (const c of flat) selected.add(c.path);
    } else if (e.key === "Delete") {
      deleteClips([...selected]);
    } else if (e.key === "Escape") {
      selected.clear();
    } else if (e.key === "Enter" && selectedClips.length > 0 && tag !== "BUTTON") {
      openClip(selectedClips[0]);
    }
  }

  function sessionTitle(s: Session) {
    return s.map ? mapName(s.map) : s.game;
  }

  function sessionTime(s: Session) {
    const start = clock(s.start);
    const end = clock(s.end);
    return start === end ? `${day(s.start)} ${start}` : `${day(s.start)} ${start} – ${end}`;
  }

  /** The one quiet line; the rest waits in its tooltip. */
  function sessionMeta(s: Session) {
    return [s.mode ? modeName(s.mode) : "", `${day(s.start)} ${clock(s.start)}`].filter(Boolean).join(" · ");
  }

  function sessionTip(s: Session) {
    return [s.map ? s.game : "", sessionTime(s), s.clips.length === 1 ? "1 clip" : `${s.clips.length} clips`].filter(Boolean).join(" · ");
  }

  /** What the list is sorted by, on the card, when it isn't time. */
  function statOf(c: Clip) {
    if (filters.sort === "largest") return size(c.sizeBytes);
    if (filters.sort === "longest" && c.durationSecs != null) return duration(c.durationSecs);
    return undefined;
  }

  const kinds: [Kind, string][] = [
    ["all", "All"],
    ["auto", "Auto"],
    ["manual", "Manual"],
  ];

  const SORTS: [Sort, string][] = [
    ["newest", "Newest"],
    ["oldest", "Oldest"],
    ["longest", "Longest"],
    ["largest", "Largest"],
  ];

  let kindFilter = $derived(filters.kind === "auto" || filters.kind === "manual" ? filters.kind : null);
  let filtered = $derived(!!kindFilter || !!filters.event);
  let canFilter = $derived(filters.kind !== "favorites" || chips.length > 0);
  let hasClips = $derived(library.loading || library.clips.length > 0);
  let sortName = $derived(SORTS.find(([s]) => s === filters.sort)?.[1] ?? "");
  let skeletons = $derived(Math.min(24, cols * Math.ceil((height + GAP) / (cardH + GAP))));

  function clearFilters() {
    filters.event = null;
    filters.search = "";
    if (filters.kind !== "favorites") filters.kind = "all";
  }
</script>

<svelte:window onkeydown={onKey} onpointerdowncapture={outside} />

<section class="library">
  <header>
    <div class="title">
      {#key title}
        <h1 in:enter={{ y: 4, duration: DUR.base }}>{title}</h1>
      {/key}
      {#if !library.loading}
        <span class="count mono" aria-label="{visible.length} {visible.length === 1 ? 'clip' : 'clips'}">{visible.length}</span>
      {/if}
    </div>

    {#if kindFilter}
      <button class="token" aria-label="Remove filter: {kindFilter}" in:enter={{ x: -6, y: 0, duration: DUR.base }} out:leave onclick={() => (filters.kind = "all")}>
        {kindFilter === "auto" ? "Auto" : "Manual"}
        <Icon name="x" size={12} />
      </button>
    {/if}
    {#if filters.event}
      {@const event = filters.event}
      <button class="token" aria-label="Remove filter: {eventName(event)}" in:enter={{ x: -6, y: 0, duration: DUR.base }} out:leave onclick={() => (filters.event = null)}>
        <Icon {event} size={14} />
        {eventName(event)}
        <Icon name="x" size={12} />
      </button>
    {/if}

    <span class="spring"></span>

    {#if hasClips}
      <label class="search">
        <Icon name="search" size={16} />
        <input type="search" placeholder="Search" aria-label="Search clips" bind:value={filters.search} />
        {#if filters.search}
          <button class="ghost clear" aria-label="Clear search" onclick={() => (filters.search = "")}><Icon name="x" size={14} /></button>
        {/if}
      </label>
      {#if canFilter}
        <div class="anchor">
          <button
            class="ghost tool"
            class:icon={narrow}
            class:on={filtered}
            class:open={menu === "filter"}
            aria-label="Filter"
            aria-haspopup="dialog"
            aria-expanded={menu === "filter"}
            data-tip={narrow ? "Filter" : undefined}
            onclick={(e) => toggleMenu("filter", e)}
          >
            <Icon name="sliders" size={16} />
            {#if !narrow}Filter{/if}
          </button>
          {#if menu === "filter"}
            <div class="popover filter" bind:this={menuEl} role="dialog" aria-label="Filter clips" in:pop out:pop={{ duration: DUR.fast }}>
              {#if filters.kind !== "favorites"}
                <Segmented label="Clip type" value={filters.kind} options={kinds} onchange={(k) => (filters.kind = k)} />
              {/if}
              {#if chips.length > 0}
                <div class="chips">
                  {#each chips as [kind, n] (kind)}
                    <button class="chip" class:on={filters.event === kind} aria-pressed={filters.event === kind} onclick={() => (filters.event = filters.event === kind ? null : kind)}>
                      <Icon event={kind} size={16} />
                      {eventName(kind)}
                      <span class="n mono">{n}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}

    <div class="anchor">
      <button
        class="ghost tool"
        class:icon={narrow}
        class:open={menu === "view"}
        aria-label="Sort and view"
        aria-haspopup="menu"
        aria-expanded={menu === "view"}
        data-tip={narrow ? `Sorted by ${sortName.toLowerCase()}` : undefined}
        onclick={(e) => toggleMenu("view", e)}
      >
        <Icon name="sort" size={16} />
        {#if !narrow}{sortName}{/if}
      </button>
      {#if menu === "view"}
        <div class="popover view" bind:this={menuEl} role="menu" aria-label="Sort and view" in:pop out:pop={{ duration: DUR.fast }}>
          {#each SORTS as [s, name] (s)}
            <button
              class="row"
              role="menuitemradio"
              aria-checked={filters.sort === s}
              onclick={() => {
                filters.sort = s;
                closeMenu(true);
              }}
            >
              <span class="grow">{name}</span>
              {#if filters.sort === s}<Icon name="check" size={16} />{/if}
            </button>
          {/each}
          <hr />
          <button
            class="row"
            role="menuitemcheckbox"
            aria-checked={prefs.groupSessions}
            disabled={!timeOrder}
            data-tip={timeOrder ? undefined : "Only when sorted by date"}
            onclick={() => setPref("groupSessions", !prefs.groupSessions)}
          >
            <span class="grow">Group by session</span>
            {#if prefs.groupSessions}<Icon name="check" size={16} />{/if}
          </button>
          <button
            class="row"
            role="menuitem"
            onclick={() => {
              reload();
              closeMenu(true);
            }}
          >
            <span class="grow">Refresh</span>
            <Icon name="refresh" size={16} />
          </button>
        </div>
      {/if}
    </div>
  </header>

  <div class="list">
    <div class="edge" class:on={scrollTop > 2}></div>
    <div
      class="scroller"
      bind:this={scroller}
      bind:clientWidth={width}
      bind:clientHeight={height}
      onscroll={() => (scrollTop = libraryView.scrollTop = scroller.scrollTop)}
      onpointerdown={dragStart}
      onpointermove={dragMove}
      onpointerup={dragEnd}
      onpointercancel={dragEnd}
      role="listbox"
      aria-multiselectable="true"
      tabindex="0"
    >
      <div class="content" style="height: {layout.height + (selected.size > 0 ? SELECTION_H : 0)}px">
        {#each items as item (item.key)}
          {#if item.kind === "arriving"}
            <div class="arriving" style="transform: translateY({item.y}px); left: {PAD}px; right: {PAD}px; gap: {GAP}px">
              {#each arriving.slice(0, cols) as a (a.id)}
                <ArrivingCard arrival={a} w={cardW} />
              {/each}
            </div>
          {:else if item.kind === "header"}
            {@const s = item.session}
            <div
              class="session"
              title={sessionTip(s)}
              style="transform: translateY({item.y}px); left: {PAD}px; right: {PAD}px; height: {HEADER_H}px"
              in:appear|global={{ index: 0 }}
              out:vanish|global
            >
              <span class="s-title">{sessionTitle(s)}</span>
              <span class="s-meta">{sessionMeta(s)}</span>
              <button class="ghost s-select" class:shown={selected.size > 0} onclick={() => selectSession(s)}>
                {s.clips.every((c) => selected.has(c.path)) ? "Deselect" : "Select all"}
              </button>
            </div>
          {:else}
            {@const clip = item.clip}
            {@const i = item.index}
            <ClipCard
              {clip}
              x={item.x}
              y={item.y}
              w={cardW}
              index={i}
              stat={statOf(clip)}
              dated={!grouped}
              {appear}
              {vanish}
              selected={selected.has(clip.path)}
              fresh={incoming.fresh.includes(clip.path)}
              selecting={selected.size > 0}
              onselect={(e) => clickCard(e, i)}
              ontoggle={() => toggle(i)}
              onopen={(e) => open(clip, e)}
              onfavorite={() => setFavorite([clip.path], !clip.favorite)}
            />
          {/if}
        {/each}
        {#if drag}
          <div
            class="marquee"
            style="left: {Math.min(drag.x0, drag.x1)}px; top: {Math.min(drag.y0, drag.y1)}px; width: {Math.abs(drag.x1 - drag.x0)}px; height: {Math.abs(drag.y1 - drag.y0)}px"
          ></div>
        {/if}
      </div>

      {#if library.loading && width > 0}
        <div class="loading" aria-hidden="true" out:pageOut>
          <div class="skeleton text" style="width: 180px"></div>
          <div class="ghosts" style="grid-template-columns: repeat({cols}, {cardW}px); gap: {GAP}px">
            {#each { length: skeletons } as _, i (i)}
              <div class="skeleton ghost-card" style="height: {cardH}px; --delay: {stagger(i)}ms"></div>
            {/each}
          </div>
        </div>
      {:else if !library.loading && visible.length === 0 && arriving.length === 0}
        <div class="empty" in:enter={{ y: 8, delay: DUR.fast }}>
          {#if library.error}
            <span class="glyph"><Icon name="warning" size={24} /></span>
            <h2>Couldn't read the clips folder</h2>
            <p class="selectable">{library.error}</p>
            <button onclick={reload}>Try again</button>
          {:else if library.clips.length === 0}
            <span class="glyph"><Icon event="manual" size={32} /></span>
            <h2>No clips yet</h2>
            {#if recorder.manual}
              <p>Press <kbd>{recorder.hotkey}</kbd> in a game to save the last {recorder.lengthSecs} seconds.</p>
            {/if}
            <p>Highlights in CS2, League of Legends and Rocket League are saved on their own.</p>
          {:else if filters.kind === "favorites" && !filters.search && !filters.event}
            <span class="glyph"><Icon name="star" size={24} /></span>
            <h2>No favorites yet</h2>
            <p>Star a clip to keep it here.</p>
          {:else}
            <span class="glyph"><Icon name="search" size={24} /></span>
            <h2>No matches</h2>
            <button onclick={clearFilters}>Clear filters</button>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if selected.size > 0}
    <div class="dock">
      <div class="selection" role="toolbar" aria-label="Selected clips" in:enter={{ y: 16, duration: DUR.spring, easing: spring }} out:leave={{ y: 12 }}>
        <span class="what"><b>{selected.size}</b> selected <span class="mono">{size(selectedSize)}</span></span>
        <button class="ghost" onclick={() => setFavorite([...selected], !allFavorite)}><Icon name="star" size={16} />{allFavorite ? "Unfavorite" : "Favorite"}</button>
        <button class="ghost" onclick={() => api.reveal(selectedClips[0].path)}><Icon name="folder" size={16} />Show in folder</button>
        <button class="ghost danger" onclick={() => deleteClips([...selected])}><Icon name="trash" size={16} />Delete</button>
        <button class="ghost icon" aria-label="Clear selection" title="Clear selection (Esc)" onclick={() => selected.clear()}><Icon name="x" size={16} /></button>
      </div>
    </div>
  {/if}
</section>

<style>
  .library {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* One line. It floats above the list so its popovers can too. */
  header {
    position: relative;
    z-index: 10;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 36px;
    padding: 0 var(--sp-5) var(--sp-3);
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
    margin-right: var(--sp-2);
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--fs-lg);
    font-weight: 600;
    letter-spacing: -0.03em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .count {
    font-size: var(--fs-sm);
    color: var(--faint);
  }

  .spring {
    flex: 1;
  }

  /* An active filter says what's narrowing the list, and undoes itself. */
  .token {
    flex-shrink: 0;
    gap: 6px;
    height: 26px;
    padding: 0 var(--sp-2);
    border-radius: var(--r-full);
    font-size: var(--fs-sm);
    background: var(--accent-soft);
    white-space: nowrap;
  }

  .token:hover {
    background: color-mix(in srgb, var(--accent) 26%, transparent);
  }

  .token :global(.icon:last-child) {
    color: var(--muted);
  }

  /* The first to give way when the line fills up. */
  .search {
    flex: 0 100 240px;
    min-width: 112px;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 34px;
    padding: 0 var(--sp-2) 0 10px;
    border-radius: var(--r-sm);
    background: var(--surface);
    color: var(--muted);
    outline: 2px solid transparent;
    outline-offset: -2px;
    transition:
      background var(--dur-fast),
      outline-color var(--dur-fast);
  }

  .search:hover {
    background: var(--surface-2);
  }

  .search:focus-within {
    background: var(--surface-2);
    outline-color: var(--line-strong);
  }

  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    outline: none;
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .clear {
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    padding: 0;
    border-radius: var(--r-xs);
  }

  .anchor {
    position: relative;
    flex-shrink: 0;
  }

  .tool {
    height: 34px;
    padding: 0 10px;
    color: var(--muted);
    font-size: var(--fs-control);
    white-space: nowrap;
  }

  .tool.icon {
    width: 34px;
    padding: 0;
  }

  .tool:hover,
  .tool.open {
    color: var(--text);
  }

  .tool.open {
    background: var(--hover);
  }

  .tool.on {
    color: var(--text);
  }

  .popover {
    position: absolute;
    top: calc(100% + var(--sp-1));
    right: 0;
    transform-origin: top right;
    border-radius: var(--r-md);
    background: var(--surface-2);
    box-shadow: var(--shadow-2);
  }

  .popover.filter {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-3);
    width: 320px;
    padding: var(--sp-3);
  }

  .popover.filter :global(.segmented) {
    background: var(--surface);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }

  .chip {
    gap: 6px;
    height: 30px;
    padding: 0 10px 0 var(--sp-2);
    border-radius: var(--r-full);
    font-size: var(--fs-control);
    white-space: nowrap;
    background: var(--surface);
  }

  .chip:hover {
    background: var(--surface-3);
  }

  .chip .n {
    font-size: var(--fs-xs);
    color: var(--faint);
  }

  .chip.on {
    background: var(--accent-soft);
  }

  .chip.on .n {
    color: var(--text);
  }

  .popover.view {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 196px;
    padding: var(--sp-1);
  }

  .row {
    justify-content: flex-start;
    height: 32px;
    padding: 0 var(--sp-2);
    border-radius: var(--r-xs);
    background: transparent;
    font-size: var(--fs-control);
    text-align: left;
  }

  .row:hover {
    background: var(--hover);
  }

  .row:active {
    transform: none;
  }

  .grow {
    flex: 1;
  }

  .row :global(.icon) {
    color: var(--muted);
  }

  .popover hr {
    width: 100%;
    margin: var(--sp-1) 0;
    border: none;
    border-top: 1px solid var(--line);
  }

  .list {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  /* Clips pass under the toolbar instead of being cut by a line. */
  .edge {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--sp-4);
    z-index: 2;
    pointer-events: none;
    background: linear-gradient(to bottom, var(--bg), transparent);
    opacity: 0;
    transition: opacity var(--dur);
  }

  .edge.on {
    opacity: 1;
  }

  .scroller {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    outline: none;
  }

  .scroller:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--line-strong);
  }

  .content {
    position: relative;
  }

  .session {
    position: absolute;
    top: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-control);
    transition: transform var(--dur-spring) var(--spring);
  }

  .arriving {
    position: absolute;
    top: 0;
    display: flex;
    transition: transform var(--dur-spring) var(--spring);
  }

  .s-title {
    font-weight: 600;
  }

  .s-meta {
    min-width: 0;
    color: var(--faint);
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .s-select {
    flex-shrink: 0;
    margin-left: auto;
    padding: 3px var(--sp-2);
    font-size: var(--fs-sm);
    color: var(--muted);
    opacity: 0;
  }

  .session:hover .s-select,
  .s-select.shown,
  .s-select:focus-visible {
    opacity: 1;
  }

  .marquee {
    position: absolute;
    border-radius: var(--r-xs);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent);
    pointer-events: none;
  }

  .loading {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    padding: 14px var(--sp-5) 0;
  }

  .ghosts {
    display: grid;
    margin-top: 14px;
  }

  .ghost-card {
    border-radius: var(--r-md);
    animation: fade var(--dur-slow) var(--ease-out) var(--delay, 0ms) backwards;
  }

  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    padding: var(--sp-6);
    padding-bottom: 72px;
    text-align: center;
  }

  .glyph {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    margin-bottom: var(--sp-2);
    border-radius: 50%;
    background: var(--surface);
    color: var(--muted);
  }

  .empty h2 {
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
  }

  .empty p {
    margin: 0;
    max-width: 44ch;
    color: var(--muted);
    font-size: var(--fs-control);
    line-height: 1.5;
  }

  .empty button {
    margin-top: var(--sp-2);
  }

  .selectable {
    user-select: text;
  }

  /* The bar itself holds still for its enter and leave. */
  .dock {
    position: absolute;
    left: 0;
    right: 0;
    bottom: var(--sp-4);
    z-index: 5;
    display: flex;
    justify-content: center;
    padding: 0 var(--sp-3);
    pointer-events: none;
  }

  .selection {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px;
    border-radius: var(--r-md);
    background: var(--surface-3);
    box-shadow: var(--shadow-3);
    white-space: nowrap;
    pointer-events: auto;
  }

  .what {
    margin: 0 var(--sp-3) 0 10px;
    color: var(--muted);
  }

  .what b {
    color: var(--text);
    font-weight: 600;
  }

  .what .mono {
    margin-left: var(--sp-1);
    font-size: var(--fs-sm);
    color: var(--faint);
  }

  .selection button:not(.icon) {
    height: 34px;
    padding: 0 var(--sp-3) 0 10px;
    font-size: var(--fs-control);
  }
</style>
