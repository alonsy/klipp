//! Finding the Discord app, whose sound becomes the Chat track.

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};

/// Discord and its test builds.
const EXES: &[&str] = &["discord.exe", "discordptb.exe", "discordcanary.exe"];

/// The Discord app's main process: the one whose parent isn't Discord too.
/// Discord plays its sound from processes it starts, so this one's process
/// tree is all of Discord. `None` if it isn't running.
pub fn main_process() -> Option<u32> {
    root(&processes())
}

/// (process id, parent id, lowercase file name) of every running process.
fn processes() -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return out };
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        out.push((entry.th32ProcessID, entry.th32ParentProcessID, String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase()));
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    out
}

fn root(processes: &[(u32, u32, String)]) -> Option<u32> {
    processes
        .iter()
        .filter(|(_, _, name)| EXES.contains(&name.as_str()))
        .find(|(_, parent, name)| !processes.iter().any(|(pid, _, n)| pid == parent && n == name))
        .map(|(pid, ..)| *pid)
}
