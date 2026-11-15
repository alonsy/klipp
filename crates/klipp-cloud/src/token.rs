//! This PC's sign-in, kept in Windows Credential Manager (Control Panel →
//! Credential Manager → Windows Credentials → "Klipp:https://klipp.dev").
//! Windows encrypts it for the signed-in Windows user; it isn't in any file
//! of Klipp's, so copying Klipp's folders or sending logs can't leak it.

use std::ffi::c_void;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW};
use windows::core::{HSTRING, PWSTR};

/// A device token as klipp.dev hands them out: 20 to 100 URL-safe characters.
pub fn looks_right(token: &str) -> bool {
    (20..=100).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One entry per site, so a token is only ever read for the site that issued it.
fn target(origin: &str) -> String {
    format!("Klipp:{origin}")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Saves the token for `origin` (e.g. `https://klipp.dev`), replacing an older one.
pub fn save(origin: &str, token: &str) -> Result<()> {
    if !looks_right(token) {
        bail!("that isn't a sign-in token");
    }
    let (mut name, mut user, mut comment) = (wide(&target(origin)), wide("this PC"), wide("Klipp's sign-in to your account on this PC. Remove it to sign Klipp out."));
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        Comment: PWSTR(comment.as_mut_ptr()),
        CredentialBlobSize: token.len() as u32,
        CredentialBlob: token.as_ptr() as *mut u8,
        // Stays on this PC: it doesn't roam with a domain profile.
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }.context("saving the sign-in in Windows Credential Manager")
}

/// The saved token for `origin`, if there is one.
pub fn load(origin: &str) -> Option<String> {
    let mut found: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(&HSTRING::from(target(origin)), CRED_TYPE_GENERIC, None, &mut found).ok()?;
        let credential = &*found;
        let bytes = if credential.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(credential.CredentialBlob, credential.CredentialBlobSize as usize).to_vec()
        };
        CredFree(found as *const c_void);
        String::from_utf8(bytes).ok().filter(|token| looks_right(token))
    }
}

/// Removes the saved token for `origin`. Fine when there is none.
pub fn forget(origin: &str) -> Result<()> {
    match unsafe { CredDeleteW(&HSTRING::from(target(origin)), CRED_TYPE_GENERIC, None) } {
        Err(e) if e.code() != ERROR_NOT_FOUND.to_hresult() => Err(e).context("removing the sign-in from Windows Credential Manager"),
        _ => Ok(()),
    }
}
