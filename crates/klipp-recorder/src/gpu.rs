//! Direct3D 11 device setup and GPU color conversion (BGRA → NV12, with scaling).

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use anyhow::{Context, Result, bail};
use klipp_media::Vendor;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::core::Interface;

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub adapter_name: String,
    /// Whose encoder it has; `None` for cards Klipp can't encode on.
    pub vendor: Option<Vendor>,
}

impl Gpu {
    /// Creates a device on the GPU that drives `monitor`, so captured frames never cross adapters.
    pub fn for_monitor(monitor: HMONITOR) -> Result<Self> {
        Self::open(&adapter_for_monitor(monitor)?)
    }

    /// The GPU whose name contains `name` (the `video.gpu` setting, for
    /// testing a card that doesn't drive the screen).
    pub fn by_name(name: &str) -> Result<Self> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        for i in 0.. {
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
            if adapter_name(&adapter)?.to_lowercase().contains(&name.to_lowercase()) {
                return Self::open(&adapter);
            }
        }
        bail!("no graphics card named like \"{name}\"")
    }

    fn open(adapter: &IDXGIAdapter1) -> Result<Self> {
        let adapter_name = adapter_name(adapter)?;
        let vendor = Vendor::from_pci_id(unsafe { adapter.GetDesc1()? }.VendorId);

        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.context("D3D11CreateDevice returned no device")?;
        let context = context.context("D3D11CreateDevice returned no context")?;

        // Capture callbacks, our converter and FFmpeg all use the immediate
        // context from different threads; this makes D3D serialize them.
        let mt: ID3D11Multithread = device.cast()?;
        unsafe {
            let _ = mt.SetMultithreadProtected(true);
        }
        Ok(Self { device, context, adapter_name, vendor })
    }
}

fn adapter_name(adapter: &IDXGIAdapter1) -> Result<String> {
    let desc = unsafe { adapter.GetDesc1()? };
    let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
    Ok(String::from_utf16_lossy(&desc.Description[..len]))
}

/// A BGRA texture that both the capture copy and the video processor can use.
pub fn create_bgra_texture(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    texture.context("CreateTexture2D returned no texture")
}

fn adapter_for_monitor(monitor: HMONITOR) -> Result<IDXGIAdapter1> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        for j in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(j) }) else { break };
            if unsafe { output.GetDesc()? }.Monitor == monitor {
                return Ok(adapter);
            }
        }
    }
    bail!("no GPU drives the selected monitor")
}

/// Converts the captured BGRA frame into the encoder's NV12 textures using the
/// GPU's fixed-function video processor. It scales too, so lower output
/// resolutions cost nothing extra.
pub struct Converter {
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
    /// Output views per (texture, array slice). The encoder's pool reuses a
    /// small, fixed set of textures, so this stays bounded.
    outputs: HashMap<(usize, u32), ID3D11VideoProcessorOutputView>,
}

impl Converter {
    pub fn new(gpu: &Gpu, source: &ID3D11Texture2D, src: (u32, u32), dst: (u32, u32), fps: u32) -> Result<Self> {
        let video_device: ID3D11VideoDevice = gpu.device.cast()?;
        let video_context: ID3D11VideoContext = gpu.context.cast()?;
        let rate = DXGI_RATIONAL { Numerator: fps, Denominator: 1 };
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: src.0,
            InputHeight: src.1,
            OutputFrameRate: rate,
            OutputWidth: dst.0,
            OutputHeight: dst.1,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        unsafe {
            let enumerator = video_device.CreateVideoProcessorEnumerator(&content)?;
            let processor = video_device.CreateVideoProcessor(&enumerator, 0)?;

            // Desktop/game pixels are full-range sRGB; video players expect BT.709 limited range.
            let vc1: ID3D11VideoContext1 = video_context.cast()?;
            vc1.VideoProcessorSetStreamColorSpace1(&processor, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
            vc1.VideoProcessorSetOutputColorSpace1(&processor, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709);
            video_context.VideoProcessorSetStreamFrameFormat(&processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);

            let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 },
                },
            };
            let mut input = None;
            video_device.CreateVideoProcessorInputView(source, &enumerator, &input_desc, Some(&mut input))?;
            let input = input.context("CreateVideoProcessorInputView returned no view")?;

            Ok(Self { video_device, video_context, enumerator, processor, input, outputs: HashMap::new() })
        }
    }

    /// Renders the current source frame into `target` (array slice `slice`).
    pub fn convert(&mut self, target: &ID3D11Texture2D, slice: u32) -> Result<()> {
        let key = (target.as_raw() as usize, slice);
        if !self.outputs.contains_key(&key) {
            let view = self.create_output_view(target, slice)?;
            self.outputs.insert(key, view);
        }
        let output = &self.outputs[&key];

        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(self.input.clone())),
            ..Default::default()
        };
        let streams = [stream];
        let result = unsafe { self.video_context.VideoProcessorBlt(&self.processor, output, 0, &streams) };
        // The stream struct holds its own reference to the input view; release it.
        let [mut stream] = streams;
        unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
        result?;
        Ok(())
    }

    fn create_output_view(&self, target: &ID3D11Texture2D, slice: u32) -> Result<ID3D11VideoProcessorOutputView> {
        let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { target.GetDesc(&mut tex_desc) };
        let desc = if tex_desc.ArraySize > 1 {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV { MipSlice: 0, FirstArraySlice: slice, ArraySize: 1 },
                },
            }
        } else {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
            }
        };
        let mut view = None;
        unsafe { self.video_device.CreateVideoProcessorOutputView(target, &self.enumerator, &desc, Some(&mut view))? };
        view.context("CreateVideoProcessorOutputView returned no view")
    }
}
