use windows::Win32::Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE};
use windows::Win32::System::Registry::{HKEY, RegCloseKey};

const PDH_FMT_DOUBLE: u32 = 0x00000200;

#[repr(C)]
#[derive(Default)]
struct PdhVal {
    status: u32,
    value: f64,
}

// SAFETY: FFI declarations for pdh.dll Performance Data Helper functions;
// signatures match the Windows PDH API.
#[link(name = "pdh")]
unsafe extern "system" {
    fn PdhOpenQueryW(ds: *const u16, ud: usize, q: *mut isize) -> i32;
    fn PdhAddCounterW(q: isize, p: *const u16, ud: usize, c: *mut isize) -> i32;
    fn PdhCollectQueryData(q: isize) -> i32;
    fn PdhGetFormattedCounterValue(c: isize, f: u32, ct: *mut u32, v: *mut PdhVal) -> i32;
    fn PdhCloseQuery(q: isize) -> i32;
}

/// Owned Windows HANDLE that closes itself on drop.
pub(crate) struct OwnedHandle(HANDLE);

impl OwnedHandle {
    pub(crate) fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_invalid()).then_some(Self(handle))
    }

    pub(crate) fn get(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: OwnedHandle is only constructed from non-invalid handles and
        // is non-clone, so this closes the handle exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Owned Windows registry key that closes itself on drop.
pub(crate) struct OwnedRegKey(HKEY);

impl OwnedRegKey {
    pub(crate) fn new(key: HKEY) -> Option<Self> {
        (!key.is_invalid()).then_some(Self(key))
    }

    pub(crate) fn get(&self) -> HKEY {
        self.0
    }
}

impl Drop for OwnedRegKey {
    fn drop(&mut self) {
        // SAFETY: OwnedRegKey is only constructed from non-invalid keys and is
        // non-clone, so this closes the key exactly once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Owned loaded module that calls FreeLibrary on drop.
pub(crate) struct OwnedLibrary(HMODULE);

impl OwnedLibrary {
    pub(crate) fn new(module: HMODULE) -> Option<Self> {
        (!module.is_invalid()).then_some(Self(module))
    }

    pub(crate) fn get(&self) -> HMODULE {
        self.0
    }
}

impl Drop for OwnedLibrary {
    fn drop(&mut self) {
        // SAFETY: OwnedLibrary is only constructed from non-invalid modules and
        // is non-clone, so this unloads the module exactly once.
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

/// Owned PDH query. Closing a query releases all counters added to it.
pub(crate) struct PdhQuery {
    raw: isize,
}

impl PdhQuery {
    pub(crate) fn open() -> Result<Self, i32> {
        let mut raw = 0isize;
        // SAFETY: PdhOpenQueryW writes an opaque query handle to raw. Return
        // status is checked before constructing the owner.
        let status = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut raw) };
        if status == 0 && raw != 0 {
            Ok(Self { raw })
        } else {
            Err(status)
        }
    }

    pub(crate) fn add_counter(&self, path: &[u16]) -> Result<PdhCounter, i32> {
        let mut raw = 0isize;
        // SAFETY: self.raw is an open PDH query and path is expected to be a
        // null-terminated UTF-16 counter path.
        let status = unsafe { PdhAddCounterW(self.raw, path.as_ptr(), 0, &mut raw) };
        if status == 0 && raw != 0 {
            Ok(PdhCounter { raw })
        } else {
            Err(status)
        }
    }

    pub(crate) fn collect(&self) -> Result<(), i32> {
        // SAFETY: self.raw is an open PDH query owned by this PdhQuery.
        let status = unsafe { PdhCollectQueryData(self.raw) };
        if status == 0 { Ok(()) } else { Err(status) }
    }
}

impl Drop for PdhQuery {
    fn drop(&mut self) {
        if self.raw != 0 {
            // SAFETY: self.raw is owned by this PdhQuery and closed exactly
            // once when the non-clone owner is dropped.
            unsafe {
                let _ = PdhCloseQuery(self.raw);
            }
        }
    }
}

/// Non-owning PDH counter handle. The containing PdhQuery owns the lifetime.
#[derive(Clone, Copy, Default)]
pub(crate) struct PdhCounter {
    raw: isize,
}

impl PdhCounter {
    pub(crate) fn is_valid(self) -> bool {
        self.raw != 0
    }

    pub(crate) fn formatted_f64(self) -> Option<f64> {
        if !self.is_valid() {
            return None;
        }

        let mut value = PdhVal::default();
        let mut counter_type: u32 = 0;
        // SAFETY: self.raw is a counter handle added to a live PdhQuery.
        let ok = unsafe {
            PdhGetFormattedCounterValue(self.raw, PDH_FMT_DOUBLE, &mut counter_type, &mut value)
        } == 0
            && value.status == 0;
        ok.then_some(value.value).filter(|value| value.is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterDelta {
    Delta(u64),
    Reset,
}

pub(crate) fn counter_delta(current: u64, previous: u64) -> CounterDelta {
    if current >= previous {
        CounterDelta::Delta(current - previous)
    } else {
        CounterDelta::Reset
    }
}

pub(crate) fn bytes_per_sec(current: u64, previous: u64, elapsed_secs: f64) -> u64 {
    if previous == 0 || elapsed_secs <= 0.0 || !elapsed_secs.is_finite() {
        return 0;
    }

    let CounterDelta::Delta(delta) = counter_delta(current, previous) else {
        return 0;
    };

    (delta as f64 / elapsed_secs).clamp(0.0, u64::MAX as f64) as u64
}

pub(crate) fn checked_u32_size(size: usize) -> Option<u32> {
    u32::try_from(size).ok()
}

pub(crate) fn utf16_len_until_nul(buf: &[u16]) -> usize {
    buf.iter().position(|&c| c == 0).unwrap_or(buf.len())
}

pub(crate) fn string_from_utf16_buf(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..utf16_len_until_nul(buf)])
        .trim()
        .to_string()
}

pub(crate) fn string_from_c_buf(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).trim().to_string()
}

pub(crate) fn percent_u64(part: u64, total: u64) -> i64 {
    if total == 0 {
        return 0;
    }

    // Round to nearest instead of truncating
    ((part as u128 * 100 + total as u128 / 2) / total as u128).min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_delta_normal() {
        assert_eq!(counter_delta(150, 100), CounterDelta::Delta(50));
    }

    #[test]
    fn counter_delta_reset() {
        assert_eq!(counter_delta(50, 100), CounterDelta::Reset);
    }

    #[test]
    fn bytes_per_sec_from_counter_delta() {
        assert_eq!(bytes_per_sec(3_000, 1_000, 2.0), 1_000);
    }

    #[test]
    fn bytes_per_sec_zero_without_previous_or_elapsed() {
        assert_eq!(bytes_per_sec(1_000, 0, 1.0), 0);
        assert_eq!(bytes_per_sec(1_000, 500, 0.0), 0);
    }

    #[test]
    fn bytes_per_sec_zero_on_reset() {
        assert_eq!(bytes_per_sec(500, 1_000, 1.0), 0);
    }

    #[test]
    fn percent_u64_handles_zero_and_large_values() {
        assert_eq!(percent_u64(50, 100), 50);
        assert_eq!(percent_u64(1, 0), 0);
        assert_eq!(percent_u64(u64::MAX, u64::MAX), 100);
    }

    #[test]
    fn string_from_utf16_buf_stops_at_nul() {
        let buf = ['r' as u16, 't' as u16, 0, 'x' as u16];
        assert_eq!(string_from_utf16_buf(&buf), "rt");
    }

    #[test]
    fn string_from_c_buf_stops_at_nul() {
        assert_eq!(string_from_c_buf(b"gpu\0garbage"), "gpu");
    }
}
