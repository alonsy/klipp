// The socials badge (Content Creator Mode): your handles with
// each platform's logo, drawn once on a canvas, so the preview shows exactly
// the picture the short gets. Logos are Simple Icons' (CC0): each brand's
// own mark, unchanged, pointing to your own accounts.

export type PlatformId = "tiktok" | "youtube" | "instagram" | "twitch" | "kick";

export interface Platform {
  id: PlatformId;
  name: string;
  /** The logo's tile: a colour, or a gradient's stops. */
  tile: string | string[];
  glyph: string;
  path: string;
}

export const PLATFORMS: Platform[] = [
  {
    id: "tiktok",
    name: "TikTok",
    tile: "#010101",
    glyph: "#ffffff",
    path: "M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z",
  },
  {
    id: "youtube",
    name: "YouTube",
    tile: "#ff0000",
    glyph: "#ffffff",
    path: "M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z",
  },
  {
    id: "instagram",
    name: "Instagram",
    tile: ["#feda75", "#fa7e1e", "#d62976", "#962fbf", "#4f5bd5"],
    glyph: "#ffffff",
    path: "M7.0301.084c-1.2768.0602-2.1487.264-2.911.5634-.7888.3075-1.4575.72-2.1228 1.3877-.6652.6677-1.075 1.3368-1.3802 2.127-.2954.7638-.4956 1.6365-.552 2.914-.0564 1.2775-.0689 1.6882-.0626 4.947.0062 3.2586.0206 3.6671.0825 4.9473.061 1.2765.264 2.1482.5635 2.9107.308.7889.72 1.4573 1.388 2.1228.6679.6655 1.3365 1.0743 2.1285 1.38.7632.295 1.6361.4961 2.9134.552 1.2773.056 1.6884.069 4.9462.0627 3.2578-.0062 3.668-.0207 4.9478-.0814 1.28-.0607 2.147-.2652 2.9098-.5633.7889-.3086 1.4578-.72 2.1228-1.3881.665-.6682 1.0745-1.3378 1.3795-2.1284.2957-.7632.4966-1.636.552-2.9124.056-1.2809.0692-1.6898.063-4.948-.0063-3.2583-.021-3.6668-.0817-4.9465-.0607-1.2797-.264-2.1487-.5633-2.9117-.3084-.7889-.72-1.4568-1.3876-2.1228C21.2982 1.33 20.628.9208 19.8378.6165 19.074.321 18.2017.1197 16.9244.0645 15.6471.0093 15.236-.005 11.977.0014 8.718.0076 8.31.0215 7.0301.0839m.1402 21.6932c-1.17-.0509-1.8053-.2453-2.2287-.408-.5606-.216-.96-.4771-1.3819-.895-.422-.4178-.6811-.8186-.9-1.378-.1644-.4234-.3624-1.058-.4171-2.228-.0595-1.2645-.072-1.6442-.079-4.848-.007-3.2037.0053-3.583.0607-4.848.05-1.169.2456-1.805.408-2.2282.216-.5613.4762-.96.895-1.3816.4188-.4217.8184-.6814 1.3783-.9003.423-.1651 1.0575-.3614 2.227-.4171 1.2655-.06 1.6447-.072 4.848-.079 3.2033-.007 3.5835.005 4.8495.0608 1.169.0508 1.8053.2445 2.228.408.5608.216.96.4754 1.3816.895.4217.4194.6816.8176.9005 1.3787.1653.4217.3617 1.056.4169 2.2263.0602 1.2655.0739 1.645.0796 4.848.0058 3.203-.0055 3.5834-.061 4.848-.051 1.17-.245 1.8055-.408 2.2294-.216.5604-.4763.96-.8954 1.3814-.419.4215-.8181.6811-1.3783.9-.4224.1649-1.0577.3617-2.2262.4174-1.2656.0595-1.6448.072-4.8493.079-3.2045.007-3.5825-.006-4.848-.0608M16.953 5.5864A1.44 1.44 0 1 0 18.39 4.144a1.44 1.44 0 0 0-1.437 1.4424M5.8385 12.012c.0067 3.4032 2.7706 6.1557 6.173 6.1493 3.4026-.0065 6.157-2.7701 6.1506-6.1733-.0065-3.4032-2.771-6.1565-6.174-6.1498-3.403.0067-6.156 2.771-6.1496 6.1738M8 12.0077a4 4 0 1 1 4.008 3.9921A3.9996 3.9996 0 0 1 8 12.0077",
  },
  {
    id: "twitch",
    name: "Twitch",
    tile: "#9146ff",
    glyph: "#ffffff",
    path: "M11.571 4.714h1.715v5.143H11.57zm4.715 0H18v5.143h-1.714zM6 0L1.714 4.286v15.428h5.143V24l4.286-4.286h3.428L22.286 12V0zm14.571 11.143l-3.428 3.428h-3.429l-3 3v-3H6.857V1.714h13.714Z",
  },
  {
    id: "kick",
    name: "Kick",
    tile: "#53fc18",
    glyph: "#0b0b0b",
    path: "M1.333 0h8v5.333H12V2.667h2.667V0h8v8H20v2.667h-2.667v2.666H20V16h2.667v8h-8v-2.667H12v-2.666H9.333V24h-8Z",
  },
];

export interface Social {
  handle: string;
  on: boolean;
}

export type Socials = Record<PlatformId, Social>;

export function emptySocials(): Socials {
  return Object.fromEntries(PLATFORMS.map((p) => [p.id, { handle: "", on: false }])) as Socials;
}

/** "@name", however it was typed. */
export function handleText(handle: string): string {
  const h = handle.trim().replace(/^https?:\/\/\S+\//, "");
  return h.startsWith("@") ? h : `@${h}`;
}

/** What the badge shows: the ticked platforms with a handle, one line per
 * handle (several logos when it's the same name on several platforms). */
export function lines(s: Socials): { platforms: Platform[]; text: string }[] {
  const out: { platforms: Platform[]; text: string }[] = [];
  for (const p of PLATFORMS) {
    const social = s[p.id];
    if (!social?.on || !social.handle.trim()) continue;
    const text = handleText(social.handle);
    const same = out.find((l) => l.text.toLowerCase() === text.toLowerCase());
    if (same) same.platforms.push(p);
    else out.push({ platforms: [p], text });
  }
  return out;
}

/** The CSS background for a logo tile. */
export function tileCss(p: Platform): string {
  return typeof p.tile === "string" ? p.tile : `linear-gradient(45deg, ${p.tile.join(", ")})`;
}

const FONT = '"Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif';

/** The badge at `scale` (1 = its size on the 1080-wide canvas), or null if
 * nothing is ticked. Logos in round tiles, then the handle, on a dark pill. */
export function drawBadge(s: Socials, scale: number): HTMLCanvasElement | null {
  const rows = lines(s);
  if (rows.length === 0) return null;
  const k = Math.max(0.4, Math.min(scale, 2.5));
  const icon = Math.round(54 * k);
  const gap = Math.round(10 * k);
  const textGap = Math.round(16 * k);
  const padX = Math.round(22 * k);
  const padY = Math.round(14 * k);
  const rowGap = Math.round(10 * k);
  const font = `700 ${Math.round(38 * k)}px ${FONT}`;

  const measure = document.createElement("canvas").getContext("2d")!;
  measure.font = font;
  const widths = rows.map((r) => r.platforms.length * icon + (r.platforms.length - 1) * gap + textGap + Math.ceil(measure.measureText(r.text).width));
  // Even sizes keep the picture's colour samples lined up in the short.
  const width = (Math.max(...widths) + 2 * padX + 1) & ~1;
  const height = (rows.length * icon + (rows.length - 1) * rowGap + 2 * padY + 1) & ~1;

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d")!;
  const r = Math.min(height / 2, 44 * k);
  ctx.fillStyle = "rgba(14, 14, 13, 0.74)";
  ctx.beginPath();
  ctx.roundRect(0, 0, width, height, r);
  ctx.fill();

  rows.forEach((row, i) => {
    const y = padY + i * (icon + rowGap);
    let x = padX;
    for (const p of row.platforms) {
      drawLogo(ctx, p, x, y, icon);
      x += icon + gap;
    }
    x += textGap - gap;
    ctx.font = font;
    ctx.textBaseline = "middle";
    ctx.fillStyle = "#ffffff";
    ctx.shadowColor = "rgba(0, 0, 0, 0.45)";
    ctx.shadowBlur = 6 * k;
    ctx.fillText(row.text, x, y + icon / 2 + 1 * k);
    ctx.shadowBlur = 0;
  });
  return canvas;
}

/** One logo in its round tile at (x, y), `size` across. */
export function drawLogo(ctx: CanvasRenderingContext2D, p: Platform, x: number, y: number, size: number) {
  ctx.save();
  ctx.beginPath();
  ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2);
  if (typeof p.tile === "string") {
    ctx.fillStyle = p.tile;
  } else {
    const g = ctx.createLinearGradient(x, y + size, x + size, y);
    p.tile.forEach((c, i) => g.addColorStop(i / (p.tile.length - 1), c));
    ctx.fillStyle = g;
  }
  ctx.fill();
  const glyph = size * 0.56;
  ctx.translate(x + (size - glyph) / 2, y + (size - glyph) / 2);
  ctx.scale(glyph / 24, glyph / 24);
  ctx.fillStyle = p.glyph;
  ctx.fill(new Path2D(p.path));
  ctx.restore();
}
