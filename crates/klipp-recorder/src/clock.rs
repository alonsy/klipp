//! One clock for every stream: the Windows performance counter, in 100 ns units.
//! Capture frame times and WASAPI audio positions use this same clock, so
//! video and audio line up without any guessing.

use std::sync::OnceLock;

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub const SECOND: i64 = 10_000_000;

pub fn now() -> i64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    let freq = *FREQUENCY.get_or_init(|| {
        let mut f = 0;
        unsafe { QueryPerformanceFrequency(&mut f).expect("QueryPerformanceFrequency") };
        f
    });
    let mut count = 0;
    unsafe { QueryPerformanceCounter(&mut count).expect("QueryPerformanceCounter") };
    (count as i128 * SECOND as i128 / freq as i128) as i64
}
