//! Windows' multimedia scheduler (MMCSS): threads registered as "Capture" get
//! priority when the CPU is busy (e.g. a heavy game), so frames and audio
//! keep their timing.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW};
use windows::core::w;

/// Registers the current thread for as long as this value lives.
pub struct CaptureThread(Option<HANDLE>);

impl CaptureThread {
    pub fn register() -> Self {
        let mut task_index = 0;
        // If MMCSS is unavailable the thread just runs at normal priority.
        Self(unsafe { AvSetMmThreadCharacteristicsW(w!("Capture"), &mut task_index) }.ok())
    }
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            let _ = unsafe { AvRevertMmThreadCharacteristics(h) };
        }
    }
}
