// Renders the on-screen indicator pills (the design approved on the backlog
// board) to transparent PNGs in assets/overlay, at the sizes the recorder
// picks between: 1x for 1080p screens, 1.33x for 1440p, 2x for 4K.
// Uses Microsoft Edge (part of Windows) headless, and the app's own fonts.
// usage: node scripts/indicators/render.mjs
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const out = path.join(root, "assets", "overlay");
const fonts = path.join(root, "studio", "node_modules", "@fontsource");
const edge = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const port = 9334;

// Base size of 1em at 1x: the "Auto-clips ON" pill is 4em (48 px) tall on a 1080p screen.
const BASE = 12;
const SCALES = [
  ["1x", 1],
  ["1.33x", 4 / 3],
  ["2x", 2],
];

const mark = `<svg class="mark" viewBox="0 0 100 100"><path d="M80.9 41.7 A32 32 0 1 1 58.3 19.1" fill="none" stroke="#F4F2EC" stroke-width="12" stroke-linecap="round"/><circle cx="79.7" cy="20.3" r="9" fill="#FF5A2E"/></svg>`;
const check = `<svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const cross = `<svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"/></svg>`;

const pills = (scale, key) => `
  <div class="wrap" id="on@${key}" style="font-size:${BASE * scale}px"><div class="on">${mark}<span class="word">klipp</span><span class="chip"><i></i><span>ON</span></span></div></div>
  <div class="wrap" id="saved@${key}" style="font-size:${BASE * scale}px"><div class="mini saved">${mark}<span class="ind">${check}</span></div></div>
  <div class="wrap" id="failed@${key}" style="font-size:${BASE * scale}px"><div class="mini failed">${mark}<span class="ind">${cross}</span></div></div>`;

const html = `<!doctype html><meta charset="utf-8"><style>
@font-face { font-family: Unbounded; font-weight: 700; src: url("${pathToFileURL(path.join(fonts, "unbounded/files/unbounded-latin-700-normal.woff2"))}"); }
@font-face { font-family: "JetBrains Mono"; font-weight: 600; src: url("${pathToFileURL(path.join(fonts, "jetbrains-mono/files/jetbrains-mono-latin-600-normal.woff2"))}"); }
html, body { margin: 0; background: transparent; }
body { display: flex; flex-direction: column; align-items: flex-start; }
/* Room around each pill for its shadow; the recorder knows it's 1em. */
.wrap { display: inline-block; padding: 1em; }
.on { display: inline-flex; align-items: center; gap: .95em; height: 4em; padding: 0 .55em 0 1.2em; border-radius: 2em; background: #111215; border: .12em solid #2b2c30; box-shadow: 0 .25em .8em rgba(0,0,0,.35); box-sizing: border-box; }
.on .mark { width: 2.5em; height: 2.5em; }
.word { font: 700 1.8em/1 Unbounded; letter-spacing: -.03em; color: #f4f2ec; margin-top: -.1em; }
.chip { display: inline-flex; align-items: center; gap: .65em; height: 2.9em; padding: 0 1.05em; border-radius: 1.45em; background: #2b3418; }
.chip i { width: .72em; height: .72em; border-radius: 50%; background: #c8f03c; }
.chip span { font: 600 1.5em/1 "JetBrains Mono"; letter-spacing: .16em; margin-right: -.16em; color: #c8f03c; }
/* Saved / Failed: lighter (see-through) and narrower, no text (style A). */
.mini { display: inline-flex; align-items: center; gap: .6em; height: 3em; padding: 0 .4em 0 .75em; border-radius: 1.5em; background: rgba(17,18,21,.72); border: .08em solid rgba(244,242,236,.14); box-shadow: 0 .2em .7em rgba(0,0,0,.3); box-sizing: border-box; }
.mini .mark { width: 1.9em; height: 1.9em; }
.ind { display: grid; place-items: center; width: 2.2em; height: 2.2em; border-radius: 50%; }
.ind svg { width: 1.2em; height: 1.2em; }
.saved .ind { background: rgba(200,240,60,.18); color: #c8f03c; }
.failed .ind { background: rgba(255,79,69,.2); color: #ff4f45; }
</style><body>${SCALES.map(([key, s]) => pills(s, key)).join("")}</body>`;

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "klipp-indicators-"));
const page = path.join(tmp, "indicators.html");
fs.writeFileSync(page, html);

const browser = spawn(edge, ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${path.join(tmp, "profile")}`, "--hide-scrollbars", "--window-size=1200,2400", "about:blank"], { stdio: "ignore" });
try {
  let targets;
  for (let i = 0; i < 50 && !targets; i++) {
    await new Promise((r) => setTimeout(r, 200));
    targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json(), () => null);
  }
  const target = targets.find((t) => t.type === "page");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let id = 0;
  const pending = new Map();
  ws.addEventListener("message", (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) pending.get(msg.id)(msg.result);
  });
  const send = (method, params = {}) => new Promise((resolve) => { pending.set(++id, resolve); ws.send(JSON.stringify({ id, method, params })); });
  await send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
  await send("Page.navigate", { url: pathToFileURL(page).href });
  await new Promise((r) => setTimeout(r, 1500)); // fonts
  const rects = (await send("Runtime.evaluate", {
    expression: `JSON.stringify([...document.querySelectorAll('.wrap')].map((e) => { const r = e.getBoundingClientRect(); return { id: e.id, x: r.x, y: r.y, w: r.width, h: r.height }; }))`,
    returnByValue: true,
  })).result.value;
  fs.mkdirSync(out, { recursive: true });
  for (const r of JSON.parse(rects)) {
    // Whole pixels, so the image is exactly what was drawn.
    const clip = { x: Math.floor(r.x), y: Math.floor(r.y), width: Math.ceil(r.w), height: Math.ceil(r.h), scale: 1 };
    const shot = await send("Page.captureScreenshot", { format: "png", clip, captureBeyondViewport: true });
    fs.writeFileSync(path.join(out, `${r.id}.png`), Buffer.from(shot.data, "base64"));
    console.log(`${r.id}.png ${clip.width}x${clip.height}`);
  }
  ws.close();
} finally {
  browser.kill();
}
