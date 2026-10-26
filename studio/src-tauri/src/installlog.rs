//! Lines for install.log next to the recorder's log: what the window's
//! updater did, with the local time. The installer writes there too, and the
//! recorder copies the file into recorder.log on its next start (its
//! installlog), so the steps of an update reach the developer with testers' logs.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn append(log_dir: &Path, line: &str) -> Result<()> {
    let t = unsafe { GetLocalTime() };
    std::fs::create_dir_all(log_dir)?;
    let mut f = OpenOptions::new().create(true).append(true).open(log_dir.join("install.log"))?;
    writeln!(f, "{:04}-{:02}-{:02} {:02}:{:02}:{:02} window: {line}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)?;
    Ok(())
}
