// Video presets: named bundles of the video settings. Nothing stores which
// preset is chosen; the settings page shows the one the values match, or
// Custom. Codec, screen and graphics card stay as they are.

export type PresetId = "plus" | "performance" | "normal" | "high";

/** The video settings a preset (or the user) sets, as in settings.toml. */
export interface VideoChoice {
  codec: "av1" | "h264";
  resolution: "native" | "1440p" | "1080p" | "720p";
  fps: number;
  quality: number;
  max_bitrate_mbps: number;
  capture: "auto" | "light" | "wgc" | "duplication";
  performance: "off" | "on" | "plus";
  fast_encoder: boolean;
}

/** What presets depend on: the recorded screen's height and refresh rate. */
export interface Screen {
  height: number;
  refreshHz: number;
}

export const PRESETS: { id: PresetId; name: string; hint: string }[] = [
  { id: "plus", name: "Performance+", hint: "The lightest Klipp can record: 1080p, the light capture and the fastest encoder, and it steps down further while a game needs the graphics card." },
  { id: "performance", name: "Performance", hint: "Your screen at 60 fps. While a game keeps the graphics card busy, Klipp lowers its bitrate, then its frame rate, and goes back up when there's room." },
  { id: "normal", name: "Normal", hint: "Your screen at 60 fps, good quality, always as set." },
  { id: "high", name: "High", hint: "Your screen at up to 120 fps and high quality: the sharpest clips, the biggest files." },
];

/** The settings a preset stands for on this screen. */
export function presetValues(id: PresetId, screen: Screen): Omit<VideoChoice, "codec"> {
  // 4K screens record 1440p unless High: sharp, at a third of the memory.
  const big = screen.height > 1440 ? "1440p" : "native";
  switch (id) {
    case "plus":
      return { resolution: screen.height > 1080 ? "1080p" : "native", fps: 60, quality: 32, max_bitrate_mbps: 20, capture: "light", performance: "plus", fast_encoder: true };
    case "performance":
      return { resolution: big, fps: 60, quality: 30, max_bitrate_mbps: 30, capture: "auto", performance: "on", fast_encoder: false };
    case "normal":
      return { resolution: big, fps: 60, quality: 30, max_bitrate_mbps: 40, capture: "auto", performance: "off", fast_encoder: false };
    case "high":
      return { resolution: "native", fps: screen.refreshHz >= 120 ? 120 : 60, quality: 24, max_bitrate_mbps: 60, capture: "auto", performance: "off", fast_encoder: false };
  }
}

/** The preset these settings are, or "custom". */
export function matchPreset(v: Partial<VideoChoice>, screen: Screen): PresetId | "custom" {
  for (const { id } of PRESETS) {
    const p = presetValues(id, screen);
    const same = (Object.keys(p) as (keyof typeof p)[]).every((k) => (v[k] ?? defaults[k]) === p[k]);
    if (same) return id;
  }
  return "custom";
}

/** settings.toml's defaults, for keys an older file doesn't have. */
const defaults: Omit<VideoChoice, "codec"> = {
  resolution: "native",
  fps: 60,
  quality: 30,
  max_bitrate_mbps: 40,
  capture: "auto",
  performance: "off",
  fast_encoder: false,
};
