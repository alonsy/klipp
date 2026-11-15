//! klipp.dev's API for the app (`/api/v1`): linking this PC to an account,
//! who that account is, and the steps of an upload. Every call that acts for
//! the account carries the device token, and only to the site itself: the
//! storage addresses an upload gets never see it.

use std::fmt;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::http::{Body, Response, Session, Url};
use crate::token;

/// What the site said no to, in words the person can act on.
#[derive(Debug)]
pub enum ApiError {
    /// The token no longer works: signed out on the website, expired, or the account is gone.
    SignedOut,
    /// The site refused the request and said why (a full quota, a plan that doesn't include it).
    Refused(String),
    /// The site looked at an uploaded file and didn't take it (not a video it plays, over the plan's
    /// resolution or length). About that one file; sending it again would end the same way.
    Rejected(String),
    /// Too many requests in a while; the same request works again later.
    TooMany(String),
    /// The site or something on the way failed.
    Site(u32),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("Klipp isn't signed in on this PC anymore. Sign in again."),
            Self::Refused(why) | Self::Rejected(why) | Self::TooMany(why) => f.write_str(why),
            Self::Site(status) => write!(f, "klipp.dev answered with an error ({status}). Try again in a moment."),
        }
    }
}

impl std::error::Error for ApiError {}

/// The code to show while the person approves this PC on the website.
#[derive(Debug, Clone, Deserialize)]
pub struct LinkStart {
    /// Kept by the app; whoever holds it can collect the token once approved.
    pub device_code: String,
    /// Shown to the person, e.g. `BCDF-GHJK`.
    pub user_code: String,
    pub verification_uri: String,
    /// The same page with the code filled in.
    pub verification_uri_complete: String,
    /// Seconds until the code stops working.
    pub expires_in: u64,
    /// Seconds to wait between asks.
    pub interval: u64,
}

#[derive(Debug)]
pub enum LinkPoll {
    /// Not approved yet: ask again after the interval.
    Pending,
    /// Asked too soon: wait longer between asks.
    SlowDown,
    Linked { token: String, handle: String },
    Denied,
    Expired,
}

/// What the account's plan allows; uploads are sized against it before they start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// `free`, `plus` or `pro`.
    pub id: String,
    /// Bytes of clips the account may keep, and how much is used.
    pub storage: u64,
    pub used: u64,
    /// Largest single clip, bytes.
    pub max_file: u64,
    /// Highest resolution, as the shorter side (1080 = 1080p).
    pub max_height: u32,
    pub max_fps: u32,
    pub max_seconds: u32,
    pub auto_backup: bool,
    /// Clips shared from this account start with the Klipp tab, burned into the video (Free).
    #[serde(default)]
    pub mark: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// The account itself, which stays the same when its handle changes.
    #[serde(default)]
    pub id: String,
    pub handle: String,
    pub name: String,
    pub avatar: Option<String>,
    /// The profile page's address.
    pub profile: String,
    /// `active`, or `restricted`, `suspended`, `banned`.
    pub status: String,
    pub plan: Plan,
    /// Which clips auto-backup takes: `off`, `favorites`, `auto` or `all`.
    pub backup: String,
    /// When auto-backup was turned on, in seconds since 1970: only clips saved after it go up.
    #[serde(default)]
    pub backup_since: Option<u64>,
}

/// What an upload says about its clip.
#[derive(Debug, Clone, Default)]
pub struct NewUpload {
    pub title: String,
    /// The game as Klipp names its folder ("Counter-Strike 2"); the site files it on the right page.
    pub game: Option<String>,
    /// Auto-clip moments, e.g. "2k", "headshot".
    pub tags: Vec<String>,
    pub map: Option<String>,
    /// `public`, `unlisted` or `private`.
    pub visibility: String,
    /// An automatic backup (paid plans; always private) instead of a clip the person chose to share.
    pub backup: bool,
    /// The Klipp tab is burned into the file, so the site's player doesn't show it again.
    pub marked: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartUrl {
    pub part_number: u32,
    pub url: String,
}

/// Where an upload's pieces go.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    pub clip_id: String,
    /// Bytes per piece; the last one is whatever is left.
    pub part_size: u64,
    pub parts: Vec<PartUrl>,
    pub thumb_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonePart {
    pub part_number: u32,
    pub etag: String,
}

/// A clip that is on the site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Uploaded {
    pub id: String,
    /// Its page, the link to share.
    pub url: String,
    pub visibility: String,
}

pub struct Client {
    base: Url,
    session: Session,
}

impl Client {
    /// `version` is this build's, for the site's logs (`Klipp/0.4.0`).
    pub fn new(base: Url, version: &str) -> Result<Self> {
        Ok(Self { base, session: Session::new(&format!("Klipp/{version}"))? })
    }

    /// The site this client talks to, e.g. `https://klipp.dev`; also the name its token is saved under.
    pub fn origin(&self) -> String {
        self.base.origin()
    }

    pub(crate) fn session(&self) -> &Session {
        &self.session
    }

    pub(crate) fn base(&self) -> &Url {
        &self.base
    }

    fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<&Value>) -> Result<Response> {
        let url = self.base.join(path)?;
        let bearer = match token {
            // A damaged token would only be refused; don't send it anywhere.
            Some(t) if !token::looks_right(t) => return Err(ApiError::SignedOut.into()),
            Some(t) => Some(format!("Bearer {t}")),
            None => None,
        };
        let mut headers = vec![("Accept", "application/json")];
        if let Some(bearer) = &bearer {
            headers.push(("Authorization", bearer.as_str()));
        }
        let bytes = body.map(serde_json::to_vec).transpose()?;
        if bytes.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        let body = match &bytes {
            Some(bytes) => Body::Bytes(bytes),
            None if method == "GET" => Body::None,
            // A POST without a body still says so.
            None => Body::Bytes(b""),
        };
        self.session.send(method, &url, &headers, body, &mut |_| true).with_context(|| format!("couldn't reach {}", self.base.host))
    }

    /// The answer as `T`, or the site's refusal as an `ApiError`.
    fn read<T: DeserializeOwned>(response: Response) -> Result<T> {
        if (200..300).contains(&response.status) {
            return serde_json::from_slice(&response.body).context("klipp.dev's answer wasn't what Klipp expected");
        }
        Err(refusal(&response).into())
    }

    /// Asks for a code to show. `device_name` is how this PC appears under the account's devices.
    pub fn link_start(&self, device_name: &str) -> Result<LinkStart> {
        Self::read(self.call("POST", "/api/v1/device/code", None, Some(&json!({ "device_name": device_name })))?)
    }

    /// Asks whether the code was approved yet.
    pub fn link_poll(&self, device_code: &str) -> Result<LinkPoll> {
        let response = self.call("POST", "/api/v1/device/token", None, Some(&json!({ "device_code": device_code })))?;
        let answer: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        if response.status == 200 {
            let token = answer["access_token"].as_str().unwrap_or_default();
            if !token::looks_right(token) {
                return Err(ApiError::Site(200).into());
            }
            return Ok(LinkPoll::Linked { token: token.to_string(), handle: answer["handle"].as_str().unwrap_or_default().to_string() });
        }
        Ok(match answer["error"].as_str() {
            Some("authorization_pending") => LinkPoll::Pending,
            Some("slow_down") => LinkPoll::SlowDown,
            Some("access_denied") => LinkPoll::Denied,
            Some("expired_token" | "invalid_grant") => LinkPoll::Expired,
            _ => return Err(refusal(&response).into()),
        })
    }

    /// Who the token belongs to and what their plan allows.
    pub fn account(&self, token: &str) -> Result<Account> {
        Self::read(self.call("GET", "/api/v1/me", Some(token), None)?)
    }

    /// Ends the token on the site. The caller then forgets it locally.
    pub fn sign_out(&self, token: &str) -> Result<()> {
        let response = self.call("POST", "/api/v1/device/revoke", Some(token), None)?;
        // Already ended there is as good as ended now.
        if (200..300).contains(&response.status) || response.status == 401 { Ok(()) } else { Err(refusal(&response).into()) }
    }

    pub(crate) fn start_upload(&self, token: &str, bytes: u64, thumb_bytes: Option<u64>, clip: &NewUpload) -> Result<Ticket> {
        let body = json!({
            "bytes": bytes,
            "thumbBytes": thumb_bytes,
            "title": clip.title,
            "game": clip.game,
            "tags": clip.tags,
            "map": clip.map,
            "visibility": clip.visibility,
            "source": if clip.backup { "backup" } else { "app" },
            "marked": clip.marked,
        });
        Self::read(self.call("POST", "/api/v1/uploads", Some(token), Some(&body))?)
    }

    /// Fresh addresses for some pieces of an upload in progress (the first ones stop working after a few hours).
    pub(crate) fn part_urls(&self, token: &str, clip_id: &str, parts: &[u32]) -> Result<Vec<PartUrl>> {
        #[derive(Deserialize)]
        struct Answer {
            parts: Vec<PartUrl>,
        }
        let answer: Answer = Self::read(self.call("POST", &format!("/api/v1/uploads/{}/parts", id(clip_id)?), Some(token), Some(&json!({ "parts": parts })))?)?;
        Ok(answer.parts)
    }

    /// Tells the site every piece is up. It then checks the file really is a video the plan allows, and publishes it.
    pub(crate) fn finish_upload(&self, token: &str, clip_id: &str, parts: &[DonePart]) -> Result<Uploaded> {
        let response = self.call("POST", &format!("/api/v1/uploads/{}/complete", id(clip_id)?), Some(token), Some(&json!({ "parts": parts })))?;
        Self::read(response).map_err(|error| match error.downcast::<ApiError>() {
            // A no at this point is about the file that was just sent.
            Ok(ApiError::Refused(why)) => ApiError::Rejected(why).into(),
            Ok(other) => other.into(),
            Err(other) => other,
        })
    }

    /// Deletes one of the account's clips, or drops an upload that is still in progress.
    pub fn delete_clip(&self, token: &str, clip_id: &str) -> Result<()> {
        let response = self.call("DELETE", &format!("/api/v1/clips/{}", id(clip_id)?), Some(token), None)?;
        if (200..300).contains(&response.status) { Ok(()) } else { Err(refusal(&response).into()) }
    }
}

/// A clip id is ten letters and digits; nothing else goes into a path.
fn id(clip_id: &str) -> Result<&str> {
    if clip_id.len() == 10 && clip_id.bytes().all(|b| b.is_ascii_alphanumeric()) { Ok(clip_id) } else { Err(ApiError::Site(0)).context("klipp.dev sent a clip id Klipp doesn't recognise") }
}

/// Why the site said no: its own words for a 4xx (`{"error": "..."}`), never a page of HTML.
fn refusal(response: &Response) -> ApiError {
    if response.status == 401 {
        return ApiError::SignedOut;
    }
    let said = serde_json::from_slice::<Value>(&response.body).ok().and_then(|v| v["error"].as_str().map(str::to_string));
    let said = said.map(|why| why.chars().filter(|c| !c.is_control()).take(300).collect::<String>()).filter(|why| !why.is_empty());
    match said {
        _ if response.status == 429 => ApiError::TooMany(said.unwrap_or_else(|| "That's a lot at once. Try again in a little while.".into())),
        Some(why) if (400..500).contains(&response.status) => ApiError::Refused(why),
        _ => ApiError::Site(response.status),
    }
}
