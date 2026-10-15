//! "Everything this PC plays, except Klipp": Windows' process loopback in
//! exclude mode (Windows 10 2004 and newer). Recording PC audio this way
//! leaves out the save sound, which the recorder plays itself, whatever
//! device it plays on.
//!
//! It isn't used with audio routers (SteelSeries Sonar, Voicemeeter, virtual
//! cables): their own process plays the game's sound again on the real
//! device, so it would be recorded twice.

use std::mem::size_of;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use anyhow::{Context, Result};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::BLOB;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, Ref, implement};

/// Device names of audio routers that replay other programs' sound. Not
/// "virtual" alone: NVIDIA's Virtual Audio Device (GameStream) is on many
/// PCs and replays nothing.
const ROUTERS: &[&str] = &["sonar", "voicemeeter", "vb-audio", "cable", "wave link", "broadcast", "oculus"];

/// True if one of the PC's playback devices belongs to an audio router.
pub fn router_present(device_names: &[String]) -> bool {
    device_names.iter().any(|n| {
        let n = n.to_lowercase();
        ROUTERS.iter().any(|r| n.contains(r))
    })
}

#[implement(IActivateAudioInterfaceCompletionHandler, windows::Win32::System::Com::IAgileObject)]
struct Activated(Mutex<Sender<()>>);

// Agile: Windows may call it back on any thread.
impl windows::Win32::System::Com::IAgileObject_Impl for Activated_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.lock().unwrap().send(());
        Ok(())
    }
}

/// An audio client for everything played on this PC except by this process
/// (and anything it starts). Not yet initialized.
pub fn activate_excluding_self() -> Result<IAudioClient> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // Not dropped: dropping a PROPVARIANT frees its blob, and this blob is `params` on the stack.
    let mut variant = std::mem::ManuallyDrop::new(PROPVARIANT::default());
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB { cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: &params as *const _ as *mut u8 };
    }
    let (tx, rx) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(Mutex::new(tx)).into();
    let operation = unsafe { ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*variant), &handler)? };
    rx.recv_timeout(Duration::from_secs(5)).context("Windows didn't answer the process loopback request")?;
    let (mut result, mut client): (HRESULT, Option<IUnknown>) = (HRESULT(0), None);
    unsafe { operation.GetActivateResult(&mut result, &mut client)? };
    result.ok().context("process loopback isn't available (it needs Windows 10 2004 or newer)")?;
    Ok(client.context("no audio client")?.cast()?)
}
