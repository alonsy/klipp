//! Parsing hotkeys like "F8" or "Ctrl+Shift+S" for `RegisterHotKey`; telling
//! the Klipp window whether the save key works; testing a key for setup.

use std::sync::Mutex;

use anyhow::{Result, bail};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, KillTimer, MSG, SetTimer, WM_HOTKEY, WM_TIMER};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

impl Hotkey {
    pub fn parse(text: &str) -> Result<Self> {
        let mut modifiers = HOT_KEY_MODIFIERS(0);
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= MOD_CONTROL,
                "alt" => modifiers |= MOD_ALT,
                "shift" => modifiers |= MOD_SHIFT,
                "win" | "windows" => modifiers |= MOD_WIN,
                other if key.is_none() => key = Some(vk(other).ok_or_else(|| anyhow::anyhow!("unknown key \"{part}\""))?),
                _ => bail!("\"{text}\" has more than one key"),
            }
        }
        let Some(vk) = key else { bail!("\"{text}\" has no key, only modifiers") };
        Ok(Self { modifiers, vk })
    }
}

fn vk(name: &str) -> Option<u32> {
    let code = match name {
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.as_bytes()[0].to_ascii_uppercase() as u16,
        k if k.starts_with('f') && k[1..].parse::<u16>().is_ok_and(|n| (1..=24).contains(&n)) => {
            VK_F1.0 + k[1..].parse::<u16>().unwrap() - 1
        }
        k if k.starts_with("numpad") && k[6..].parse::<u16>().is_ok_and(|n| n <= 9) => {
            VK_NUMPAD0.0 + k[6..].parse::<u16>().unwrap()
        }
        "printscreen" | "prtsc" => VK_SNAPSHOT.0,
        "pause" => VK_PAUSE.0,
        "scrolllock" => VK_SCROLL.0,
        "insert" | "ins" => VK_INSERT.0,
        "delete" | "del" => VK_DELETE.0,
        "home" => VK_HOME.0,
        "end" => VK_END.0,
        "pageup" | "pgup" => VK_PRIOR.0,
        "pagedown" | "pgdn" => VK_NEXT.0,
        "space" => VK_SPACE.0,
        _ => return None,
    };
    Some(code as u32)
}

/// What `hotkey.json` last said, so it's only written when that changes.
static WRITTEN: Mutex<Option<(String, bool)>> = Mutex::new(None);

/// `hotkey.json` next to the log: whether the save key works or another app
/// holds it. The Klipp window shows a taken key in its sidebar.
pub fn write_state(hotkey: &str, works: bool) {
    // Tests don't touch the real Klipp folder.
    if cfg!(test) {
        return;
    }
    let mut written = WRITTEN.lock().unwrap();
    if written.as_ref().is_some_and(|(k, w)| k == hotkey && *w == works) {
        return;
    }
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Klipp")) else { return };
    let path = dir.join("hotkey.json");
    let part = path.with_extension("json.part");
    let body = serde_json::json!({ "hotkey": hotkey, "works": works }).to_string();
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&part, body).is_ok() && std::fs::rename(&part, &path).is_ok() {
        *written = Some((hotkey.to_string(), works));
    }
}

/// `--test-hotkey KEY SECONDS` (setup's Test button): holds the key for up to
/// `seconds` and waits for one press. "works", "taken" (another app holds
/// it), "nothing" (no press arrived: on many laptops the F keys need Fn) or
/// "invalid". Nothing is saved.
pub fn test(text: &str, seconds: u32) -> &'static str {
    const ID: i32 = 2;
    let Ok(h) = Hotkey::parse(text) else { return "invalid" };
    if unsafe { RegisterHotKey(None, ID, h.modifiers | MOD_NOREPEAT, h.vk) }.is_err() {
        return "taken";
    }
    let timer = unsafe { SetTimer(None, 0, seconds * 1000, None) };
    let mut result = "nothing";
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        match msg.message {
            WM_HOTKEY if msg.wParam.0 == ID as usize => {
                result = "works";
                break;
            }
            WM_TIMER if msg.wParam.0 == timer => break,
            _ => {}
        }
    }
    unsafe {
        let _ = KillTimer(None, timer);
        let _ = UnregisterHotKey(None, ID);
    }
    result
}
