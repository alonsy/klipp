import type { Clip } from "./api";

const EVENT_NAMES: Record<string, string> = {
  kill: "Kill",
  headshot: "Headshot",
  ace: "ACE",
  knife: "Knife kill",
  zeus: "Zeus kill",
  death: "Death",
  mvp: "MVP",
  victory: "Round won",
  defeat: "Round lost",
  defuse: "Bomb defused",
  manual: "Manual clip",
  capture: "Saved here",
  // League of Legends.
  penta: "Pentakill",
  teamace: "Team ace",
  firstblood: "First blood",
  steal: "Steal",
  objective: "Objective",
  assist: "Assist",
  win: "Victory",
  loss: "Defeat",
  // Rocket League.
  goal: "Goal",
  save: "Save",
  epicsave: "Epic save",
  demolition: "Demolition",
  // Fortnite.
  knock: "Knock",
};

/** "2k" → "2K", "headshot" → "Headshot". A win is a round in CS2, the match in Fortnite. */
export function eventName(kind: string, game?: string): string {
  if (kind === "victory" && game === "Fortnite") return "Victory Royale";
  return EVENT_NAMES[kind] ?? (/^\d+k$/.test(kind) ? kind.toUpperCase() : kind);
}

/** Order for filters, badges and shared markers, most notable first. */
export function eventRank(kind: string): number {
  if (kind === "ace" || kind === "penta") return 105;
  const k = /^(\d+)k$/.exec(kind);
  if (k) return 100 + Number(k[1]);
  return (
    {
      goal: 90,
      epicsave: 80,
      teamace: 70,
      steal: 60,
      demolition: 55,
      knife: 50,
      zeus: 49,
      firstblood: 45,
      headshot: 40,
      save: 35,
      mvp: 30,
      kill: 20,
      knock: 15,
      assist: 15,
      death: 10,
      objective: 8,
      defuse: 5,
      victory: 4,
      win: 4,
      defeat: 3,
      loss: 3,
      capture: 1,
    }[kind] ?? 0
  );
}

export const isMultiKill = (kind: string | null | undefined) => !!kind && (/^\d+k$/.test(kind) || kind === "ace" || kind === "penta");

/** What a clip is called in the library: its reason, or "Manual clip". */
export function clipTitle(c: Clip): string {
  return c.label ? eventName(c.label, c.game) : c.kind === "auto" ? "Auto-clip" : "Manual clip";
}

const MAPS: Record<string, string> = {
  de_dust2: "Dust II",
  de_mirage: "Mirage",
  de_inferno: "Inferno",
  de_nuke: "Nuke",
  de_overpass: "Overpass",
  de_vertigo: "Vertigo",
  de_ancient: "Ancient",
  de_anubis: "Anubis",
  de_train: "Train",
  cs_office: "Office",
  cs_italy: "Italy",
  // League of Legends reports map numbers (Riot's maps.json).
  Map11: "Summoner's Rift",
  Map12: "Howling Abyss",
  Map21: "Nexus Blitz",
  Map30: "Rings of Wrath",
  Map35: "The Bandlewood",
};

/** "de_mirage" → "Mirage"; unknown maps lose their prefix and get capitals. */
export function mapName(map: string): string {
  if (!map) return "";
  if (MAPS[map]) return MAPS[map];
  const bare = map.replace(/^(de|cs|ar|dz|gd|aim|awp|fy)_/, "").replace(/_/g, " ");
  return bare.replace(/\b\w/g, (c) => c.toUpperCase());
}

const MODES: Record<string, string> = {
  competitive: "Competitive",
  premier: "Premier",
  casual: "Casual",
  deathmatch: "Deathmatch",
  scrimcomp2v2: "Wingman",
  gungameprogressive: "Arms Race",
  gungametrbomb: "Demolition",
  survival: "Danger Zone",
  custom: "Custom",
  // League of Legends (Riot's gameModes.json).
  CLASSIC: "Classic",
  ARAM: "ARAM",
  URF: "URF",
  ARURF: "ARURF",
  ONEFORALL: "One for All",
  CHERRY: "Arena",
  SWIFTPLAY: "Swiftplay",
  NEXUSBLITZ: "Nexus Blitz",
  ULTBOOK: "Ultimate Spellbook",
  PRACTICETOOL: "Practice Tool",
  TUTORIAL: "Tutorial",
};

export function modeName(mode: string): string {
  return MODES[mode] ?? (mode ? mode[0].toUpperCase() + mode.slice(1) : "");
}

const WEAPONS: Record<string, string> = {
  ak47: "AK-47",
  m4a1: "M4A4",
  m4a1_silencer: "M4A1-S",
  awp: "AWP",
  deagle: "Desert Eagle",
  usp_silencer: "USP-S",
  hkp2000: "P2000",
  glock: "Glock-18",
  p250: "P250",
  fiveseven: "Five-SeveN",
  tec9: "Tec-9",
  cz75a: "CZ75-Auto",
  elite: "Dual Berettas",
  revolver: "R8 Revolver",
  famas: "FAMAS",
  galilar: "Galil AR",
  sg556: "SG 553",
  aug: "AUG",
  ssg08: "SSG 08",
  scar20: "SCAR-20",
  g3sg1: "G3SG1",
  mac10: "MAC-10",
  mp9: "MP9",
  mp7: "MP7",
  mp5sd: "MP5-SD",
  ump45: "UMP-45",
  p90: "P90",
  bizon: "PP-Bizon",
  nova: "Nova",
  xm1014: "XM1014",
  mag7: "MAG-7",
  sawedoff: "Sawed-Off",
  negev: "Negev",
  m249: "M249",
  taser: "Zeus x27",
  hegrenade: "HE Grenade",
  molotov: "Molotov",
  incgrenade: "Incendiary",
  inferno: "Fire",
};

/** What other games' markers say: League's objectives, Rocket League's goals. */
const DETAILS: Record<string, string> = {
  baron: "Baron Nashor",
  herald: "Rift Herald",
  dragon: "Dragon",
  dragon_elder: "Elder Dragon",
  dragon_fire: "Infernal Drake",
  dragon_earth: "Mountain Drake",
  dragon_water: "Ocean Drake",
  dragon_air: "Cloud Drake",
  dragon_hextech: "Hextech Drake",
  dragon_chemtech: "Chemtech Drake",
  turret: "Turret",
  inhibitor: "Inhibitor",
  overtime: "Overtime",
  bicycle: "Bicycle",
  aerial: "Aerial",
  backwards: "Backwards",
  long: "Long range",
  turtle: "Turtle",
  pool: "Pool shot",
  swish: "Swish",
  extermination: "Extermination",
};

/** A marker's detail: a CS2 weapon ("AK-47"), a League champion or objective, a Rocket League goal. */
export function weaponName(w: string): string {
  if (!w) return "";
  if (DETAILS[w]) return DETAILS[w];
  // Champion names come written for people already ("Miss Fortune").
  if (/[A-Z ]/.test(w)) return w;
  if (w.startsWith("knife") || w === "bayonet" || w.startsWith("knife_")) return "Knife";
  return WEAPONS[w] ?? w.toUpperCase();
}

/** How many times the hotkey was pressed for this clip (presses close together make one clip). */
export function captures(c: Clip): number {
  return c.events.filter((e) => e.kind === "capture").length;
}

/** "2 kills · 1 HS · AK-47" from a clip's events. */
export function killSummary(c: Clip): string {
  const kills = c.events.filter((e) => e.kind === "kill" || e.kind === "headshot");
  if (kills.length === 0) return "";
  const hs = kills.filter((e) => e.kind === "headshot").length;
  const weapons = [...new Set(kills.map((e) => weaponName(e.detail)).filter(Boolean))];
  const parts = [kills.length === 1 ? "1 kill" : `${kills.length} kills`];
  if (hs > 0) parts.push(`${hs} HS`);
  if (weapons.length > 0) parts.push(weapons.slice(0, 2).join(", "));
  return parts.join(" · ");
}

export function duration(secs: number | null): string {
  if (secs == null) return "";
  const s = Math.round(secs);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "0:07.42" */
export function timecode(t: number): string {
  const s = Math.max(0, t);
  return `${Math.floor(s / 60)}:${(s % 60).toFixed(2).padStart(5, "0")}`;
}

export function size(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(bytes >= 1e8 ? 0 : 1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}

export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** "Today", "Yesterday", "Mon 13 Oct", "13 Oct 2025". */
export function day(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  const sameYear = d.getFullYear() === today.getFullYear();
  const recent = today.getTime() - ms < 6 * 24 * 3600 * 1000;
  return d.toLocaleDateString(undefined, {
    weekday: recent ? "short" : undefined,
    day: "numeric",
    month: "short",
    year: sameYear ? undefined : "numeric",
  });
}

export function when(ms: number): string {
  return `${day(ms)} ${clock(ms)}`;
}
