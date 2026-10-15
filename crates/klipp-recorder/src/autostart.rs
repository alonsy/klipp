//! "Start with Windows": an entry under HKCU\...\CurrentVersion\Run.

use anyhow::Result;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: PCWSTR = w!("Klipp");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, access, &mut key) }.is_ok().then_some(key)
}

pub fn is_enabled() -> bool {
    let Some(key) = open(KEY_QUERY_VALUE) else { return false };
    let found = unsafe { RegQueryValueExW(key, VALUE, None, None, None, None) }.is_ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    found
}

/// Points the Run entry at the running executable, or removes it.
pub fn set(enabled: bool) -> Result<()> {
    let key = open(KEY_SET_VALUE).ok_or_else(|| anyhow::anyhow!("can't open the Run registry key"))?;
    let result = if enabled {
        let exe = std::env::current_exe()?;
        let command = format!("\"{}\"", exe.display());
        let wide: Vec<u16> = command.encode_utf16().chain([0]).collect();
        let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        unsafe { RegSetValueExW(key, VALUE, None, REG_SZ, Some(bytes)) }
    } else {
        unsafe { RegDeleteValueW(key, VALUE) }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result.ok()?;
    Ok(())
}
