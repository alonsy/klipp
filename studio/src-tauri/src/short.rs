//! Shorts (Content Creator Mode): a clip and its camera track
//! put together as a 1080×1920 video for TikTok, Reels and YouTube Shorts.
//! The Content Creation Center's project says where each layer sits and which
//! part of its picture shows, over time (keyframes, eased in and out); behind
//! them a blurred copy of the game or black, on top the socials badge.
//!
//! ffmpeg.exe decodes and encodes; the picture in between is put together
//! here, in YUV 4:2:0, with the same geometry as the editor's preview
//! (`short.ts`), so the file looks like the preview.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::export::{self, EXPORTS_DIR};
use crate::ffmpeg;

pub const WIDTH: usize = 1080;
pub const HEIGHT: usize = 1920;
/// The blurred background is worked out at this fraction of the size.
const BLUR_SCALE: usize = 8;

/// Where a layer is at one moment: its frame on the 1080×1920 canvas, how far
/// it's zoomed into its picture (1 = the picture just covers the frame) and
/// which point of the picture is in the middle (0–1 across and down).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub zoom: f64,
    pub cx: f64,
    pub cy: f64,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Rect,
    Rounded,
    /// Round corners as big as they get (a circle in a square frame).
    Circle,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Layer {
    pub visible: bool,
    pub shape: Shape,
    /// Corner radius for `Rounded`, in canvas pixels.
    pub radius: f64,
    /// At least one; sorted by time.
    pub keys: Vec<Key>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    Blur,
    Black,
}

/// The socials badge as the page drew it: RGBA pixels (not premultiplied)
/// and where its top-left corner goes on the canvas.
#[derive(Deserialize)]
pub struct Badge {
    pub x: f64,
    pub y: f64,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRequest {
    pub path: String,
    /// The part of the clip, in seconds.
    pub start: f64,
    pub end: f64,
    pub background: Background,
    /// The camera is shown this many seconds later (earlier if negative) than it was captured.
    pub cam_shift: f64,
    pub game: Layer,
    pub camera: Option<Layer>,
    pub badge: Option<Badge>,
    /// Per separate audio track, and the premix's level (as exports take them).
    pub volumes: Vec<f32>,
    pub mix_volume: f32,
    /// 30 or 60 (never more than the clip has).
    pub fps: f64,
}

/// The layer's place at clip time `t`: eased in and out between keyframes
/// (zoom in proportion, so zooming in feels even), held before the first and
/// after the last.
pub fn at(keys: &[Key], t: f64) -> Key {
    let Some(&first) = keys.first() else {
        return Key { t, x: 0.0, y: 0.0, w: WIDTH as f64, h: HEIGHT as f64, zoom: 1.0, cx: 0.5, cy: 0.5 };
    };
    let last = keys[keys.len() - 1];
    if keys.len() == 1 || t <= first.t {
        return Key { t, ..first };
    }
    if t >= last.t {
        return Key { t, ..last };
    }
    let i = keys.windows(2).position(|w| t < w[1].t).unwrap_or(keys.len() - 2);
    let (a, b) = (keys[i], keys[i + 1]);
    let u = ((t - a.t) / (b.t - a.t).max(1e-9)).clamp(0.0, 1.0);
    let e = u * u * (3.0 - 2.0 * u);
    let mix = |p: f64, q: f64| p + (q - p) * e;
    Key {
        t,
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        w: mix(a.w, b.w),
        h: mix(a.h, b.h),
        zoom: mix(a.zoom.max(1e-3).ln(), b.zoom.max(1e-3).ln()).exp(),
        cx: mix(a.cx, b.cx),
        cy: mix(a.cy, b.cy),
    }
}

/// Where a layer's picture lands: canvas point (X, Y) inside the frame shows
/// picture point (`sx0` + (X − `x`) / `scale`, `sy0` + (Y − `y`) / `scale`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
    pub sx0: f64,
    pub sy0: f64,
}

/// The picture covers the frame at zoom 1; its middle stays inside the
/// picture, so the frame never shows past its edges.
pub fn view(k: &Key, src_w: f64, src_h: f64) -> View {
    let (w, h) = (k.w.max(1.0), k.h.max(1.0));
    let scale = (w / src_w).max(h / src_h) * k.zoom.max(1.0);
    let (vw, vh) = (w / scale, h / scale);
    let sx0 = (k.cx * src_w - vw / 2.0).clamp(0.0, (src_w - vw).max(0.0));
    let sy0 = (k.cy * src_h - vh / 2.0).clamp(0.0, (src_h - vh).max(0.0));
    View { x: k.x, y: k.y, w, h, scale, sx0, sy0 }
}

/// A YUV 4:2:0 picture: the Y plane, then U and V at half size (rounded up).
pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Picture {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, data: vec![0; Self::bytes(w, h)] }
    }

    pub fn bytes(w: usize, h: usize) -> usize {
        w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
    }

    fn planes(&self) -> [Plane<'_>; 3] {
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let (y, rest) = self.data.split_at(self.w * self.h);
        let (u, v) = rest.split_at(cw * ch);
        [Plane { data: y, w: self.w, h: self.h }, Plane { data: u, w: cw, h: ch }, Plane { data: v, w: cw, h: ch }]
    }
}

#[derive(Clone, Copy)]
struct Plane<'a> {
    data: &'a [u8],
    w: usize,
    h: usize,
}

impl Plane<'_> {
    /// Bilinear sample at (x, y), in pixel-centre coordinates, held at the edges.
    #[inline]
    fn sample(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.w - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (r0, r1) = (y0 * self.w, y1 * self.w);
        let top = self.data[r0 + x0] as f32 + (self.data[r0 + x1] as f32 - self.data[r0 + x0] as f32) * fx;
        let bottom = self.data[r1 + x0] as f32 + (self.data[r1 + x1] as f32 - self.data[r1 + x0] as f32) * fx;
        top + (bottom - top) * fy
    }
}

/// One layer to draw on this frame.
struct Draw<'a> {
    picture: &'a Picture,
    view: View,
    shape: Shape,
    radius: f64,
}

impl Draw<'_> {
    /// How much of the pixel at canvas point (`px`, `py`) the frame covers,
    /// for pixels `pixel` canvas pixels wide (anti-aliased edges and corners).
    #[inline]
    fn alpha(&self, px: f32, py: f32, pixel: f32) -> f32 {
        let v = &self.view;
        let (hw, hh) = ((v.w / 2.0) as f32, (v.h / 2.0) as f32);
        let r = match self.shape {
            Shape::Rect => 0.0,
            Shape::Rounded => (self.radius as f32).clamp(0.0, hw.min(hh)),
            Shape::Circle => hw.min(hh),
        };
        let qx = (px - (v.x as f32 + hw)).abs() - hw + r;
        let qy = (py - (v.y as f32 + hh)).abs() - hh + r;
        let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
        (0.5 - d / pixel).clamp(0.0, 1.0)
    }
}

/// The socials badge in YUV with its coverage, chroma at half size.
pub struct BadgeYuv {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    luma: Vec<f32>,
    alpha: Vec<f32>,
    u: Vec<f32>,
    v: Vec<f32>,
    chroma_alpha: Vec<f32>,
}

/// BT.709, limited range: what the clips are and the short is tagged as.
fn yuv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    (
        16.0 + 219.0 * (0.2126 * r + 0.7152 * g + 0.0722 * b),
        128.0 + 224.0 * (-0.1146 * r - 0.3854 * g + 0.5 * b),
        128.0 + 224.0 * (0.5 * r - 0.4542 * g - 0.0458 * b),
    )
}

pub fn badge_yuv(b: &Badge) -> Option<BadgeYuv> {
    if b.width == 0 || b.height == 0 || b.rgba.len() < b.width * b.height * 4 {
        return None;
    }
    // Even, so its chroma lines up with the canvas's.
    let x = (b.x.max(0.0).round() as usize) & !1;
    let y = (b.y.max(0.0).round() as usize) & !1;
    let (w, h) = (b.width, b.height);
    let px = |i: usize, j: usize| {
        let o = (j * w + i) * 4;
        (b.rgba[o] as f32, b.rgba[o + 1] as f32, b.rgba[o + 2] as f32, b.rgba[o + 3] as f32 / 255.0)
    };
    let mut luma = vec![0.0; w * h];
    let mut alpha = vec![0.0; w * h];
    for j in 0..h {
        for i in 0..w {
            let (r, g, bl, a) = px(i, j);
            luma[j * w + i] = yuv(r, g, bl).0;
            alpha[j * w + i] = a;
        }
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (mut u, mut v, mut chroma_alpha) = (vec![128.0; cw * ch], vec![128.0; cw * ch], vec![0.0; cw * ch]);
    for j in 0..ch {
        for i in 0..cw {
            let (mut su, mut sv, mut sa, mut n) = (0.0, 0.0, 0.0, 0.0);
            for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (pi, pj) = (2 * i + di, 2 * j + dj);
                if pi < w && pj < h {
                    let (r, g, bl, a) = px(pi, pj);
                    let (_, cu, cv) = yuv(r, g, bl);
                    su += cu * a;
                    sv += cv * a;
                    sa += a;
                    n += 1.0;
                }
            }
            if sa > 0.0 {
                u[j * cw + i] = su / sa;
                v[j * cw + i] = sv / sa;
            }
            chroma_alpha[j * cw + i] = sa / n;
        }
    }
    Some(BadgeYuv { x, y, w, h, luma, alpha, u, v, chroma_alpha })
}

/// A blurred, darker copy of the game covering the canvas, at 1/`BLUR_SCALE` size.
fn blurred(game: &Picture) -> Picture {
    let (bw, bh) = (WIDTH / BLUR_SCALE, HEIGHT / BLUR_SCALE);
    let mut small = Picture::new(bw, bh);
    let cover = view(&Key { t: 0.0, x: 0.0, y: 0.0, w: WIDTH as f64, h: HEIGHT as f64, zoom: 1.0, cx: 0.5, cy: 0.5 }, game.w as f64, game.h as f64);
    let src = game.planes();
    let (cw, ch) = (bw.div_ceil(2), bh.div_ceil(2));
    {
        let (y, rest) = small.data.split_at_mut(bw * bh);
        let (u, v) = rest.split_at_mut(cw * ch);
        for (plane, out, w, h, step) in [(0, y, bw, bh, 1.0), (1, u, cw, ch, 2.0), (2, v, cw, ch, 2.0)] {
            for j in 0..h {
                for i in 0..w {
                    // Canvas point of this small pixel's centre, then the game's.
                    let cx = (i as f64 + 0.5) * BLUR_SCALE as f64 * step;
                    let cy = (j as f64 + 0.5) * BLUR_SCALE as f64 * step;
                    let gx = (cover.sx0 + cx / cover.scale) / step - 0.5;
                    let gy = (cover.sy0 + cy / cover.scale) / step - 0.5;
                    out[j * w + i] = src[plane].sample(gx as f32, gy as f32) as u8;
                }
            }
            let radius = if plane == 0 { 6 } else { 3 };
            for _ in 0..3 {
                box_blur(out, w, h, radius);
            }
            // Darker and calmer, so the layers stand out.
            for p in out.iter_mut() {
                *p = if plane == 0 { 16.0 + (*p as f32 - 16.0) * 0.55 } else { 128.0 + (*p as f32 - 128.0) * 0.8 } as u8;
            }
        }
    }
    small
}

/// One pass of a box blur, across then down, held at the edges.
fn box_blur(p: &mut [u8], w: usize, h: usize, r: usize) {
    let mut line = Vec::with_capacity(w.max(h));
    let mut pass = |get: &dyn Fn(usize) -> usize, n: usize, p: &mut [u8]| {
        line.clear();
        line.extend((0..n).map(|k| p[get(k)] as u32));
        let at = |k: isize| line[k.clamp(0, n as isize - 1) as usize];
        let size = (2 * r + 1) as u32;
        let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
        for k in 0..n {
            p[get(k)] = (sum / size) as u8;
            sum += at(k as isize + r as isize + 1);
            sum -= at(k as isize - r as isize);
        }
    };
    for j in 0..h {
        pass(&|k| j * w + k, w, p);
    }
    for i in 0..w {
        pass(&|k| k * w + i, h, p);
    }
}

/// Everything one frame of the short is made of.
struct Scene<'a> {
    background: Option<&'a Picture>,
    layers: Vec<Draw<'a>>,
    badge: Option<&'a BadgeYuv>,
}

/// Draws the frame into `out` (1080×1920), in bands of rows on every core.
fn compose(out: &mut Picture, scene: &Scene) {
    let threads = thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    let band = (HEIGHT.div_ceil(threads) + 1) & !1;
    let (cw, ch) = (WIDTH / 2, HEIGHT / 2);
    let (ys, rest) = out.data.split_at_mut(WIDTH * HEIGHT);
    let (us, vs) = rest.split_at_mut(cw * ch);
    thread::scope(|s| {
        for (k, ((y, u), v)) in ys.chunks_mut(WIDTH * band).zip(us.chunks_mut(cw * band / 2)).zip(vs.chunks_mut(cw * band / 2)).enumerate() {
            s.spawn(move || compose_band(k * band, y, u, v, scene));
        }
    });
}

fn compose_band(top: usize, ys: &mut [u8], us: &mut [u8], vs: &mut [u8], scene: &Scene) {
    let rows = ys.len() / WIDTH;
    let cw = WIDTH / 2;
    let (ctop, crows) = (top / 2, us.len() / cw);
    match scene.background {
        None => {
            ys.fill(16);
            us.fill(128);
            vs.fill(128);
        }
        Some(bg) => {
            let p = bg.planes();
            let (fx, fy) = (bg.w as f32 / WIDTH as f32, bg.h as f32 / HEIGHT as f32);
            for j in 0..rows {
                let sy = (top + j) as f32 * fy + fy / 2.0 - 0.5;
                for i in 0..WIDTH {
                    ys[j * WIDTH + i] = p[0].sample(i as f32 * fx + fx / 2.0 - 0.5, sy) as u8;
                }
            }
            let (fx, fy) = (p[1].w as f32 / cw as f32, p[1].h as f32 / (HEIGHT / 2) as f32);
            for j in 0..crows {
                let sy = (ctop + j) as f32 * fy + fy / 2.0 - 0.5;
                for i in 0..cw {
                    let sx = i as f32 * fx + fx / 2.0 - 0.5;
                    us[j * cw + i] = p[1].sample(sx, sy) as u8;
                    vs[j * cw + i] = p[2].sample(sx, sy) as u8;
                }
            }
        }
    }
    for layer in &scene.layers {
        draw(layer, top, rows, ys, us, vs);
    }
    if let Some(b) = scene.badge {
        draw_badge(b, top, rows, ys, us, vs);
    }
}

/// The canvas pixels `from..to` (luma, or chroma with `step` 2) that `lo..hi` canvas units touch.
fn span(lo: f64, hi: f64, step: f64, limit: usize) -> (usize, usize) {
    let a = (lo / step).floor().max(0.0) as usize;
    let b = ((hi / step).ceil().max(0.0) as usize).min(limit);
    (a.min(b), b)
}

#[inline]
fn blend(dst: &mut u8, src: f32, a: f32) {
    let d = *dst as f32;
    *dst = (d + (src - d) * a).round().clamp(0.0, 255.0) as u8;
}

fn draw(layer: &Draw, top: usize, rows: usize, ys: &mut [u8], us: &mut [u8], vs: &mut [u8]) {
    let v = &layer.view;
    let p = layer.picture.planes();
    let inv = (1.0 / v.scale) as f32;
    let (x0, y0, sx0, sy0) = (v.x as f32, v.y as f32, v.sx0 as f32, v.sy0 as f32);
    let (c0, c1) = span(v.x, v.x + v.w, 1.0, WIDTH);
    let (r0, r1) = span(v.y, v.y + v.h, 1.0, HEIGHT);
    for row in r0.max(top)..r1.min(top + rows) {
        let py = row as f32 + 0.5;
        let sy = sy0 + (py - y0) * inv - 0.5;
        let out = &mut ys[(row - top) * WIDTH..(row - top + 1) * WIDTH];
        for col in c0..c1 {
            let px = col as f32 + 0.5;
            let a = layer.alpha(px, py, 1.0);
            if a > 0.0 {
                blend(&mut out[col], p[0].sample(sx0 + (px - x0) * inv - 0.5, sy), a);
            }
        }
    }
    let cw = WIDTH / 2;
    let (ctop, crows) = (top / 2, us.len() / cw);
    let (c0, c1) = span(v.x, v.x + v.w, 2.0, cw);
    let (r0, r1) = span(v.y, v.y + v.h, 2.0, HEIGHT / 2);
    for row in r0.max(ctop)..r1.min(ctop + crows) {
        // The chroma pixel's centre, in canvas (luma) units.
        let py = row as f32 * 2.0 + 1.0;
        let sy = (sy0 + (py - y0) * inv) / 2.0 - 0.5;
        let at = (row - ctop) * cw;
        for col in c0..c1 {
            let px = col as f32 * 2.0 + 1.0;
            let a = layer.alpha(px, py, 2.0);
            if a > 0.0 {
                let sx = (sx0 + (px - x0) * inv) / 2.0 - 0.5;
                blend(&mut us[at + col], p[1].sample(sx, sy), a);
                blend(&mut vs[at + col], p[2].sample(sx, sy), a);
            }
        }
    }
}

fn draw_badge(b: &BadgeYuv, top: usize, rows: usize, ys: &mut [u8], us: &mut [u8], vs: &mut [u8]) {
    for j in b.y.max(top)..(b.y + b.h).min(top + rows).min(HEIGHT) {
        for i in b.x..(b.x + b.w).min(WIDTH) {
            let k = (j - b.y) * b.w + (i - b.x);
            if b.alpha[k] > 0.0 {
                blend(&mut ys[(j - top) * WIDTH + i], b.luma[k], b.alpha[k]);
            }
        }
    }
    let (cw, bcw) = (WIDTH / 2, b.w.div_ceil(2));
    let (ctop, crows) = (top / 2, us.len() / cw);
    let (bx, by) = (b.x / 2, b.y / 2);
    for j in by.max(ctop)..(by + b.h.div_ceil(2)).min(ctop + crows).min(HEIGHT / 2) {
        for i in bx..(bx + bcw).min(cw) {
            let k = (j - by) * bcw + (i - bx);
            let a = b.chroma_alpha[k];
            if a > 0.0 {
                blend(&mut us[(j - ctop) * cw + i], b.u[k], a);
                blend(&mut vs[(j - ctop) * cw + i], b.v[k], a);
            }
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: u64,
    fraction: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Done {
    id: u64,
    path: String,
    size_bytes: u64,
}

#[derive(Serialize, Clone)]
struct Failed {
    id: u64,
    error: String,
}

/// A render in progress: its ffmpeg processes, to stop them when cancelled.
#[derive(Default)]
struct Job {
    cancelled: AtomicBool,
    children: Mutex<Vec<Child>>,
}

impl Job {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        for c in self.children.lock().unwrap().iter_mut() {
            let _ = c.kill();
        }
    }
}

#[derive(Default)]
pub struct Shorts {
    next: AtomicU64,
    running: Arc<Mutex<HashMap<u64, Arc<Job>>>>,
}

impl Shorts {
    /// Starts a render; progress and the result arrive as "short-progress",
    /// "short-done" and "short-failed" events.
    pub fn start(&self, app: AppHandle, clips_dir: PathBuf, req: ShortRequest) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let job = Arc::new(Job::default());
        self.running.lock().unwrap().insert(id, job.clone());
        let running = self.running.clone();
        thread::spawn(move || {
            let progress = |fraction: f64| {
                let _ = app.emit("short-progress", Progress { id, fraction });
            };
            let result = render(&clips_dir, &req, &job, &progress);
            for mut c in job.children.lock().unwrap().drain(..) {
                let _ = c.kill();
                let _ = c.wait();
            }
            running.lock().unwrap().remove(&id);
            let _ = match result {
                Ok(path) => {
                    let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    app.emit("short-done", Done { id, path: path.to_string_lossy().into_owned(), size_bytes })
                }
                Err(e) => app.emit("short-failed", Failed { id, error: format!("{e:#}") }),
            };
        });
        id
    }

    pub fn busy(&self) -> bool {
        !self.running.lock().unwrap().is_empty()
    }

    pub fn cancel(&self, id: u64) {
        if let Some(job) = self.running.lock().unwrap().get(&id) {
            job.cancel();
        }
    }
}

/// A video stream of the clip.
struct Stream {
    width: usize,
    height: usize,
    fps: f64,
}

/// The clip's video streams (the game first, then the camera if it has one) and how many audio tracks it has.
fn probe(clip: &Path) -> Result<(Vec<Stream>, usize)> {
    let out = ffmpeg::probe_command().args(["-show_entries", "stream=codec_type,width,height,avg_frame_rate", "-of", "json"]).arg(clip).output()?;
    if !out.status.success() {
        bail!("couldn't read {}", clip.display());
    }
    let json: Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"].as_array().context("no streams")?;
    let video = streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .map(|s| Stream {
            width: s["width"].as_u64().unwrap_or(0) as usize,
            height: s["height"].as_u64().unwrap_or(0) as usize,
            fps: s["avg_frame_rate"]
                .as_str()
                .and_then(|r| r.split_once('/'))
                .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?.max(1.0)))
                .unwrap_or(60.0),
        })
        .filter(|s| s.width > 0 && s.height > 0)
        .collect();
    Ok((video, streams.iter().filter(|s| s["codec_type"] == "audio").count()))
}

fn encoder_args(name: &str) -> Vec<&'static str> {
    let mut a = match name {
        "h264_nvenc" => vec!["-c:v", "h264_nvenc", "-preset", "p6", "-tune", "hq", "-rc", "vbr", "-cq", "19", "-b:v", "0"],
        "h264_amf" => vec!["-c:v", "h264_amf", "-quality", "quality", "-rc", "vbr_peak", "-b:v", "16M"],
        "h264_qsv" => vec!["-c:v", "h264_qsv", "-preset", "slow", "-b:v", "16M"],
        _ => vec!["-c:v", "h264_mf", "-b:v", "16M"],
    };
    // Shorts are capped well above what the platforms keep.
    a.extend(["-maxrate", "30M", "-bufsize", "60M", "-pix_fmt", "yuv420p"]);
    // Windows' own encoder refuses a profile by name.
    if name != "h264_mf" {
        a.extend(["-profile:v", "high"]);
    }
    a.extend(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
    a
}

/// Reads whole pictures from a decoder into `tx`, reusing buffers that come back through `free`.
fn read_pictures(mut from: impl Read + Send + 'static, w: usize, h: usize, tx: SyncSender<Picture>, free: Receiver<Vec<u8>>) {
    thread::spawn(move || {
        let size = Picture::bytes(w, h);
        loop {
            let mut data = free.try_recv().unwrap_or_else(|_| vec![0; size]);
            data.resize(size, 0);
            if from.read_exact(&mut data).is_err() || tx.send(Picture { w, h, data }).is_err() {
                return;
            }
        }
    });
}

/// Reads a process's errors on the side, so a chatty ffmpeg can't block on a full pipe.
fn collect_errors(mut from: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut s = String::new();
        let _ = from.read_to_string(&mut s);
        s
    })
}

fn last_line(s: &str) -> &str {
    s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

/// A decoder for one video stream from `seek` seconds, at a steady `fps`,
/// `frames` pictures of YUV 4:2:0 (padded at the start with the first picture
/// if the stream starts later), shifted by `shift` seconds.
fn decoder(clip: &Path, stream: &str, seek: f64, shift: f64, fps: f64, frames: usize) -> Result<Child> {
    let filters = format!("setpts=PTS+({shift:.4})/TB,fps={fps}:start_time=0,format=yuv420p");
    ffmpeg::command()
        .args(["-hwaccel", "auto", "-ss", &format!("{seek:.3}"), "-i"])
        .arg(clip)
        .args(["-map", stream, "-vf", &filters, "-frames:v", &frames.to_string(), "-f", "rawvideo", "-pix_fmt", "yuv420p", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting ffmpeg.exe")
}

/// Renders the short. Returns the file.
fn render(clips_dir: &Path, req: &ShortRequest, job: &Job, on_progress: &dyn Fn(f64)) -> Result<PathBuf> {
    let clip = PathBuf::from(&req.path);
    let (video, audio_streams) = probe(&clip)?;
    let game = video.first().context("the clip has no video")?;
    let camera = req.camera.as_ref().filter(|c| c.visible && !c.keys.is_empty()).zip(video.get(1));
    let fps = req.fps.clamp(24.0, 60.0).min(game.fps.round().max(24.0));
    let duration = (req.end - req.start).max(0.1);
    let frames = (duration * fps).round().max(1.0) as usize;

    let stem = clip.file_stem().context("no file name")?.to_string_lossy().into_owned();
    let dir = clips_dir.join(EXPORTS_DIR);
    fs::create_dir_all(&dir)?;
    let out = export::unique(&dir.join(format!("{stem}_short.mp4")));
    let part = out.with_extension("part.mp4");

    // Decoders: the game, and the camera shifted by the sync setting (sought
    // that much earlier, when it's shown later).
    let mut game_dec = decoder(&clip, "0:v:0", req.start, 0.0, fps, frames)?;
    let game_out = game_dec.stdout.take().context("no output")?;
    let game_err = collect_errors(game_dec.stderr.take().context("no errors")?);
    job.children.lock().unwrap().push(game_dec);
    let (game_tx, game_rx) = mpsc::sync_channel(3);
    let (game_free_tx, game_free_rx) = mpsc::channel();
    read_pictures(game_out, game.width, game.height, game_tx, game_free_rx);

    let mut cam_rx = None;
    if let Some((_, stream)) = camera {
        let seek = (req.start - req.cam_shift).max(0.0);
        let mut dec = decoder(&clip, "0:v:1", seek, seek + req.cam_shift - req.start, fps, frames)?;
        let out = dec.stdout.take().context("no output")?;
        // A camera that ends early holds its last picture; its errors don't stop the short.
        drop(collect_errors(dec.stderr.take().context("no errors")?));
        job.children.lock().unwrap().push(dec);
        let (tx, rx) = mpsc::sync_channel(3);
        let (free_tx, free_rx) = mpsc::channel();
        read_pictures(out, stream.width, stream.height, tx, free_rx);
        cam_rx = Some((rx, free_tx));
    }

    // The encoder: our pictures, and the clip's audio mixed as an export mixes it.
    let mut args: Vec<String> = ["-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", &format!("{WIDTH}x{HEIGHT}"), "-r", &format!("{fps}"), "-i", "pipe:0"]
        .map(String::from)
        .to_vec();
    args.extend(["-ss".into(), format!("{:.3}", req.start), "-t".into(), format!("{duration:.3}"), "-i".into(), clip.to_string_lossy().into_owned()]);
    let audio = export::audio_filter_from(1, audio_streams, &req.volumes, req.mix_volume);
    if let Some(a) = &audio {
        args.extend(["-filter_complex".into(), a.clone()]);
    }
    args.extend(["-map".into(), "0:v".into()]);
    args.extend(encoder_args(ffmpeg::h264_encoder()).into_iter().map(String::from));
    if audio.is_some() {
        args.extend(["-map", "[aout]", "-c:a", "aac", "-b:a", "192k", "-ar", "48000"].map(String::from));
    }
    args.extend(["-movflags", "+faststart", "-y"].map(String::from));
    args.push(part.to_string_lossy().into_owned());
    let mut enc = ffmpeg::command().args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().context("starting ffmpeg.exe")?;
    let mut stdin = enc.stdin.take().context("no input")?;
    let enc_err = collect_errors(enc.stderr.take().context("no errors")?);
    job.children.lock().unwrap().push(enc);

    let badge = req.badge.as_ref().and_then(badge_yuv);
    let mut canvas = Picture::new(WIDTH, HEIGHT);
    let mut cam_last: Option<Picture> = None;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    let mut written = 0;
    let result = (|| -> Result<()> {
        for n in 0..frames {
            if job.cancelled.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let Ok(g) = game_rx.recv() else { break };
            if let Some((rx, free)) = &cam_rx
                && let Ok(c) = rx.recv()
                && let Some(old) = cam_last.replace(c)
            {
                let _ = free.send(old.data);
            }
            let t = req.start + n as f64 / fps;
            let background = (req.background == Background::Blur).then(|| blurred(&g));
            let mut layers = Vec::new();
            if req.game.visible && !req.game.keys.is_empty() {
                let k = at(&req.game.keys, t);
                layers.push(Draw { picture: &g, view: view(&k, g.w as f64, g.h as f64), shape: req.game.shape, radius: req.game.radius });
            }
            if let (Some((layer, _)), Some(c)) = (camera, &cam_last) {
                let k = at(&layer.keys, t);
                layers.push(Draw { picture: c, view: view(&k, c.w as f64, c.h as f64), shape: layer.shape, radius: layer.radius });
            }
            compose(&mut canvas, &Scene { background: background.as_ref(), layers, badge: badge.as_ref() });
            stdin.write_all(&canvas.data).context("the encoder stopped")?;
            let _ = game_free_tx.send(g.data);
            written = n + 1;
            if last_report.elapsed() > Duration::from_millis(100) {
                on_progress(written as f64 / frames as f64 * 0.98);
                last_report = Instant::now();
            }
        }
        Ok(())
    })();
    drop(stdin);
    if job.cancelled.load(Ordering::Relaxed) {
        let _ = fs::remove_file(&part);
        bail!("cancelled");
    }
    if let Err(e) = result {
        job.cancel();
        let _ = fs::remove_file(&part);
        let detail = [enc_err.join().unwrap_or_default(), game_err.join().unwrap_or_default()].iter().map(|s| last_line(s).to_string()).find(|s| !s.is_empty());
        bail!("{e:#}{}", detail.map(|d| format!(": {d}")).unwrap_or_default());
    }
    if written == 0 {
        job.cancel();
        let _ = fs::remove_file(&part);
        bail!("ffmpeg gave no picture of the clip: {}", last_line(&game_err.join().unwrap_or_default()));
    }
    // The encoder finishes once its input ends.
    let enc = job.children.lock().unwrap().pop();
    let status = match enc {
        Some(mut e) => e.wait()?,
        None => bail!("cancelled"),
    };
    if !status.success() {
        let _ = fs::remove_file(&part);
        bail!("encoding failed: {}", last_line(&enc_err.join().unwrap_or_default()));
    }
    fs::rename(&part, &out)?;
    on_progress(1.0);
    Ok(out)
}
