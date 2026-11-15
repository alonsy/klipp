//! HTTPS through WinHTTP: Windows' own TLS, certificate store and proxy
//! settings, with nothing extra to ship. Requests never follow redirects and
//! never keep cookies, so what Klipp sends goes only where it was addressed.

use std::ffi::c_void;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail};
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_COOKIES, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_DISABLE_FEATURE,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_QUERY_CUSTOM, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, WinHttpWriteData,
};
use windows::core::{HSTRING, PCWSTR, w};

/// The most an answer may weigh: these are short JSON messages.
const MAX_ANSWER: usize = 1 << 20;
/// How much of a file goes out per write, which is also how often progress is reported.
const WRITE_CHUNK: usize = 256 * 1024;

/// An address split into what WinHTTP asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// The path with its query, starting with `/`.
    pub path: String,
}

impl Url {
    pub fn parse(text: &str) -> Result<Self> {
        let (secure, rest) = if let Some(rest) = text.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix("http://") {
            (false, rest)
        } else {
            bail!("the address must start with https://");
        };
        let rest = rest.split('#').next().unwrap_or_default();
        let (authority, path) = match rest.find(['/', '?']) {
            Some(at) => rest.split_at(at),
            None => (rest, ""),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().ok().filter(|p| *p != 0).context("the address has a bad port")?),
            None => (authority, if secure { 443 } else { 80 }),
        };
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
            bail!("the address has a bad host name");
        }
        if path.chars().any(|c| c.is_control() || c == ' ') {
            bail!("the address has a bad path");
        }
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        Ok(Self { secure, host: host.to_ascii_lowercase(), port, path })
    }

    /// Where `reference` points: a full address as it stands, or a path on this site.
    pub fn join(&self, reference: &str) -> Result<Self> {
        if reference.starts_with('/') && !reference.starts_with("//") {
            let mut url = Self::parse(&format!("{}{reference}", self.origin()))?;
            url.port = self.port;
            return Ok(url);
        }
        Self::parse(reference)
    }

    /// `https://host`, with the port only when it isn't the usual one.
    pub fn origin(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        let usual = if self.secure { 443 } else { 80 };
        if self.port == usual { format!("{scheme}://{}", self.host) } else { format!("{scheme}://{}:{}", self.host, self.port) }
    }

    /// This PC itself, the only place Klipp will talk to without encryption (a local copy of the site, for testing).
    pub fn is_this_pc(&self) -> bool {
        self.host == "localhost" || self.host == "127.0.0.1"
    }
}

/// What a request carries.
pub enum Body<'a> {
    None,
    Bytes(&'a [u8]),
    /// `len` bytes of a file from `offset`, streamed.
    File { file: &'a mut File, offset: u64, len: u64 },
}

pub struct Response {
    pub status: u32,
    pub body: Vec<u8>,
    /// The answer's `ETag` header (storage returns one for every uploaded part).
    pub etag: Option<String>,
}

/// The request was stopped by its caller, not by a failure.
#[derive(Debug)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

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

// SAFETY: a WinHTTP handle isn't tied to the thread that opened it; WinHTTP lets any
// thread use and close it. (Not Sync: one thread at a time uses a session here.)
unsafe impl Send for Handle {}

/// One WinHTTP session: requests through it share connections, so an upload's parts don't each start from a new handshake.
pub struct Session(Handle);

impl Session {
    /// `agent` is what the site sees as the caller, e.g. `Klipp/0.4.0`.
    pub fn new(agent: &str) -> Result<Self> {
        unsafe {
            let session = Handle::new(
                WinHttpOpen(&HSTRING::from(agent), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
                "opening WinHTTP",
            )?;
            // Finding and reaching the site: 15 s each. A write or a wait for the answer: 90 s (finishing an upload makes the site check the file).
            WinHttpSetTimeouts(session.0, 15_000, 15_000, 90_000, 90_000)?;
            Ok(Self(session))
        }
    }

    /// Sends one request and reads the whole answer. `sent` hears how many body
    /// bytes have gone out so far and stops the request by returning false.
    pub fn send(&self, method: &str, url: &Url, headers: &[(&str, &str)], body: Body, sent: &mut dyn FnMut(u64) -> bool) -> Result<Response> {
        if !url.secure && !url.is_this_pc() {
            bail!("refusing to talk to {} without encryption", url.host);
        }
        let mut all = String::new();
        for (name, value) in headers {
            if name.is_empty() || name.chars().any(|c| !c.is_ascii_graphic() || c == ':') || value.chars().any(|c| c.is_control()) {
                bail!("a request header isn't valid");
            }
            all.push_str(&format!("{name}: {value}\r\n"));
        }
        let all: Vec<u16> = all.encode_utf16().collect();
        let all = (!all.is_empty()).then_some(all.as_slice());
        unsafe {
            let connect = Handle::new(WinHttpConnect(self.0.0, &HSTRING::from(url.host.as_str()), url.port, 0), "connecting")?;
            let flags = if url.secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
            let request = Handle::new(
                WinHttpOpenRequest(connect.0, &HSTRING::from(method), &HSTRING::from(url.path.as_str()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags),
                "opening the request",
            )?;
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_REDIRECT_POLICY, Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes()))?;
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_COOKIES.to_ne_bytes()))?;

            match body {
                Body::None => WinHttpSendRequest(request.0, all, None, 0, 0, 0).context("sending the request")?,
                Body::Bytes(bytes) => {
                    let len = u32::try_from(bytes.len()).context("the request is too large")?;
                    WinHttpSendRequest(request.0, all, Some(bytes.as_ptr() as *const c_void), len, len, 0).context("sending the request")?;
                    if !sent(bytes.len() as u64) {
                        return Err(Cancelled.into());
                    }
                }
                Body::File { file, offset, len } => {
                    let total = u32::try_from(len).context("one piece of an upload must be under 4 GB")?;
                    file.seek(SeekFrom::Start(offset)).context("reading the file")?;
                    WinHttpSendRequest(request.0, all, None, 0, total, 0).context("sending the request")?;
                    let mut chunk = vec![0u8; WRITE_CHUNK];
                    let mut done = 0u64;
                    while done < len {
                        let want = chunk.len().min((len - done) as usize);
                        file.read_exact(&mut chunk[..want]).context("reading the file")?;
                        let mut at = 0usize;
                        while at < want {
                            let mut wrote = 0u32;
                            WinHttpWriteData(request.0, Some(chunk[at..].as_ptr() as *const c_void), (want - at) as u32, &mut wrote).context("sending the file")?;
                            if wrote == 0 {
                                bail!("sending the file: the connection took nothing more");
                            }
                            at += wrote as usize;
                        }
                        done += want as u64;
                        if !sent(done) {
                            return Err(Cancelled.into());
                        }
                    }
                }
            }
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
            let mut name = [0u16; 512];
            let mut bytes = (name.len() * 2) as u32;
            let etag = WinHttpQueryHeaders(request.0, WINHTTP_QUERY_CUSTOM, w!("ETag"), Some(name.as_mut_ptr() as *mut c_void), &mut bytes, std::ptr::null_mut())
                .ok()
                .map(|()| String::from_utf16_lossy(&name[..(bytes as usize / 2).min(name.len())]))
                .filter(|tag| !tag.is_empty());

            let mut answer = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read).context("reading the answer")?;
                if read == 0 {
                    break;
                }
                answer.extend_from_slice(&chunk[..read as usize]);
                if answer.len() > MAX_ANSWER {
                    bail!("the answer is too large");
                }
            }
            Ok(Response { status, body: answer, etag })
        }
    }
}
