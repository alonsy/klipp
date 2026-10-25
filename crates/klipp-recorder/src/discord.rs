//! Finding the Discord app, whose sound becomes the Chat track.

use crate::games::processes;

/// Discord and its test builds.
const EXES: &[&str] = &["discord.exe", "discordptb.exe", "discordcanary.exe"];

/// The Discord app's main process: the one whose parent isn't Discord too.
/// Discord plays its sound from processes it starts, so this one's process
/// tree is all of Discord. `None` if it isn't running.
pub fn main_process() -> Option<u32> {
    root(&processes())
}

fn root(processes: &[(u32, u32, String)]) -> Option<u32> {
    processes
        .iter()
        .filter(|(_, _, name)| EXES.contains(&name.as_str()))
        .find(|(_, parent, name)| !processes.iter().any(|(pid, _, n)| pid == parent && n == name))
        .map(|(pid, ..)| *pid)
}
