//! Once a day, asks GitHub whether a newer Klipp is out. When there is one,
//! the tray menu offers it ("Update to Klipp 0.2.2"), which opens the window's
//! update card. Nothing pops up. The request goes through WinHTTP, which is
//! part of Windows, so this adds no library and next to no memory.
//!
//! On the test channel (see `channel`) the newest of the test builds and the
//! normal releases counts.

use std::cmp::Ordering;
use std::ffi::c_void;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{HSTRING, PCWSTR, w};

use crate::channel::{self, Channel};
use crate::log::{info, warning};

/// The update manifests the window's updater reads too (see tauri.conf.json):
/// the latest release's, and the test channel's (the newest test build).
const HOST: PCWSTR = w!("github.com");
const PATH: PCWSTR = w!("/alonsy/klipp/releases/latest/download/latest.json");
const TEST_PATH: PCWSTR = w!("/alonsy/klipp/releases/download/test-channel/latest.json");
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

/// The newest version this copy's channel offers.
pub fn latest_version() -> Result<String> {
    let release = manifest_version(PATH);
    if channel::read().channel == Channel::Production {
        return release;
    }
    match (release, manifest_version(TEST_PATH)) {
        (Ok(r), Ok(t)) => Ok(if is_newer(&t, &r) { t } else { r }),
        (Ok(v), Err(_)) | (Err(_), Ok(v)) => Ok(v),
        (Err(e), Err(_)) => Err(e),
    }
}

fn manifest_version(path: PCWSTR) -> Result<String> {
    let (status, body) = request(HOST, path, None)?;
    if status != 200 {
        bail!("HTTP {status}");
    }
    let json: Value = serde_json::from_slice(&body).context("latest.json isn't JSON")?;
    json["version"].as_str().map(str::to_string).context("latest.json has no version")
}

/// Whether `candidate` is a later version than `current`, the way semver
/// orders them: "0.2.10" after "0.2.9", "0.3.5-test.2" after "0.3.5-test.1",
/// and "0.3.5" after every "0.3.5-…". Anything after a "+" is ignored.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare(candidate, current) == Ordering::Greater
}

fn compare(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| {
        let v = v.trim().trim_start_matches('v').split('+').next().unwrap_or_default().to_string();
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c.to_string(), Some(p.to_string())),
            None => (v, None),
        };
        let mut numbers: Vec<u64> = core.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        numbers.resize(3.max(numbers.len()), 0);
        (numbers, pre)
    };
    let ((a_core, a_pre), (b_core, b_pre)) = (parse(a), parse(b));
    a_core.cmp(&b_core).then_with(|| match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        // A release comes after its own pre-releases.
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (xs, ys): (Vec<&str>, Vec<&str>) = (x.split('.').collect(), y.split('.').collect());
            for (p, q) in xs.iter().zip(&ys) {
                let order = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => p.cmp(q),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            xs.len().cmp(&ys.len())
        }
    })
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

/// An HTTPS GET, or a POST of JSON when there's a body; redirects (GitHub's
/// download links have one) are followed. The status and the answer.
pub fn request(host: PCWSTR, path: PCWSTR, json: Option<&[u8]>) -> Result<(u32, Vec<u8>)> {
    unsafe {
        let agent = HSTRING::from(format!("Klipp/{}", env!("CARGO_PKG_VERSION")));
        let session = Handle::new(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "opening WinHTTP")?;
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 30_000)?;
        let connect = Handle::new(WinHttpConnect(session.0, host, INTERNET_DEFAULT_HTTPS_PORT, 0), "connecting")?;
        let verb = if json.is_some() { w!("POST") } else { w!("GET") };
        let request = Handle::new(
            WinHttpOpenRequest(connect.0, verb, path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "opening the request",
        )?;
        match json {
            Some(body) => {
                let headers: Vec<u16> = "Content-Type: application/json\r\n".encode_utf16().collect();
                WinHttpSendRequest(request.0, Some(&headers), Some(body.as_ptr() as *const c_void), body.len() as u32, body.len() as u32, 0)
            }
            None => WinHttpSendRequest(request.0, None, None, 0, 0, 0),
        }
        .context("sending the request")?;
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
        Ok((status, body))
    }
}
