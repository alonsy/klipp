// Icons. Event icons follow the brand sheet (assets/brand/event-icons.dc.html):
// a 100 grid, round caps, white strokes and one accent element. The stroke
// gets heavier at small sizes (9 → 10 at 24 px → 11 at 20 px and below).
// Kill, headshot, death, victory, defeat, defuse and manual are the
// designer's; multi-kill, ace, MVP, knife and Zeus are composed in the same
// style, and so are League's and Rocket League's (team ace to demolition).

const A = "var(--accent)";

/** Markup on the 100 grid; `{w}` is replaced by the stroke width. */
const EVENTS: Record<string, string> = {
  kill: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round"><circle cx="50" cy="50" r="27"/><path d="M50 8 V22 M50 78 V92 M8 50 H22 M78 50 H92"/></g><circle cx="50" cy="50" r="9" fill="${A}"/>`,
  headshot: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><circle cx="50" cy="36" r="21"/><path d="M16 90 C16 72 31 65 50 65 C69 65 84 72 84 90"/></g><circle cx="50" cy="36" r="8" fill="${A}"/>`,
  death: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M30 66 C21 59 18 50 18 42 C18 24 32 12 50 12 C68 12 82 24 82 42 C82 50 79 59 70 66 V86 H30 Z"/><path d="M44 74 V86 M56 74 V86"/></g><circle cx="38" cy="44" r="8" fill="${A}"/><circle cx="62" cy="44" r="8" fill="${A}"/>`,
  victory: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M32 14 H68 V38 C68 50 60 58 50 58 C40 58 32 50 32 38 Z"/><path d="M32 22 H18 C18 35 24 42 32 42 M68 22 H82 C82 35 76 42 68 42"/><path d="M50 58 V86 M34 86 H66"/></g><circle cx="50" cy="34" r="8" fill="${A}"/>`,
  defeat: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M28 22 V90"/><path d="M28 50 H74 L63 64 L74 78 H28"/></g><circle cx="28" cy="11" r="8" fill="${A}"/>`,
  defuse: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><circle cx="42" cy="62" r="26"/><path d="M60 44 L65 39"/><path d="M76 28 C80 22 84 18 90 16"/></g><path d="M61 22 L80 41" stroke="${A}" stroke-width="{w}" stroke-linecap="round"/>`,
  manual: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><rect x="14" y="22" width="72" height="56" rx="16"/></g><circle cx="50" cy="50" r="13" fill="${A}"/>`,
  // Two crosshairs, the front one carrying the accent: more than one kill.
  multikill: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round"><path d="M64 14 A27 27 0 0 1 86 36"/><circle cx="44" cy="56" r="26"/><path d="M44 16 V26 M44 86 V96 M4 56 H14 M74 56 H84"/></g><circle cx="44" cy="56" r="9" fill="${A}"/>`,
  // The whole team: a crown with an accent jewel.
  ace: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M16 74 L12 30 L34 50 L50 22 L66 50 L88 30 L84 74 Z"/><path d="M20 89 H80"/></g><circle cx="50" cy="60" r="8" fill="${A}"/>`,
  mvp: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M50 12 L60.6 37.4 L88 39.6 L67.1 57.6 L73.5 84.4 L50 70 L26.5 84.4 L32.9 57.6 L12 39.6 L39.4 37.4 Z"/></g><circle cx="50" cy="52" r="8" fill="${A}"/>`,
  // Fortnite's knock: the crosshair, and an accent chevron for someone going down.
  knock: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round"><circle cx="50" cy="42" r="26"/><path d="M50 4 V16 M10 42 H22 M78 42 H90"/></g><path d="M34 76 L50 92 L66 76" fill="none" stroke="${A}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"/>`,
  // Blade and handle in white, the guard in accent.
  knife: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M42 58 L86 14 C84 36 70 54 52 68 Z"/><path d="M16 84 L32 68"/></g><path d="M26 56 L44 74" stroke="${A}" stroke-width="{w}" stroke-linecap="round"/>`,
  // A bolt in accent between two white charge arcs.
  zeus: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M20 30 C12 42 12 58 20 70 M80 30 C88 42 88 58 80 70"/></g><path d="M58 10 L32 54 H52 L42 90 L70 44 H50 Z" fill="none" stroke="${A}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"/>`,
  // Three of the team, the middle one carrying the accent.
  teamace: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><circle cx="50" cy="34" r="14"/><path d="M24 88 C24 70 35 62 50 62 C65 62 76 70 76 88"/><circle cx="18" cy="44" r="9"/><circle cx="82" cy="44" r="9"/><path d="M6 82 C6 72 10 66 18 66 M94 82 C94 72 90 66 82 66"/></g><circle cx="50" cy="34" r="6" fill="${A}"/>`,
  // A drop, with an accent shine.
  firstblood: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M50 10 C38 28 22 46 22 62 C22 78 35 90 50 90 C65 90 78 78 78 62 C78 46 62 28 50 10 Z"/></g><circle cx="50" cy="64" r="9" fill="${A}"/>`,
  // A gem, taken away along the accent arrow.
  steal: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M42 12 L68 38 L42 70 L16 38 Z"/><path d="M16 38 H68"/></g><path d="M56 84 H88 M78 74 L88 84 L78 94" fill="none" stroke="${A}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"/>`,
  // A tower, its window in accent.
  objective: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M34 88 L38 40 H62 L66 88 Z"/><path d="M30 40 V22 H40 V30 H50 V22 H60 V30 H70 V22 V40"/></g><circle cx="50" cy="60" r="7" fill="${A}"/>`,
  // A pass along an arc, from the accent.
  assist: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M24 70 C34 30 66 30 82 62"/><path d="M68 60 L82 62 L84 48"/></g><circle cx="20" cy="80" r="9" fill="${A}"/>`,
  // The ball (accent) in the goal.
  goal: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M12 84 V26 H88 V84"/></g><circle cx="50" cy="62" r="13" fill="${A}"/>`,
  save: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M50 10 L82 22 V46 C82 66 68 82 50 90 C32 82 18 66 18 46 V22 Z"/></g><circle cx="50" cy="48" r="9" fill="${A}"/>`,
  // The save's shield with an accent star.
  epicsave: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M50 10 L82 22 V46 C82 66 68 82 50 90 C32 82 18 66 18 46 V22 Z"/></g><path d="M50 32 L54.1 42.3 L65.2 43.1 L56.7 50.2 L59.4 60.9 L50 55 L40.6 60.9 L43.3 50.2 L34.8 43.1 L45.9 42.3 Z" fill="${A}"/>`,
  // A burst around an accent core.
  demolition: `<g fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"><path d="M50 10 L58.4 29.7 L78.3 21.7 L70.3 41.6 L90 50 L70.3 58.4 L78.3 78.3 L58.4 70.3 L50 90 L41.6 70.3 L21.7 78.3 L29.7 58.4 L10 50 L29.7 41.6 L21.7 21.7 L41.6 29.7 Z"/></g><circle cx="50" cy="50" r="9" fill="${A}"/>`,
};

/** Labels drawn with another label's icon: League's pentakill is five kills like an ace. */
const SAME_ICON: Record<string, string> = { penta: "ace", win: "victory", loss: "defeat" };

/** The icon for an event name from clip info ("2k", "headshot", "victory"…). */
export function eventIconId(kind: string): string {
  if (/^\d+k$/.test(kind)) return "multikill";
  // A hotkey press: the record mark.
  if (kind === "capture") return "manual";
  if (SAME_ICON[kind]) return SAME_ICON[kind];
  return kind in EVENTS ? kind : "kill";
}

export function eventSvg(kind: string, size: number): string {
  const w = size <= 20 ? 11 : size <= 28 ? 10 : 9;
  const body = EVENTS[eventIconId(kind)].replaceAll("{w}", String(w));
  return `<svg viewBox="0 0 100 100" width="${size}" height="${size}" aria-hidden="true">${body}</svg>`;
}

/** Interface icons on a 24 grid, drawn with the same round strokes. */
const UI: Record<string, string> = {
  grid: `<rect x="4" y="4" width="7" height="7" rx="2"/><rect x="13" y="4" width="7" height="7" rx="2"/><rect x="4" y="13" width="7" height="7" rx="2"/><rect x="13" y="13" width="7" height="7" rx="2"/>`,
  monitor: `<rect x="3" y="4.5" width="18" height="12" rx="2.5"/><path d="M8.5 20h7M12 16.5V20"/>`,
  gamepad: `<path d="M7 8h10a5 5 0 0 1 4.6 7l-.5 1.1a2.6 2.6 0 0 1-4.3.6L15 15H9l-1.8 1.7a2.6 2.6 0 0 1-4.3-.6L2.4 15A5 5 0 0 1 7 8z"/><path d="M7.5 10.5v3M6 12h3"/><circle cx="16" cy="11" r=".6" fill="currentColor"/><circle cx="17.5" cy="13" r=".6" fill="currentColor"/>`,
  crosshair: `<circle cx="12" cy="12" r="6.5"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3"/>`,
  star: `<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z"/>`,
  sliders: `<path d="M4 7h9M17 7h3M4 12h3M11 12h9M4 17h11M19 17h1"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="17" r="2"/>`,
  folder: `<path d="M3.5 7A1.5 1.5 0 0 1 5 5.5h4.2l2 2.3H19A1.5 1.5 0 0 1 20.5 9.3v8.2A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z"/>`,
  search: `<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5"/>`,
  play: `<path d="M8 5.8v12.4a.8.8 0 0 0 1.2.7l10-6.2a.8.8 0 0 0 0-1.4l-10-6.2A.8.8 0 0 0 8 5.8z" fill="currentColor"/>`,
  pause: `<rect x="6.5" y="5" width="3.6" height="14" rx="1" fill="currentColor"/><rect x="13.9" y="5" width="3.6" height="14" rx="1" fill="currentColor"/>`,
  back: `<path d="M15 5l-7 7 7 7"/>`,
  trash: `<path d="M4.5 7h15M10 11v5.5M14 11v5.5M6.5 7l.9 11.6A1.6 1.6 0 0 0 9 20h6a1.6 1.6 0 0 0 1.6-1.4L17.5 7M9.5 7V4.5h5V7"/>`,
  check: `<path d="M5 12.5l4.5 4.5L19 7.5"/>`,
  x: `<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>`,
  loop: `<path d="M17 3.5l3 3-3 3"/><path d="M4 11.5v-1A4 4 0 0 1 8 6.5h12"/><path d="M7 20.5l-3-3 3-3"/><path d="M20 12.5v1a4 4 0 0 1-4 4H4"/>`,
  scissors: `<circle cx="6" cy="6.5" r="2.8"/><circle cx="6" cy="17.5" r="2.8"/><path d="M8.4 8L20 18M8.4 16L20 6"/>`,
  export: `<path d="M12 15V3.5M7.5 8L12 3.5 16.5 8"/><path d="M5 13v5.5A1.5 1.5 0 0 0 6.5 20h11a1.5 1.5 0 0 0 1.5-1.5V13"/>`,
  send: `<path d="M20.5 3.5L10 14M20.5 3.5l-6.5 17-4-6.5-6.5-4z"/>`,
  copy: `<rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.5"/><path d="M15.5 8.5V6A2 2 0 0 0 13.5 4H6A2 2 0 0 0 4 6v7.5a2 2 0 0 0 2 2h2.5"/>`,
  volume: `<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4.5 4.5 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11"/>`,
  mute: `<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>`,
  refresh: `<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4.5H15"/>`,
  setStart: `<path d="M5 4.5v15M19 12H9M12.5 8.5L9 12l3.5 3.5"/>`,
  setEnd: `<path d="M19 4.5v15M5 12h10M11.5 8.5L15 12l-3.5 3.5"/>`,
  prevFrame: `<path d="M6 5.5v13M18 6l-8 6 8 6z"/>`,
  nextFrame: `<path d="M18 5.5v13M6 6l8 6-8 6z"/>`,
  info: `<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.8v.2"/>`,
  warning: `<path d="M12 4L2.8 19.5h18.4z"/><path d="M12 10v4.5M12 17.2v.2"/>`,
  power: `<path d="M12 3.5v8"/><path d="M7 6.5a7.5 7.5 0 1 0 10 0"/>`,
  keyboard: `<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M8 14h8"/>`,
  film: `<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M8 4.5v15M16 4.5v15M3.5 9.5H8M3.5 14.5H8M16 9.5h4.5M16 14.5h4.5"/>`,
  mic: `<rect x="9" y="3.5" width="6" height="10.5" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>`,
  bell: `<path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 1.5h-15z"/><path d="M10 20.5a2.2 2.2 0 0 0 4 0"/>`,
  drive: `<rect x="3" y="13" width="18" height="6.5" rx="2"/><path d="M5 13l2.5-7.5h9L19 13"/><circle cx="16.5" cy="16.3" r=".6" fill="currentColor"/>`,
  bolt: `<path d="M13 3L5 13.5h6L10 21l8-10.5h-6z"/>`,
  record: `<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="3.5" fill="currentColor"/>`,
  fullscreen: `<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"/>`,
  exitFullscreen: `<path d="M9 4v3.5A1.5 1.5 0 0 1 7.5 9H4M20 9h-3.5A1.5 1.5 0 0 1 15 7.5V4M15 20v-3.5a1.5 1.5 0 0 1 1.5-1.5H20M4 15h3.5A1.5 1.5 0 0 1 9 16.5V20"/>`,
  volumeLow: `<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4.5 4.5 0 0 1 0 6"/>`,
};

export type UiIcon = keyof typeof UI;

export function uiSvg(name: string, size: number, stroke = 1.9): string {
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${UI[name] ?? ""}</svg>`;
}

/** The Klipp mark: a looping ring releasing a dot. */
export function markSvg(size: number, ring = "currentColor"): string {
  return `<svg viewBox="0 0 100 100" width="${size}" height="${size}" aria-hidden="true"><path d="M80.9 41.7 A32 32 0 1 1 58.3 19.1" fill="none" stroke="${ring}" stroke-width="12" stroke-linecap="round"/><circle cx="79.7" cy="20.3" r="9" fill="${A}"/></svg>`;
}
