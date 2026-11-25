use serde::Serialize;
use std::ffi::CString;

#[derive(Serialize, Clone, Copy, Debug)]
pub struct DiskUsage {
    pub total: u64,
    pub free: u64,
    pub used: u64,
    pub used_pct: f32,
}

/// Volume stats for the Data volume (APFS containers share space, so `/` reports the container).
pub fn usage(path: &str) -> Option<DiskUsage> {
    let c = CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frsize = s.f_frsize as u64;
    let total = s.f_blocks as u64 * frsize;
    let free = s.f_bavail as u64 * frsize;
    let used = total.saturating_sub(free);
    let used_pct = if total == 0 { 0.0 } else { used as f32 / total as f32 * 100.0 };
    Some(DiskUsage { total, free, used, used_pct })
}
