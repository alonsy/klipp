//! Which game is in the foreground, for naming clip folders.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

/// Clips taken while no game was in the foreground go here.
pub const DESKTOP: &str = "Desktop";

/// Games whose install folder name isn't the name people know them by.
const KNOWN: &[(&str, &str)] = &[
    ("cs2.exe", "Counter-Strike 2"),
    ("valorant-win64-shipping.exe", "VALORANT"),
    ("league of legends.exe", "League of Legends"),
    ("dota2.exe", "Dota 2"),
    ("r5apex.exe", "Apex Legends"),
    ("r5apex_dx12.exe", "Apex Legends"),
    ("fortniteclient-win64-shipping.exe", "Fortnite"),
    ("overwatch.exe", "Overwatch 2"),
    ("rocketleague.exe", "Rocket League"),
    ("rainbowsix.exe", "Rainbow Six Siege"),
    ("rainbowsix_vulkan.exe", "Rainbow Six Siege"),
    ("gta5.exe", "Grand Theft Auto V"),
];

/// Launcher library folders: the folder after these components is the game's name.
const LIBRARIES: &[&[&str]] = &[
    &["steamapps", "common"],
    &["epic games"],
    &["riot games"],
    &["gog galaxy", "games"],
    &["gog games"],
    &["ea games"],
    &["ubisoft game launcher", "games"],
    &["xboxgames"],
];

/// Remembers the last game seen in the foreground, so a clip saved right
/// after alt-tabbing out of a game still goes into that game's folder.
#[derive(Default)]
pub struct GameTracker {
    last: Option<(String, Instant)>,
}

impl GameTracker {
    pub fn poll(&mut self, custom: &BTreeMap<String, String>) {
        if let Some(name) = foreground_exe().and_then(|exe| game_name(&exe, custom)) {
            self.last = Some((name, Instant::now()));
        }
    }

    /// The game seen in the foreground within `within`, or "Desktop".
    pub fn current(&self, within: Duration) -> String {
        match &self.last {
            Some((name, seen)) if seen.elapsed() <= within => name.clone(),
            _ => DESKTOP.to_string(),
        }
    }
}

fn foreground_exe() -> Option<PathBuf> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// The game an exe belongs to, or `None` if it doesn't look like a game.
pub fn game_name(exe: &Path, custom: &BTreeMap<String, String>) -> Option<String> {
    let file = exe.file_name()?.to_str()?.to_lowercase();
    if let Some((_, name)) = custom.iter().find(|(k, _)| k.to_lowercase() == file) {
        return Some(name.clone());
    }
    if let Some((_, name)) = KNOWN.iter().find(|(k, _)| *k == file) {
        return Some(name.to_string());
    }
    let parts: Vec<&str> = exe.components().filter_map(|c| match c {
        Component::Normal(s) => s.to_str(),
        _ => None,
    }).collect();
    for library in LIBRARIES {
        for i in 0..parts.len().saturating_sub(library.len() + 1) {
            let matches = library.iter().enumerate().all(|(j, l)| parts[i + j].eq_ignore_ascii_case(l));
            if matches {
                return Some(parts[i + library.len()].to_string());
            }
        }
    }
    None
}
