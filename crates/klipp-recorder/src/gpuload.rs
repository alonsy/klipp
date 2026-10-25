//! How busy the graphics card's engines were over the last minute, for the
//! log: the 3D engine (games, and Klipp's colour conversion), video encode,
//! copy. From Windows' own "GPU Engine" performance counters, which need no
//! admin rights. A card at 100 % 3D with Klipp's work waiting behind the
//! game is what FIX-17 is about, so the log says when that happens.

use std::collections::BTreeMap;

use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

pub struct GpuLoad {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

/// One engine type's busiest engine over the interval, whole card and Klipp's own part, in percent.
pub struct Engine {
    pub kind: String,
    pub total: f64,
    pub klipp: f64,
}

impl GpuLoad {
    /// Starts counting; `None` if Windows has no GPU counters (older drivers).
    pub fn start() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        let mut counter = PDH_HCOUNTER::default();
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut counter) != 0 {
                let _ = PdhCloseQuery(query);
                return None;
            }
            // Utilization is a rate: the first sample only sets the start.
            let _ = PdhCollectQueryData(query);
        }
        Some(Self { query, counter })
    }

    /// Since the last call: each engine type's busiest engine, busiest type first.
    pub fn sample(&mut self) -> Vec<Engine> {
        let me = format!("pid_{}_", std::process::id());
        // (engine, type) -> (all processes, Klipp)
        let mut engines: BTreeMap<(String, String), (f64, f64)> = BTreeMap::new();
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return Vec::new();
            }
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                return Vec::new();
            }
            // Items first, then the names they point to, in one buffer.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return Vec::new();
            }
            for item in std::slice::from_raw_parts(items, count as usize) {
                let Ok(name) = item.szName.to_string() else { continue };
                let value = item.FmtValue.Anonymous.doubleValue;
                // "pid_1234_luid_0x0_0xD1A5_phys_0_eng_3_engtype_VideoEncode"
                let (Some(eng), Some(kind)) = (name.find("_luid_"), name.find("_engtype_")) else { continue };
                let entry = engines.entry((name[eng..kind].to_string(), name[kind + 9..].to_string())).or_default();
                entry.0 += value;
                if name.starts_with(&me) {
                    entry.1 += value;
                }
            }
        }
        let mut kinds: BTreeMap<String, (f64, f64)> = BTreeMap::new();
        for ((_, kind), (total, klipp)) in engines {
            let busiest = kinds.entry(kind).or_default();
            if total > busiest.0 {
                *busiest = (total.min(100.0), klipp.min(100.0));
            }
        }
        let mut out: Vec<Engine> = kinds.into_iter().map(|(kind, (total, klipp))| Engine { kind, total, klipp }).collect();
        out.sort_by(|a, b| b.total.total_cmp(&a.total));
        out
    }
}

impl Drop for GpuLoad {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}

/// "3D 98% (Klipp 1%), VideoEncode 21% (Klipp 21%), Copy 4%": the busy ones.
pub fn describe(engines: &[Engine]) -> String {
    let busy: Vec<String> = engines
        .iter()
        .filter(|e| e.total >= 1.0)
        .take(4)
        .map(|e| if e.klipp >= 0.5 { format!("{} {:.0}% (Klipp {:.0}%)", e.kind, e.total, e.klipp) } else { format!("{} {:.0}%", e.kind, e.total) })
        .collect();
    if busy.is_empty() { "idle".into() } else { busy.join(", ") }
}
