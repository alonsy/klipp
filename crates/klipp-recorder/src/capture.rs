//! Monitor capture through Windows.Graphics.Capture (no injection into games).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Result;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IInspectable, Interface, Ref, factory};

use crate::gpu::{Gpu, create_bgra_texture};

/// Either capture method: both keep the newest frame in a `LatestFrame`.
pub enum Capture {
    Wgc(MonitorCapture),
    Duplication(crate::duplication::DuplicationCapture),
}

impl Capture {
    pub fn latest(&self) -> &Arc<LatestFrame> {
        match self {
            Capture::Wgc(c) => &c.latest,
            Capture::Duplication(c) => &c.latest,
        }
    }
}

const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

/// Our copy of the newest captured frame. Replaced (with a new size) when the
/// display mode changes, e.g. a game switching to 4:3 in exclusive fullscreen.
pub struct Latest {
    pub texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    /// Increases every time the texture is replaced with a different size.
    pub size_generation: u64,
}

/// The most recent captured frame, copied into a texture we own so Windows can
/// reuse its capture buffer immediately.
pub struct LatestFrame {
    latest: Mutex<Latest>,
    generation: AtomicU64,
    resizes: AtomicU64,
    /// Windows ended the capture (its monitor went away); no more frames will come.
    closed: AtomicBool,
}

// SAFETY: D3D11 resources are free-threaded; all GPU commands go through the
// multithread-protected immediate context, and the texture is behind a mutex.
unsafe impl Send for LatestFrame {}
unsafe impl Sync for LatestFrame {}

impl LatestFrame {
    pub fn new(device: &ID3D11Device, width: u32, height: u32) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            latest: Mutex::new(Latest { texture: create_bgra_texture(device, width, height)?, width, height, size_generation: 0 }),
            generation: AtomicU64::new(0),
            resizes: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }))
    }

    pub fn set_closed(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// A new frame is in the texture.
    pub fn publish(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// The capture size changed: a new texture of the new size.
    pub fn resize(&self, latest: &mut Latest, device: &ID3D11Device, width: u32, height: u32) -> Result<()> {
        let texture = create_bgra_texture(device, width, height)?;
        *latest = Latest { texture, width, height, size_generation: latest.size_generation + 1 };
        self.resizes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Increases every time a new frame lands in the texture. 0 = nothing captured yet.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// How often the capture size changed.
    pub fn resizes(&self) -> u64 {
        self.resizes.load(Ordering::Relaxed)
    }

    /// Hold this while reading the texture so it can't be swapped mid-use.
    pub fn lock(&self) -> MutexGuard<'_, Latest> {
        self.latest.lock().unwrap()
    }
}

/// D3D11 objects used from the capture callback thread.
struct CallbackGpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt_device: IDirect3DDevice,
}
// SAFETY: see LatestFrame; the device is free-threaded and the context is multithread-protected.
unsafe impl Send for CallbackGpu {}

pub struct MonitorCapture {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    /// Kept so its Closed event can arrive.
    _item: GraphicsCaptureItem,
    pub latest: Arc<LatestFrame>,
}

impl MonitorCapture {
    pub fn start(gpu: &Gpu, monitor: HMONITOR, max_fps: u32) -> Result<Self> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let size = item.Size()?;
        let (width, height) = (size.Width as u32, size.Height as u32);

        let latest = LatestFrame::new(&gpu.device, width, height)?;

        let dxgi: IDXGIDevice = gpu.device.cast()?;
        let winrt_device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt_device, FORMAT, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        // Both are newer Windows 11 features; capture still works without them.
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetMinUpdateInterval(TimeSpan::from(Duration::from_secs_f64(1.0 / max_fps as f64)));
        session.SetIsCursorCaptureEnabled(true)?;

        let cb = CallbackGpu { device: gpu.device.clone(), context: gpu.context.clone(), winrt_device };
        let shared = latest.clone();
        pool.FrameArrived(&TypedEventHandler::new(
            move |pool: Ref<Direct3D11CaptureFramePool>, _: Ref<IInspectable>| {
                // Use the wrapper as a whole so the closure captures it (and its
                // Send impl) rather than its individual fields.
                let cb = &cb;
                let pool = pool.ok()?;
                let frame = pool.TryGetNextFrame()?;
                let content = frame.ContentSize()?;
                let (w, h) = (content.Width as u32, content.Height as u32);
                let mut latest = shared.lock();
                if (w, h) != (latest.width, latest.height) {
                    // The display mode changed. This frame still has the old
                    // size, so skip it, resize the pool, and replace our copy.
                    frame.Close()?;
                    pool.Recreate(&cb.winrt_device, FORMAT, 2, content)?;
                    let _ = shared.resize(&mut latest, &cb.device, w, h);
                    return Ok(());
                }
                let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
                let source: ID3D11Texture2D = unsafe { access.GetInterface()? };
                unsafe { cb.context.CopyResource(&latest.texture, &source) };
                drop(latest);
                shared.publish();
                frame.Close()
            },
        ))?;
        // The monitor went away (asleep, switched off, unplugged): the video
        // thread starts a new capture (see `revive`).
        let closed = latest.clone();
        item.Closed(&TypedEventHandler::new(move |_: Ref<GraphicsCaptureItem>, _: Ref<IInspectable>| {
            closed.set_closed();
            Ok(())
        }))?;
        session.StartCapture()?;

        Ok(Self { session, pool, _item: item, latest })
    }
}

impl Drop for MonitorCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
