//! Klipp's link to an account on klipp.dev.
//!
//! Klipp works fully without an account. With one, this PC is linked by a
//! code the person approves on the website ([`Client::link_start`]), the token
//! that comes back is kept in Windows Credential Manager ([`token`]), clips
//! can be uploaded for a link to share ([`upload`]), and on a paid plan the
//! recorder backs clips up by itself ([`backup`]).
//!
//! Everything here is plain blocking calls for a worker thread; nothing runs
//! unless the window or the recorder asks for it.

pub mod api;
pub mod backup;
pub mod http;
pub mod token;
mod upload;

pub use api::{Account, ApiError, Client, LinkPoll, LinkStart, NewUpload, Plan, Uploaded};
pub use http::{Cancelled, Url};
pub use upload::{Progress, upload};

/// Where Klipp's accounts and shared clips live.
const SITE: &str = "https://klipp.dev";

/// The site this build talks to.
pub fn site() -> Url {
    Url::parse(SITE).expect("the site's address is valid")
}

/// How this PC appears under the account's devices: "Klipp on DESKTOP-ABC123".
pub fn device_name() -> String {
    let pc: String = std::env::var("COMPUTERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(40).collect();
    if pc.is_empty() { "Klipp on Windows".into() } else { format!("Klipp on {pc}") }
}
