//! Fortnite shares nothing while a match is on, so this reads the
//! eliminations counter off the screen: the white number left of the skull
//! icon, top right. `hudtest`'s writer thread hands it small areas of the
//! frame twice a second; every rise of the number is an elimination for the
//! auto-clip engine, "KNOCKED!" below the crosshair a knock, and the "#1
//! VICTORY ROYALE" banner a win. Nothing touches the game.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use serde::Deserialize;

use crate::autoclip::Rules;
use crate::clock::SECOND;
use crate::guard::{Budget, Limit};
use crate::hudtest::Region;
use crate::settings::MultiKillMode;

pub const GAME_NAME: &str = "Fortnite";

/// `%APPDATA%\Klipp\fortnite.json`, which the window writes: on unless it says
/// otherwise. A file of its own: settings.toml is shared with builds that
/// don't have this.
#[derive(Deserialize)]
#[serde(default)]
struct Choice {
    enabled: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Self { enabled: true }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(true);

fn choice_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Klipp").join("fortnite.json"))
}

/// Whether Fortnite's auto-clips are on.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Follows fortnite.json. Once a second, with what it saw last time.
pub fn follow_choice(stamp: &mut Option<SystemTime>) {
    let Some(path) = choice_path() else { return };
    let now = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if now == *stamp {
        return;
    }
    *stamp = now;
    let choice: Choice = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    ENABLED.store(choice.enabled, Ordering::Relaxed);
}

/// The digits, with the default HUD at 16:9 (pixels at 1440p: x 2041-2109, y 50-82).
pub fn digits_area() -> Region {
    area("elims", 2041.0, 50.0, 68.0, 32.0)
}

/// The icon right of them (x 2112-2162, y 43-88): a skull when the number is
/// your eliminations. While you're out, players left (a person) takes its place.
pub fn icon_area() -> Region {
    area("elims-icon", 2112.0, 43.0, 50.0, 45.0)
}

/// Where "#1 VICTORY ROYALE" can be, top centre (x 732-1632, y 115-515): the
/// crowned win's orange banner sits lower than the usual blue one.
pub fn victory_area() -> Region {
    area("victory", 732.0, 115.0, 900.0, 400.0)
}

/// "KNOCKED!" below the crosshair when you knock someone (x 1184-1382, y 1050-1102),
/// with room for the word to move a little.
pub fn knock_area() -> Region {
    area("knock", 1184.0, 1050.0, 198.0, 52.0)
}

fn area(name: &str, x: f32, y: f32, w: f32, h: f32) -> Region {
    Region { name: name.into(), x: x / 2560.0, y: y / 1440.0, w: w / 2560.0, h: h / 1440.0, scale: 1.0 }
}

/// A clip never grows past this much of the game: a stream of moments, real or
/// misread, becomes several clips the limits below can stop, not one that never ends.
const MAX_CHAIN: i64 = 60 * SECOND;

/// Every elimination is a clip, and close ones become one 2K, 3K… clip.
/// Timing follows the auto-clip settings, with at least 10 s before the
/// first elimination: a fight starts well before it. A Victory Royale is a
/// clip too, from 20 s before the banner (the last elimination happens just
/// before it, and the counter never shows it) to 6 s after, unless the
/// settings time "victory" themselves. A knock joins the eliminations near
/// it, and is a clip of its own when none is.
pub fn rules(base: &Rules) -> Rules {
    let mut timing = base.timing.clone();
    timing.entry("victory".into()).or_insert((Some(20 * SECOND), Some(6 * SECOND)));
    Rules {
        headshot: false,
        multi_kill_min: 2,
        multi_kill_mode: MultiKillMode::Streak,
        any_kill: true,
        knife_or_zeus_kill: false,
        death: false,
        mvp: false,
        victory: true,
        knock: true,
        pre_roll: base.pre_roll.max(10 * SECOND),
        max_chain: MAX_CHAIN,
        timing,
        ..base.clone()
    }
}

// Reading the screen.

/// The HUD's white: bright and nearly grey. Lower let bright sky through and
/// turned 0s into 6s.
const WHITE_MIN: u8 = 225;
const WHITE_SHARE: f32 = 0.8824;
const WHITE_SHARE_SMALL: f32 = 0.8;
const WHITE_LOWEST: f32 = 200.0;
const WHITE_SPREAD: u8 = 40;
/// Glyphs are compared at 16 px high, in a 20 px wide box.
const DIGIT_H: usize = 16;
const DIGIT_W: usize = 20;
const ICON: usize = 16;
/// Mean difference to the nearest template above which a glyph isn't a digit.
const MAX_DIFF: f32 = 0.12;
const MAX_ICON_DIFF: f32 = 0.15;

#[derive(Deserialize)]
struct DigitShape {
    digit: String,
    rows: Vec<Vec<f32>>,
}

#[derive(Deserialize)]
struct IconShape {
    skull: bool,
    rows: Vec<Vec<f32>>,
}

// Words read by their shape, in greyscale, whatever their colour: the areas
// are resized to a fixed size (so any screen size works) and the template is
// searched across them by normalized correlation.

/// The knock area at half size (1440p), and the victory area at an eighth.
const KNOCK_SIZE: (usize, usize) = (99, 26);
const VICTORY_SIZE: (usize, usize) = (112, 50);
/// Clear knocks scored 0.87-1.00 against the template; nothing else in four
/// evenings scored more than 0.71 (edges of buildings, a bar).
const MIN_KNOCK: f32 = 0.82;
/// The word's outline (edge strength) holds on a bright sky that washes out
/// its fill. Outlines are rarer than shapes: nothing that wasn't a knock
/// scored more than 0.38 (8,197 pictures).
const MIN_KNOCK_OUTLINE: f32 = 0.55;
/// Both banner styles scored 0.84-0.98 once settled; nothing else above 0.55.
const MIN_VICTORY: f32 = 0.72;

#[derive(Deserialize, Default)]
struct ScreenShapes {
    knock: Vec<Vec<f32>>,
    banners: std::collections::BTreeMap<String, Vec<Vec<f32>>>,
}

/// A template: width, height, values with the mean taken out, and their norm.
struct Shape {
    w: usize,
    h: usize,
    centered: Vec<f32>,
    norm: f32,
}

impl Shape {
    fn new(rows: &[Vec<f32>]) -> Option<Self> {
        let (h, w) = (rows.len(), rows.first()?.len());
        Self::from_flat(rows.iter().flatten().copied().collect(), w, h)
    }

    fn from_flat(flat: Vec<f32>, w: usize, h: usize) -> Option<Self> {
        if w == 0 || flat.len() != w * h {
            return None;
        }
        let mean = flat.iter().sum::<f32>() / flat.len() as f32;
        let centered: Vec<f32> = flat.iter().map(|v| v - mean).collect();
        let norm = centered.iter().map(|v| v * v).sum::<f32>().sqrt();
        Some(Self { w, h, centered, norm })
    }
}

struct Templates {
    digits: Vec<(u32, Vec<f32>)>,
    icons: Vec<(bool, Vec<f32>)>,
    knock: Option<Shape>,
    knock_outline: Option<Shape>,
    /// The blue banner and the crowned win's orange one.
    victory: Vec<Shape>,
}

/// Digits, icons, the knock and both banners, learned from real matches at 1440p.
fn templates() -> &'static Templates {
    static T: OnceLock<Templates> = OnceLock::new();
    T.get_or_init(|| {
        let flat = |rows: Vec<Vec<f32>>| rows.into_iter().flatten().collect::<Vec<f32>>();
        let digits: Vec<DigitShape> = serde_json::from_str(include_str!("fortnite/digits.json")).unwrap_or_default();
        let icons: Vec<IconShape> = serde_json::from_str(include_str!("fortnite/icons.json")).unwrap_or_default();
        let screen: ScreenShapes = serde_json::from_str(include_str!("fortnite/screen.json")).unwrap_or_default();
        Templates {
            digits: digits
                .into_iter()
                .filter_map(|t| Some((t.digit.parse().ok()?, flat(t.rows))))
                .filter(|(_, v)| v.len() == DIGIT_W * DIGIT_H)
                .collect(),
            icons: icons.into_iter().map(|t| (t.skull, flat(t.rows))).filter(|(_, v)| v.len() == ICON * ICON).collect(),
            knock: Shape::new(&screen.knock),
            knock_outline: screen.knock.first().and_then(|row| {
                let (w, h) = (row.len(), screen.knock.len());
                Shape::from_flat(edges(&screen.knock.iter().flatten().copied().collect::<Vec<f32>>(), w, h), w, h)
            }),
            victory: screen.banners.values().filter_map(|rows| Shape::new(rows)).collect(),
        }
    })
}

/// "KNOCKED!" is on screen, from the knock area (24-bit BGR, any size): its
/// shape or its outline.
pub fn is_knock(bgr: &[u8], width: usize, height: usize) -> bool {
    let t = templates();
    let (Some(shape), Some(outline)) = (&t.knock, &t.knock_outline) else { return false };
    let Some(area) = grey_area(bgr, width, height, KNOCK_SIZE) else { return false };
    let (aw, ah) = KNOCK_SIZE;
    best_match(&area, KNOCK_SIZE, shape) >= MIN_KNOCK || best_match(&edges(&area, aw, ah), KNOCK_SIZE, outline) >= MIN_KNOCK_OUTLINE
}

/// The Victory Royale banner is on screen, in either style, from the victory area.
pub fn is_victory(bgr: &[u8], width: usize, height: usize) -> bool {
    let Some(area) = grey_area(bgr, width, height, VICTORY_SIZE) else { return false };
    templates().victory.iter().any(|shape| best_match(&area, VICTORY_SIZE, shape) >= MIN_VICTORY)
}

/// The area in greyscale, resized to `size`.
fn grey_area(bgr: &[u8], width: usize, height: usize, (aw, ah): (usize, usize)) -> Option<Vec<f32>> {
    if width == 0 || height == 0 || bgr.len() < width * height * 3 {
        return None;
    }
    let grey: Vec<f32> = bgr.chunks_exact(3).take(width * height).map(|p| 0.299 * p[2] as f32 + 0.587 * p[1] as f32 + 0.114 * p[0] as f32).collect();
    Some(resample(&grey, width, height, aw, ah))
}

/// Edge strength: how much each pixel differs from the next one right and below.
fn edges(v: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: usize, y: usize| v[y * w + x];
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let dx = if x + 1 < w { (at(x + 1, y) - at(x, y)).abs() } else { 0.0 };
            let dy = if y + 1 < h { (at(x, y + 1) - at(x, y)).abs() } else { 0.0 };
            out[y * w + x] = dx + dy;
        }
    }
    out
}

/// The template's best correlation anywhere in the area (-1 if it doesn't fit or the area is flat).
fn best_match(a: &[f32], (aw, ah): (usize, usize), t: &Shape) -> f32 {
    if t.w > aw || t.h > ah || a.len() < aw * ah {
        return -1.0;
    }
    let n = (t.w * t.h) as f32;
    let mut best = -1.0f32;
    for y in 0..=ah - t.h {
        for x in 0..=aw - t.w {
            let (mut sum, mut sq, mut dot) = (0.0f32, 0.0f32, 0.0f32);
            for ty in 0..t.h {
                let row = &a[(y + ty) * aw + x..(y + ty) * aw + x + t.w];
                for (v, c) in row.iter().zip(&t.centered[ty * t.w..(ty + 1) * t.w]) {
                    sum += v;
                    sq += v * v;
                    dot += v * c;
                }
            }
            let var = sq - sum * sum / n;
            // A flat patch (standard deviation under 4) can't be the word.
            if var < 16.0 * n {
                continue;
            }
            best = best.max(dot / (var.sqrt() * t.norm + 1e-6));
        }
    }
    best
}

/// Your eliminations, from the two areas (24-bit BGR), or `None` when the
/// counter isn't on screen or isn't clear.
pub fn read(digits: &[u8], dw: usize, dh: usize, icon: &[u8], iw: usize, ih: usize) -> Option<u32> {
    if !is_skull(icon, iw, ih) {
        return None;
    }
    read_digits(digits, dw, dh)
}

fn mask(bgr: &[u8], width: usize, height: usize) -> Option<Vec<bool>> {
    if width == 0 || height == 0 || bgr.len() < width * height * 3 {
        return None;
    }
    let pixels = || bgr.chunks_exact(3).take(width * height);
    let peak = pixels().map(|p| p[0].min(p[1]).min(p[2])).max()?;
    // The whitest pixel sets the bar: a dimmer screen is read too, and smaller
    // pictures, whose glyphs blur at the edges, get more room.
    let share = if height >= 32 { WHITE_SHARE } else { WHITE_SHARE_SMALL };
    let floor = (peak as f32 * share).clamp(WHITE_LOWEST, WHITE_MIN as f32) as u8;
    Some(
        pixels()
            .map(|p| {
                let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                lo >= floor && hi - lo <= WHITE_SPREAD
            })
            .collect(),
    )
}

fn read_digits(bgr: &[u8], width: usize, height: usize) -> Option<u32> {
    let m = mask(bgr, width, height)?;
    let at = |x: usize, y: usize| m[y * width + x];
    // Sizes scale with the area's height (32 px at 1440p).
    let bh = height as f64;
    let min_column = ((bh / 16.0 + 0.5).floor() as usize).max(1);
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..=width {
        let on = x < width && (0..height).filter(|&y| at(x, y)).count() >= min_column;
        match (on, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    let mut glyphs = Vec::new();
    for (a, b) in runs {
        let rows: Vec<usize> = (0..height).filter(|&y| (a..b).any(|x| at(x, y))).collect();
        let (top, bottom) = (*rows.first()?, *rows.last()?);
        let (gh, gw) = ((bottom - top + 1) as f64, (b - a) as f64);
        if !(0.47 * bh <= gh && gh <= 0.82 * bh && 0.09 * bh <= gw && gw <= 0.69 * bh) {
            return None; // not a digit: something bright behind the HUD, or a menu
        }
        // Keep the glyph's width relative to its height ("1" stays narrow).
        let w = ((gw / gh * DIGIT_H as f64 + 0.5).floor() as usize).clamp(1, DIGIT_W);
        let cut = ones((top..=bottom).flat_map(|y| m[y * width + a..y * width + b].iter().copied()));
        let small = resample(&cut, b - a, bottom - top + 1, w, DIGIT_H);
        let mut glyph = vec![0f32; DIGIT_W * DIGIT_H];
        for y in 0..DIGIT_H {
            glyph[y * DIGIT_W..y * DIGIT_W + w].copy_from_slice(&small[y * w..(y + 1) * w]);
        }
        glyphs.push(glyph);
    }
    if glyphs.is_empty() || glyphs.len() > 2 {
        return None;
    }
    let mut value = 0;
    for g in &glyphs {
        let (d, digit) = templates().digits.iter().map(|(digit, t)| (diff(t, g), *digit)).min_by(|a, b| a.0.total_cmp(&b.0))?;
        if d > MAX_DIFF {
            return None;
        }
        value = value * 10 + digit;
    }
    Some(value)
}

fn is_skull(bgr: &[u8], width: usize, height: usize) -> bool {
    let Some(m) = mask(bgr, width, height) else { return false };
    let rows: Vec<usize> = (0..height).filter(|&y| (0..width).any(|x| m[y * width + x])).collect();
    let cols: Vec<usize> = (0..width).filter(|&x| (0..height).any(|y| m[y * width + x])).collect();
    let (Some(&top), Some(&bottom), Some(&left), Some(&right)) = (rows.first(), rows.last(), cols.first(), cols.last()) else {
        return false;
    };
    let (h, w) = (bottom - top + 1, right - left + 1);
    if (h as f64) < 0.4 * height as f64 || (w as f64) < 0.4 * height as f64 {
        return false;
    }
    let cut = ones((top..=bottom).flat_map(|y| m[y * width + left..=y * width + right].iter().copied()));
    let shape = resample(&cut, w, h, ICON, ICON);
    let best = |skull: bool| templates().icons.iter().filter(|(s, _)| *s == skull).map(|(_, t)| diff(t, &shape)).fold(f32::INFINITY, f32::min);
    let skull = best(true);
    skull < MAX_ICON_DIFF && skull < best(false)
}

fn diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32
}

/// How much of each source pixel falls in each output pixel (area averaging): dst rows of src weights.
fn area_weights(src: usize, dst: usize) -> Vec<Vec<f64>> {
    let step = src as f64 / dst as f64;
    (0..dst)
        .map(|o| {
            let (a, b) = (o as f64 * step, (o + 1) as f64 * step);
            let mut row = vec![0.0; src];
            for (i, weight) in row.iter_mut().enumerate().take((b.ceil() as usize).min(src)).skip(a.floor() as usize) {
                *weight = (b.min(i as f64 + 1.0) - a.max(i as f64)).max(0.0) / step;
            }
            row
        })
        .collect()
}

/// Area-average resize of w x h values (row by row) to dw x dh: rows first, then columns.
fn resample(v: &[f32], w: usize, h: usize, dw: usize, dh: usize) -> Vec<f32> {
    let (wy, wx) = (area_weights(h, dh), area_weights(w, dw));
    let mut rows = vec![0f64; dh * w];
    for (oy, weights) in wy.iter().enumerate() {
        for (y, &f) in weights.iter().enumerate().filter(|(_, f)| **f != 0.0) {
            for x in 0..w {
                rows[oy * w + x] += f * v[y * w + x] as f64;
            }
        }
    }
    let mut out = vec![0f32; dw * dh];
    for oy in 0..dh {
        for (ox, weights) in wx.iter().enumerate() {
            out[oy * dw + ox] = weights.iter().enumerate().filter(|(_, f)| **f != 0.0).map(|(x, f)| f * rows[oy * w + x]).sum::<f64>() as f32;
        }
    }
    out
}

fn ones(mask: impl Iterator<Item = bool>) -> Vec<f32> {
    mask.map(|on| if on { 1.0 } else { 0.0 }).collect()
}

// From readings to moments, with limits: other screens and HUD settings show
// this reader things it never learned, so everything below assumes it can be
// wrong and keeps what a wrong reading can do small.

/// A rise bigger than this is a misreading or a stretch Klipp didn't see, not eliminations.
const MAX_STEP: u32 = 4;
/// A reading counts once another one agrees within this time.
const CONFIRM_WITHIN: i64 = 6 * SECOND;
/// A number the counter can't plausibly go to must show this long before it's believed.
const HOLD: i64 = 10 * SECOND;
/// Back at 0 this soon after the last change isn't a new match. The quickest
/// in real play was nearly two minutes.
const MIN_MATCH_GAP: i64 = 45 * SECOND;

#[derive(Debug, PartialEq)]
pub enum Change {
    /// The counter shows 0: a match has started, and rises count from here on.
    Count(u32),
    /// `by` eliminations at `time` (when the new number first showed); the counter now shows `to`.
    Eliminations { by: u32, to: u32, time: i64 },
    /// The counter did something it can't: this reader may not be reading it.
    /// It's going on from this number.
    Odd(u32),
}

struct Pending {
    value: u32,
    time: i64,
}

/// Turns readings into eliminations. A number counts once the next clear
/// reading agrees, so one odd picture never makes a clip. The counter starts
/// at 0 and only rises, so that's all that's believed: nothing counts until
/// it has shown 0, and it goes down only to 0, once that has held.
#[derive(Default)]
pub struct Counter {
    value: Option<u32>,
    pending: Option<Pending>,
    /// A number that isn't plausible yet, and when it first showed.
    held: Option<Pending>,
    changed: i64,
}

impl Counter {
    /// One reading (`None`: no counter on screen), with its frame's time.
    pub fn feed(&mut self, reading: Option<u32>, time: i64) -> Option<Change> {
        let v = reading?;
        if let Some(p) = self.pending.take()
            && p.value == v
            && time - p.time <= CONFIRM_WITHIN
        {
            return self.confirm(v, p.time);
        }
        if self.value != Some(v) {
            self.pending = Some(Pending { value: v, time });
            return None;
        }
        // Back to the number it had: what was held was a misreading.
        self.held.take().map(|_| Change::Odd(v))
    }

    fn confirm(&mut self, v: u32, time: i64) -> Option<Change> {
        let Some(old) = self.value else {
            if v != 0 {
                return None;
            }
            self.value = Some(0);
            self.changed = time;
            return Some(Change::Count(0));
        };
        if v == old {
            return None;
        }
        if v > old && v - old <= MAX_STEP {
            self.held = None;
            self.value = Some(v);
            self.changed = time;
            return Some(Change::Eliminations { by: v - old, to: v, time });
        }
        // Lower but not 0 never goes through: going back up from it would look like eliminations.
        if v != 0 && v < old {
            return None;
        }
        let first = match &self.held {
            Some(h) if h.value == v => h.time,
            _ => {
                self.held = Some(Pending { value: v, time });
                return None;
            }
        };
        if time - first < HOLD {
            return None;
        }
        self.held = None;
        self.value = Some(v);
        let soon = time - self.changed < MIN_MATCH_GAP + HOLD;
        self.changed = time;
        Some(if v == 0 && !soon { Change::Count(0) } else { Change::Odd(v) })
    }
}

/// The banner must show in two samples this close together (0.5 s apart, some may be dropped).
const BANNER_CONFIRM: i64 = 3 * SECOND;
/// After a win, another one counts only after a new match or this long
/// without the banner. Real wins were 18 minutes or more apart.
const BANNER_REARM: i64 = 300 * SECOND;

/// The Victory Royale banner, once per match.
#[derive(Default)]
pub struct Banner {
    first: Option<i64>,
    shown: bool,
    last_seen: i64,
}

impl Banner {
    /// One sample; the time of the win (when the banner first showed) once it's sure.
    pub fn feed(&mut self, seen: bool, time: i64) -> Option<i64> {
        if !seen {
            self.first = None;
            if self.shown && time - self.last_seen > BANNER_REARM {
                self.shown = false;
            }
            return None;
        }
        self.last_seen = time;
        if self.shown {
            return None;
        }
        match self.first {
            Some(first) if time - first <= BANNER_CONFIRM => {
                self.shown = true;
                self.first = None;
                Some(first)
            }
            _ => {
                self.first = Some(time);
                None
            }
        }
    }

    /// The counter came back at 0 (a new match): the next banner counts again.
    pub fn new_match(&mut self) {
        self.shown = false;
        self.first = None;
    }
}

/// "KNOCKED!" stays up about 3 s; it's a new knock when it shows after this long without it.
const KNOCK_GAP: i64 = 3 * SECOND / 2;
/// Knocks count only while your eliminations counter was read this recently:
/// spectating a teammate shows their knocks, with players left in its place.
const KNOCK_COUNTER_WITHIN: i64 = 5 * SECOND;
/// Showing this long without a break, it's something on screen that looks like the word...
const KNOCK_STUCK: i64 = 10 * SECOND;
/// ...and nothing there counts again until it has been gone this long.
const KNOCK_RELEASE: i64 = 30 * SECOND;

#[derive(Debug, PartialEq)]
pub enum Knock {
    New,
    /// Up too long to be the word.
    Stuck,
}

/// Your knocks, once each.
#[derive(Default)]
pub struct Knocks {
    last_seen: Option<i64>,
    counter_seen: Option<i64>,
    since: i64,
    stuck: bool,
}

impl Knocks {
    /// Your eliminations counter was read clearly at `time`.
    pub fn counter_seen(&mut self, time: i64) {
        self.counter_seen = Some(time);
    }

    /// One sample.
    pub fn feed(&mut self, seen: bool, time: i64) -> Option<Knock> {
        if !seen {
            return None;
        }
        let gone = self.last_seen.map_or(i64::MAX, |t| time - t);
        self.last_seen = Some(time);
        if self.stuck {
            self.stuck = gone <= KNOCK_RELEASE;
            if self.stuck {
                return None;
            }
        }
        let new = gone > KNOCK_GAP;
        if new {
            self.since = time;
        } else if time - self.since > KNOCK_STUCK {
            self.stuck = true;
            return Some(Knock::Stuck);
        }
        (new && self.counter_seen.is_some_and(|t| time - t <= KNOCK_COUNTER_WITHIN)).then_some(Knock::New)
    }
}

/// How many moments can become clips. The busiest stretches of a real evening
/// (44 moments in about two hours): 5 in a minute, 7 in five minutes, 20 in half
/// an hour. The last limit stops a steady false stream too slow for the first two.
const MOMENTS: &[Limit] = &[(10, 60 * SECOND), (30, 300 * SECOND), (60, 1800 * SECOND)];
/// Knocks alone: real play had at most 2 in a minute.
const KNOCKS: &[Limit] = &[(5, 60 * SECOND)];
/// The counter never did anything odd in real play, so three of these are enough.
const ODDITIES: &[Limit] = &[(2, 180 * SECOND)];
/// How long Fortnite's clips stop the first time; the second time, it's until Fortnite is left.
const PAUSE: i64 = 300 * SECOND;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Why {
    TooMany,
    TooManyKnocks,
    Odd,
}

impl Why {
    pub fn describe(self) -> &'static str {
        match self {
            Why::TooMany => "too many moments at once",
            Why::TooManyKnocks => "too many knocks at once",
            Why::Odd => "the counter keeps doing what it can't",
        }
    }
}

/// Stops the clips when what's read doesn't look like a game.
struct Breaker {
    moments: Budget,
    knocks: Budget,
    oddities: Budget,
    until: Option<i64>,
    trips: u32,
}

impl Breaker {
    fn new() -> Self {
        Self { moments: Budget::new(MOMENTS), knocks: Budget::new(KNOCKS), oddities: Budget::new(ODDITIES), until: None, trips: 0 }
    }

    fn paused(&self, time: i64) -> bool {
        self.until.is_some_and(|t| time < t)
    }

    fn trip(&mut self, why: Why, time: i64) -> Note {
        self.trips += 1;
        let for_good = self.trips > 1;
        self.until = Some(if for_good { i64::MAX } else { time + PAUSE });
        self.moments.clear();
        self.knocks.clear();
        self.oddities.clear();
        Note::Paused { why, for_good }
    }
}

/// What came of one sample.
#[derive(Debug, PartialEq)]
pub enum Note {
    /// A match started: the counter shows 0.
    Counter(u32),
    Eliminations { by: u32, to: u32, time: i64 },
    Knock(i64),
    Victory(i64),
    /// The counter did something it can't, or "KNOCKED!" didn't go away.
    Odd(String),
    /// Something real-looking that the pause or a limit left out.
    LeftOut(&'static str),
    Paused { why: Why, for_good: bool },
    Resumed,
}

/// One area of the screen (24-bit BGR).
pub struct Area<'a> {
    pub bgr: &'a [u8],
    pub width: usize,
    pub height: usize,
}

#[derive(Default)]
pub struct Areas<'a> {
    pub digits: Option<Area<'a>>,
    pub icon: Option<Area<'a>>,
    pub victory: Option<Area<'a>>,
    pub knock: Option<Area<'a>>,
}

/// The HUD was laid out for 16:9. On any other picture it's somewhere else.
pub fn picture_fits(width: u32, height: u32) -> bool {
    height > 0 && (width as f32 / height as f32 / (16.0 / 9.0) - 1.0).abs() < 0.015
}

/// Everything read from Fortnite's screen in one session.
pub struct Reader {
    counter: Counter,
    banner: Banner,
    knocks: Knocks,
    breaker: Breaker,
    /// A pause was reported and hasn't ended yet.
    pausing: bool,
}

impl Default for Reader {
    fn default() -> Self {
        Self { counter: Counter::default(), banner: Banner::default(), knocks: Knocks::default(), breaker: Breaker::new(), pausing: false }
    }
}

impl Reader {
    pub fn paused(&self, time: i64) -> bool {
        self.breaker.paused(time)
    }

    fn odd(&mut self, what: String, time: i64, notes: &mut Vec<Note>) {
        notes.push(Note::Odd(what));
        if !self.breaker.paused(time) && !self.breaker.oddities.take(time) {
            notes.push(self.breaker.trip(Why::Odd, time));
        }
    }

    /// True if `count` more moments may become clips.
    fn allow(&mut self, count: u32, knock: bool, time: i64, notes: &mut Vec<Note>) -> bool {
        if self.breaker.paused(time) {
            return false;
        }
        if knock && !self.breaker.knocks.take(time) {
            notes.push(self.breaker.trip(Why::TooManyKnocks, time));
            return false;
        }
        for _ in 0..count {
            if !self.breaker.moments.take(time) {
                notes.push(self.breaker.trip(Why::TooMany, time));
                return false;
            }
        }
        true
    }

    /// One sample of the screen at `time`.
    pub fn sample(&mut self, areas: &Areas, time: i64) -> Vec<Note> {
        let mut notes = Vec::new();
        if self.pausing && !self.breaker.paused(time) {
            self.pausing = false;
            notes.push(Note::Resumed);
        }
        if let Some(a) = &areas.victory
            && let Some(at) = self.banner.feed(is_victory(a.bgr, a.width, a.height), time)
        {
            let ok = self.allow(1, false, at, &mut notes);
            notes.push(if ok { Note::Victory(at) } else { Note::LeftOut("Victory Royale") });
        }
        if let (Some(d), Some(i)) = (&areas.digits, &areas.icon) {
            let reading = read(d.bgr, d.width, d.height, i.bgr, i.width, i.height);
            if reading.is_some() {
                self.knocks.counter_seen(time);
            }
            match self.counter.feed(reading, time) {
                Some(Change::Count(n)) => {
                    self.banner.new_match();
                    notes.push(Note::Counter(n));
                }
                Some(Change::Eliminations { by, to, time: at }) => {
                    let ok = self.allow(by, false, at, &mut notes);
                    notes.push(if ok { Note::Eliminations { by, to, time: at } } else { Note::LeftOut("elimination") });
                }
                Some(Change::Odd(n)) => self.odd(format!("the counter went back to or jumped to {n}"), time, &mut notes),
                None => {}
            }
        }
        if let Some(k) = &areas.knock {
            match self.knocks.feed(is_knock(k.bgr, k.width, k.height), time) {
                Some(Knock::New) => {
                    let ok = self.allow(1, true, time, &mut notes);
                    notes.push(if ok { Note::Knock(time) } else { Note::LeftOut("knock") });
                }
                Some(Knock::Stuck) => self.odd("\"KNOCKED!\" isn't going away".into(), time, &mut notes),
                None => {}
            }
        }
        self.pausing = self.breaker.paused(time);
        notes
    }
}
