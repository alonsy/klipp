//! CPU and memory use of this process, for checking against the performance budget.

use std::mem::size_of;
use std::time::Duration;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessStats {
    /// Total CPU time used so far (user + kernel).
    pub cpu: Duration,
    /// RAM only this process uses. This is Task Manager's "Memory" column.
    pub private_working_set: usize,
    /// Committed private memory, whether in RAM or paged out. The best leak
    /// indicator: it grows with a leak even when Windows pages memory out.
    pub commit: usize,
}

pub fn sample() -> ProcessStats {
    unsafe {
        let process = GetCurrentProcess();
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        let cpu = match GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) {
            Ok(()) => filetime(kernel) + filetime(user),
            Err(_) => Duration::ZERO,
        };
        let mut mem = PROCESS_MEMORY_COUNTERS_EX2::default();
        if GetProcessMemoryInfo(
            process,
            &mut mem as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS,
            size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        )
        .is_err()
        {
            mem = Default::default();
        }
        ProcessStats {
            cpu,
            private_working_set: mem.PrivateWorkingSetSize,
            commit: mem.PrivateUsage,
        }
    }
}

fn filetime(t: FILETIME) -> Duration {
    let ticks = ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
    Duration::from_nanos(ticks * 100)
}

pub fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
