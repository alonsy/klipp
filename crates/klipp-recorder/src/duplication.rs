//! Monitor capture through DXGI Desktop Duplication: what Klipp uses on
//! Windows 10, where Windows Graphics Capture draws a yellow border around
//! the screen and apps can't turn it off (Windows 11 can). Windows sends the
//! mouse cursor separately here, so it's drawn onto each frame.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::core::{Interface, PCSTR, s};

use crate::capture::LatestFrame;
use crate::gpu::{Gpu, create_bgra_texture};
use crate::log::{info, warning};

/// Windows 10 (desktop builds end at 19045) can't hide Windows Graphics
/// Capture's border; that arrived with build 20348.
pub fn border_cannot_be_hidden() -> bool {
    windows_build() < 20348
}

fn windows_build() -> u32 {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let mut buf = [0u16; 16];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    }
    .is_ok();
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    if ok { String::from_utf16_lossy(&buf[..n]).parse().unwrap_or(u32::MAX) } else { u32::MAX }
}

pub struct DuplicationCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub latest: Arc<LatestFrame>,
}

/// D3D objects used on the duplication thread.
struct Worker {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    duplication: IDXGIOutputDuplication,
    latest: Arc<LatestFrame>,
    /// The desktop without the cursor; the cursor is drawn on a copy of it.
    desktop: ID3D11Texture2D,
    painter: CursorPainter,
    interval: Duration,
}
// SAFETY: D3D11 devices are free-threaded and the context is multithread-protected (see gpu.rs).
unsafe impl Send for Worker {}

impl DuplicationCapture {
    pub fn start(gpu: &Gpu, monitor: HMONITOR, max_fps: u32) -> Result<Self> {
        let output = output_for(gpu, monitor)?;
        let duplication = duplicate(&output, &gpu.device)?;
        let (width, height) = size_of_duplication(&duplication)?;
        let latest = LatestFrame::new(&gpu.device, width, height)?;
        let worker = Worker {
            device: gpu.device.clone(),
            context: gpu.context.clone(),
            output,
            duplication,
            latest: latest.clone(),
            desktop: create_bgra_texture(&gpu.device, width, height)?,
            painter: CursorPainter::new(&gpu.device)?,
            interval: Duration::from_secs_f64(1.0 / max_fps.max(1) as f64),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new().name("duplication".into()).spawn({
            let stop = stop.clone();
            move || {
                let _priority = crate::mmcss::CaptureThread::register();
                worker.run(&stop);
            }
        })?;
        Ok(Self { stop, thread: Some(thread), latest })
    }
}

impl Drop for DuplicationCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The output (screen) of `gpu`'s adapter that shows `monitor`.
fn output_for(gpu: &Gpu, monitor: HMONITOR) -> Result<IDXGIOutput1> {
    let adapter = unsafe { gpu.device.cast::<IDXGIDevice>()?.GetAdapter()? };
    for i in 0.. {
        let Ok(output) = (unsafe { adapter.EnumOutputs(i) }) else { break };
        let desc = unsafe { output.GetDesc()? };
        if desc.Monitor == monitor {
            if !matches!(desc.Rotation, DXGI_MODE_ROTATION_IDENTITY | DXGI_MODE_ROTATION_UNSPECIFIED) {
                bail!("the screen is rotated");
            }
            return Ok(output.cast()?);
        }
    }
    bail!("{} doesn't drive this screen", gpu.adapter_name)
}

fn duplicate(output: &IDXGIOutput1, device: &ID3D11Device) -> Result<IDXGIOutputDuplication> {
    Ok(unsafe { output.DuplicateOutput(device)? })
}

fn size_of_duplication(duplication: &IDXGIOutputDuplication) -> Result<(u32, u32)> {
    let desc = unsafe { duplication.GetDesc() };
    if desc.ModeDesc.Width == 0 || desc.ModeDesc.Height == 0 {
        bail!("the screen reports no size");
    }
    Ok((desc.ModeDesc.Width, desc.ModeDesc.Height))
}

#[derive(Default)]
struct Pointer {
    visible: bool,
    position: POINT,
    shape: Option<Shape>,
}

impl Worker {
    fn run(mut self, stop: &AtomicBool) {
        let mut pointer = Pointer::default();
        let mut last = Instant::now() - self.interval;
        // Waiting inside AcquireNextFrame holds the device's lock, which stalls
        // the encoder on the same device; so it only ever checks, and naps here.
        let nap = (self.interval / 4).min(Duration::from_millis(4));
        while !stop.load(Ordering::Relaxed) {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            match unsafe { self.duplication.AcquireNextFrame(0, &mut info, &mut resource) } {
                Ok(()) => {
                    if let Err(e) = self.take(&info, resource, &mut pointer) {
                        warning!("desktop duplication: {e:#}");
                    }
                    let _ = unsafe { self.duplication.ReleaseFrame() };
                    // Windows collects what changes in between; no need to look more often than we record.
                    let elapsed = last.elapsed();
                    if elapsed < self.interval {
                        thread::sleep(self.interval - elapsed);
                    }
                    last = Instant::now();
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => thread::sleep(nap),
                // A display mode change, the UAC or lock screen, or a game
                // switching to exclusive fullscreen: duplicate again.
                Err(e) => {
                    if e.code() != DXGI_ERROR_ACCESS_LOST {
                        warning!("desktop duplication: {e}; reconnecting");
                    }
                    if !self.reconnect(stop) {
                        return;
                    }
                }
            }
        }
    }

    /// Copies the new desktop image and/or pointer, then publishes desktop + cursor.
    fn take(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO, resource: Option<IDXGIResource>, pointer: &mut Pointer) -> Result<()> {
        let mut changed = false;
        if info.LastPresentTime != 0
            && let Some(resource) = resource
        {
            let texture: ID3D11Texture2D = resource.cast()?;
            unsafe { self.context.CopyResource(&self.desktop, &texture) };
            changed = true;
        }
        if info.LastMouseUpdateTime != 0 {
            pointer.visible = info.PointerPosition.Visible.as_bool();
            pointer.position = info.PointerPosition.Position;
            if info.PointerShapeBufferSize > 0 {
                let mut buffer = vec![0u8; info.PointerShapeBufferSize as usize];
                let (mut needed, mut shape_info) = (0u32, DXGI_OUTDUPL_POINTER_SHAPE_INFO::default());
                unsafe {
                    self.duplication.GetFramePointerShape(buffer.len() as u32, buffer.as_mut_ptr() as *mut c_void, &mut needed, &mut shape_info)?;
                }
                pointer.shape = Some(Shape::new(&self.device, &buffer, &shape_info)?);
            }
            changed = true;
        }
        if !changed {
            return Ok(());
        }
        let latest = self.latest.lock();
        unsafe { self.context.CopyResource(&latest.texture, &self.desktop) };
        if pointer.visible
            && let Some(shape) = &pointer.shape
        {
            let size = (latest.width, latest.height);
            self.painter.draw(&self.device, &self.context, &latest.texture, size, shape, pointer.position)?;
        }
        drop(latest);
        self.latest.publish();
        Ok(())
    }

    /// Duplicates the screen again, resizing if its mode changed. False if asked to stop meanwhile.
    fn reconnect(&mut self, stop: &AtomicBool) -> bool {
        loop {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            thread::sleep(Duration::from_millis(100));
            let Ok(duplication) = duplicate(&self.output, &self.device) else { continue };
            let Ok((width, height)) = size_of_duplication(&duplication) else { continue };
            let mut latest = self.latest.lock();
            if (width, height) != (latest.width, latest.height) {
                let Ok(desktop) = create_bgra_texture(&self.device, width, height) else { continue };
                if self.latest.resize(&mut latest, &self.device, width, height).is_err() {
                    continue;
                }
                self.desktop = desktop;
                info!("desktop duplication: the screen is now {width}x{height}");
            }
            drop(latest);
            self.duplication = duplication;
            return true;
        }
    }
}

/// A cursor image on the GPU, premultiplied BGRA.
struct Shape {
    view: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
}

impl Shape {
    fn new(device: &ID3D11Device, buffer: &[u8], info: &DXGI_OUTDUPL_POINTER_SHAPE_INFO) -> Result<Self> {
        let (width, height, pixels) = to_bgra(buffer, info).context("unknown cursor format")?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let data = D3D11_SUBRESOURCE_DATA { pSysMem: pixels.as_ptr() as *const c_void, SysMemPitch: width * 4, SysMemSlicePitch: 0 };
        let (mut texture, mut view) = (None, None);
        unsafe {
            device.CreateTexture2D(&desc, Some(&data), Some(&mut texture))?;
            device.CreateShaderResourceView(&texture.context("no cursor texture")?, None, Some(&mut view))?;
        }
        Ok(Self { view: view.context("no cursor view")?, width, height })
    }
}

/// Windows' three cursor formats as premultiplied BGRA pixels. Inverting
/// pixels (the text cursor's XOR parts) can't be drawn by blending; they
/// become half-transparent black, which reads well on both light and dark.
fn to_bgra(buffer: &[u8], info: &DXGI_OUTDUPL_POINTER_SHAPE_INFO) -> Option<(u32, u32, Vec<u32>)> {
    let (w, pitch) = (info.Width as usize, info.Pitch as usize);
    const MONOCHROME: u32 = DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME.0 as u32;
    const COLOR: u32 = DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 as u32;
    const MASKED: u32 = DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR.0 as u32;
    match info.Type {
        MONOCHROME => {
            // An AND mask, then an XOR mask, one bit per pixel.
            let h = info.Height as usize / 2;
            let bit = |row: usize, x: usize| buffer.get(row * pitch + x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0);
            let mut out = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    out.push(match (bit(y, x), bit(y + h, x)) {
                        (false, false) => 0xFF00_0000,
                        (false, true) => 0xFFFF_FFFF,
                        (true, false) => 0,
                        (true, true) => 0x8000_0000,
                    });
                }
            }
            Some((w as u32, h as u32, out))
        }
        COLOR | MASKED => {
            let h = info.Height as usize;
            let mut out = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    let i = y * pitch + x * 4;
                    let px = u32::from_le_bytes(buffer.get(i..i + 4)?.try_into().ok()?);
                    let a = px >> 24;
                    out.push(if info.Type == COLOR {
                        premultiply(px, a)
                    } else if a == 0 {
                        px | 0xFF00_0000 // shown as is
                    } else if px & 0x00FF_FFFF == 0 {
                        0 // XOR with black: no change
                    } else {
                        0x8000_0000 // XOR with a color: inverting
                    });
                }
            }
            Some((w as u32, h as u32, out))
        }
        _ => None,
    }
}

fn premultiply(px: u32, a: u32) -> u32 {
    let channel = |shift: u32| (((px >> shift) & 0xFF) * a / 255) << shift;
    (a << 24) | channel(16) | channel(8) | channel(0)
}

/// Draws a cursor image onto a frame: one textured quad, blended.
struct CursorPainter {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    blend: ID3D11BlendState,
    place: ID3D11Buffer,
    /// Render target for the frame texture it was made for.
    target: Option<(usize, ID3D11RenderTargetView)>,
}

const SHADERS: &str = r#"
cbuffer Place : register(b0) { float4 rect; };
Texture2D shape : register(t0);
SamplerState nearest : register(s0);
struct V { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
V vs(uint id : SV_VertexID) {
    float2 uv = float2(id & 1, id >> 1);
    V o;
    o.pos = float4(lerp(rect.x, rect.z, uv.x), lerp(rect.y, rect.w, uv.y), 0, 1);
    o.uv = uv;
    return o;
}
float4 ps(V i) : SV_Target { return shape.Sample(nearest, i.uv); }
"#;

fn compile(entry: PCSTR, target: PCSTR) -> Result<Vec<u8>> {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    let result = unsafe {
        D3DCompile(SHADERS.as_ptr() as *const c_void, SHADERS.len(), s!("cursor"), None, None, entry, target, 0, 0, &mut code, Some(&mut errors))
    };
    if let Err(e) = result {
        let message = errors
            .map(|b| unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() })
            .unwrap_or_default();
        bail!("compiling the cursor shader: {e} {message}");
    }
    let code = code.context("no shader code")?;
    Ok(unsafe { std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()) }.to_vec())
}

impl CursorPainter {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let (vs, ps) = (compile(s!("vs"), s!("vs_4_0"))?, compile(s!("ps"), s!("ps_4_0"))?);
        let (mut vertex, mut pixel, mut sampler, mut blend, mut place) = (None, None, None, None, None);
        let mut targets = [D3D11_RENDER_TARGET_BLEND_DESC::default(); 8];
        targets[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        unsafe {
            device.CreateVertexShader(&vs, None, Some(&mut vertex))?;
            device.CreatePixelShader(&ps, None, Some(&mut pixel))?;
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
            device.CreateBlendState(&D3D11_BLEND_DESC { RenderTarget: targets, ..Default::default() }, Some(&mut blend))?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, ..Default::default() },
                None,
                Some(&mut place),
            )?;
        }
        Ok(Self {
            vertex: vertex.context("no vertex shader")?,
            pixel: pixel.context("no pixel shader")?,
            sampler: sampler.context("no sampler")?,
            blend: blend.context("no blend state")?,
            place: place.context("no constant buffer")?,
            target: None,
        })
    }

    fn draw(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext, frame: &ID3D11Texture2D, (fw, fh): (u32, u32), shape: &Shape, at: POINT) -> Result<()> {
        let key = frame.as_raw() as usize;
        if self.target.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut view = None;
            unsafe { device.CreateRenderTargetView(frame, None, Some(&mut view))? };
            self.target = Some((key, view.context("no render target")?));
        }
        let (_, target) = self.target.as_ref().expect("set above");
        // The cursor's rectangle in clip space (y up).
        let (fw, fh) = (fw as f32, fh as f32);
        let rect = [
            at.x as f32 / fw * 2.0 - 1.0,
            1.0 - at.y as f32 / fh * 2.0,
            (at.x as f32 + shape.width as f32) / fw * 2.0 - 1.0,
            1.0 - (at.y as f32 + shape.height as f32) / fh * 2.0,
        ];
        unsafe {
            context.UpdateSubresource(&self.place, 0, None, rect.as_ptr() as *const c_void, 0, 0);
            context.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: fw, Height: fh, MinDepth: 0.0, MaxDepth: 1.0 }]));
            context.OMSetBlendState(&self.blend, Some(&[0.0; 4]), u32::MAX);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.VSSetShader(&self.vertex, None);
            context.VSSetConstantBuffers(0, Some(&[Some(self.place.clone())]));
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[Some(shape.view.clone())]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.Draw(4, 0);
            // Leave nothing bound that the video converter reads next.
            context.OMSetRenderTargets(None, None);
            context.PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }
}
