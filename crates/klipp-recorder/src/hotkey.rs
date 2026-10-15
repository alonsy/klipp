//! Parsing hotkeys like "Alt+F10" or "Ctrl+Shift+S" for `RegisterHotKey`.

use anyhow::{Result, bail};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

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
