//! Putting a file on the clipboard, so Ctrl+V in a chat app (or Explorer) pastes it.

use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Result, bail};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, POINT};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::DROPFILES;

pub fn copy_file(path: &Path) -> Result<()> {
    // CF_HDROP data: a DROPFILES header followed by the path, double-null terminated.
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let header = DROPFILES { pFiles: size_of::<DROPFILES>() as u32, pt: POINT::default(), fNC: false.into(), fWide: true.into() };
    let total = size_of::<DROPFILES>() + wide.len() * 2;
    unsafe {
        let memory: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, total)?;
        let ptr = GlobalLock(memory) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(memory));
            bail!("couldn't lock clipboard memory");
        }
        std::ptr::copy_nonoverlapping(&header as *const DROPFILES as *const u8, ptr, size_of::<DROPFILES>());
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(size_of::<DROPFILES>()), wide.len() * 2);
        let _ = GlobalUnlock(memory);

        if OpenClipboard(None).is_err() {
            let _ = GlobalFree(Some(memory));
            bail!("the clipboard is in use by another app");
        }
        let _ = EmptyClipboard();
        // On success the clipboard owns the memory.
        let result = SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(memory.0)));
        let _ = CloseClipboard();
        if result.is_err() {
            let _ = GlobalFree(Some(memory));
            bail!("couldn't put the file on the clipboard");
        }
    }
    Ok(())
}
