//! Once a day, asks GitHub whether a newer Klipp is out. When there is one,
//! the tray menu offers it ("Update to Klipp 0.2.2"), which opens the window's
//! update card. Nothing pops up. The request goes through WinHTTP, which is
//! part of Windows, so this adds no library and next to no memory.

use std::ffi::c_void;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{PCWSTR, w};

use crate::log::{info, warning};

/// The update manifest the window's updater reads too (see tauri.conf.json).
const HOST: PCWSTR = w!("github.com");
const PATH: PCWSTR = w!("/alonsy/klipp/releases/latest/download/latest.json");
/// Not right at startup: Windows is busy logging in, and the network may not be up yet.
const FIRST_CHECK: Duration = Duration::from_secs(90);
const EVERY: Duration = Duration::from_secs(24 * 3600);

static AVAILABLE: Mutex<Option<String>> = Mutex::new(None);

/// A newer version than this one, once a check has found it.
pub fn available() -> Option<String> {
    AVAILABLE.lock().unwrap().clone()
}

pub fn start() {
    let spawned = thread::Builder::new().name("update-check".into()).spawn(|| {
        thread::sleep(FIRST_CHECK);
        loop {
            match latest_version() {
                Ok(latest) if is_newer(&latest, env!("CARGO_PKG_VERSION")) => {
                    let mut found = AVAILABLE.lock().unwrap();
                    if found.as_deref() != Some(latest.as_str()) {
                        info!("update: Klipp {latest} is available");
                    }
                    *found = Some(latest);
                }
                Ok(_) => {}
                Err(e) => warning!("update check: {e:#}"),
            }
            thread::sleep(EVERY);
        }
    });
    if let Err(e) = spawned {
        warning!("update check: {e}");
    }
}

/// The version in the latest release's manifest.
pub fn latest_version() -> Result<String> {
    let body = get(HOST, PATH)?;
    let json: Value = serde_json::from_slice(&body).context("latest.json isn't JSON")?;
    json["version"].as_str().map(str::to_string).context("latest.json has no version")
}

/// "0.2.10" is newer than "0.2.9"; anything after a "-" or "+" is ignored.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        let core = v.trim().trim_start_matches('v').split(['-', '+']).next().unwrap_or_default();
        core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parse(candidate) > parse(current)
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self> {
        if raw.is_null() {
            bail!("{what}: {}", windows::core::Error::from_thread());
        }
        Ok(Self(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// An HTTPS GET; redirects (GitHub's download links have one) are followed.
fn get(host: PCWSTR, path: PCWSTR) -> Result<Vec<u8>> {
    unsafe {
        let session = Handle::new(WinHttpOpen(w!("Klipp"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "opening WinHTTP")?;
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 15_000)?;
        let connect = Handle::new(WinHttpConnect(session.0, host, INTERNET_DEFAULT_HTTPS_PORT, 0), "connecting")?;
        let request = Handle::new(
            WinHttpOpenRequest(connect.0, w!("GET"), path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "opening the request",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).context("sending the request")?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).context("waiting for the answer")?;
        let (mut status, mut len) = (0u32, 4u32);
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            bail!("HTTP {status}");
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > 1 << 20 {
                bail!("the answer is too large");
            }
        }
        Ok(body)
    }
}
