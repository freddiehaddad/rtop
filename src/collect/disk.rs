use crate::domain::disk::DiskData;
use std::collections::HashMap;

use super::Collector;

const MAX_HISTORY: usize = 300;
const PDH_FMT_DOUBLE: u32 = 0x00000200;

#[repr(C)]
#[derive(Default)]
struct PdhVal {
    status: u32,
    value: f64,
}

#[derive(Clone, Copy, Default)]
struct DiskPerfCounters {
    read: isize,
    write: isize,
    busy: isize,
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

/// Disk data collector using Windows APIs.
pub struct DiskCollector {
    /// Collected disk data.
    pub data: DiskData,
    pub status: super::CollectStatus,
    pdh_query: isize,
    pdh_counters: HashMap<String, DiskPerfCounters>,
    pdh_drive_order: Vec<String>,
    pdh_initialized: bool,
    pdh_has_first_sample: bool,
}

impl Default for DiskCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskCollector {
    /// Create a new disk collector.
    pub fn new() -> Self {
        Self {
            data: DiskData::default(),
            status: super::CollectStatus::Ok,
            pdh_query: 0,
            pdh_counters: HashMap::new(),
            pdh_drive_order: Vec::new(),
            pdh_initialized: false,
            pdh_has_first_sample: false,
        }
    }

    fn collect_impl(&mut self) {
        self.status = super::CollectStatus::Ok;

        use windows::Win32::Storage::FileSystem::*;
        use windows::core::*;

        let previous_disks = std::mem::take(&mut self.data.disks);
        self.data.disks_order.clear();

        // SAFETY: GetLogicalDriveStringsW writes to a stack-allocated u16 buffer
        // sized to 512 elements. GetDriveTypeW and GetDiskFreeSpaceExW receive
        // valid null-terminated wide strings from the drive enumeration. Return
        // values are checked before using the output.
        unsafe {
            let mut buf = [0u16; 512];
            let len = GetLogicalDriveStringsW(Some(&mut buf));
            if len == 0 {
                tracing::warn!("Disk: GetLogicalDriveStringsW returned 0");
                self.status
                    .downgrade(super::CollectStatus::Failed("drive query failed"));
                return;
            }

            let drives_str = String::from_utf16_lossy(&buf[..len as usize]);
            for drive in drives_str.split('\0').filter(|s| !s.is_empty()) {
                let drive_w: Vec<u16> = drive.encode_utf16().chain(std::iter::once(0)).collect();
                let drive_type = GetDriveTypeW(PCWSTR(drive_w.as_ptr()));

                // Only fixed and removable drives
                if drive_type != 3 && drive_type != 2 {
                    continue;
                }

                let mut free_bytes = 0u64;
                let mut total_bytes = 0u64;
                let mut total_free_bytes = 0u64;

                if GetDiskFreeSpaceExW(
                    PCWSTR(drive_w.as_ptr()),
                    Some(&mut free_bytes),
                    Some(&mut total_bytes),
                    Some(&mut total_free_bytes),
                )
                .is_ok()
                {
                    let used = total_bytes.saturating_sub(free_bytes);
                    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;

                    let mut vol_name = [0u16; 256];
                    let mut fs_name = [0u16; 32];
                    let _ = GetVolumeInformationW(
                        PCWSTR(drive_w.as_ptr()),
                        Some(&mut vol_name),
                        None,
                        None,
                        None,
                        Some(&mut fs_name),
                    );

                    let fstype = String::from_utf16_lossy(&fs_name)
                        .trim_end_matches('\0')
                        .to_string();

                    let name = drive.trim_end_matches('\\').to_string();

                    let mut disk = previous_disks.get(&name).cloned().unwrap_or_default();
                    disk.name = name.clone();
                    disk.fstype = fstype;
                    disk.total = total_bytes;
                    disk.used = used;
                    disk.used_percent = used_pct;

                    self.data.disks_order.push(name.clone());
                    self.data.disks.insert(name, disk);
                }
            }
        }

        self.ensure_perf_query();
        self.collect_perf();
    }

    fn ensure_perf_query(&mut self) {
        let drives = self.data.disks_order.clone();
        if self.pdh_initialized && self.pdh_drive_order == drives {
            return;
        }

        self.close_perf_query();
        if drives.is_empty() {
            return;
        }

        // SAFETY: PDH functions receive valid pointers to stack-allocated
        // handles and null-terminated UTF-16 counter path strings. Return
        // values are checked; the query is closed on failure.
        unsafe {
            let mut query: isize = 0;
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                tracing::warn!("Disk: PdhOpenQueryW failed");
                self.status
                    .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
                return;
            }

            for drive in &drives {
                let counters = DiskPerfCounters {
                    read: add_pdh_counter(query, &counter_path(drive, "Disk Read Bytes/sec")),
                    write: add_pdh_counter(query, &counter_path(drive, "Disk Write Bytes/sec")),
                    busy: add_pdh_counter(query, &counter_path(drive, "% Disk Time")),
                };

                if counters.read != 0 || counters.write != 0 || counters.busy != 0 {
                    self.pdh_counters.insert(drive.clone(), counters);
                }
            }

            if self.pdh_counters.is_empty() {
                let _ = PdhCloseQuery(query);
                tracing::warn!("Disk: no logical disk performance counters available");
                self.status
                    .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
                return;
            }

            let _ = PdhCollectQueryData(query);
            self.pdh_query = query;
            self.pdh_drive_order = drives;
            self.pdh_initialized = true;
            self.pdh_has_first_sample = false;
        }
    }

    fn collect_perf(&mut self) {
        if self.pdh_query == 0 {
            return;
        }

        if !self.pdh_has_first_sample {
            self.pdh_has_first_sample = true;
            return;
        }

        // SAFETY: self.pdh_query and counter handles were successfully
        // initialized by PdhOpenQueryW/PdhAddCounterW in ensure_perf_query().
        unsafe {
            if PdhCollectQueryData(self.pdh_query) != 0 {
                tracing::warn!("Disk: PdhCollectQueryData failed");
                self.status
                    .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
                return;
            }
        }

        let counters: Vec<(String, DiskPerfCounters)> = self
            .pdh_counters
            .iter()
            .map(|(drive, counters)| (drive.clone(), *counters))
            .collect();

        for (drive, counters) in counters {
            let read = formatted_counter(counters.read).unwrap_or(0.0).max(0.0) as u64;
            let write = formatted_counter(counters.write).unwrap_or(0.0).max(0.0) as u64;
            let busy = formatted_counter(counters.busy).unwrap_or(0.0).round() as i32;

            if let Some(disk) = self.data.disks.get_mut(&drive) {
                disk.read_bytes_per_sec = read;
                disk.write_bytes_per_sec = write;
                disk.read_top = disk.read_top.max(read);
                disk.write_top = disk.write_top.max(write);
                disk.busy_percent = busy.clamp(0, 100);
                push_history(&mut disk.read_history, read as i64);
                push_history(&mut disk.write_history, write as i64);
            }
        }
    }

    fn close_perf_query(&mut self) {
        if self.pdh_query != 0 {
            // SAFETY: pdh_query is owned by this collector and was returned by
            // PdhOpenQueryW. It is closed at most once before the handle is reset.
            unsafe {
                let _ = PdhCloseQuery(self.pdh_query);
            }
        }
        self.pdh_query = 0;
        self.pdh_counters.clear();
        self.pdh_drive_order.clear();
        self.pdh_initialized = false;
        self.pdh_has_first_sample = false;
    }
}

impl Drop for DiskCollector {
    fn drop(&mut self) {
        self.close_perf_query();
    }
}

impl Collector for DiskCollector {
    fn collect(&mut self) {
        self.collect_impl();
    }
}

fn counter_path(drive: &str, counter: &str) -> Vec<u16> {
    format!("\\LogicalDisk({drive})\\{counter}\0")
        .encode_utf16()
        .collect()
}

unsafe fn add_pdh_counter(query: isize, path: &[u16]) -> isize {
    let mut counter = 0isize;
    // SAFETY: query is a valid PDH query handle and path is a null-terminated
    // UTF-16 counter path allocated by counter_path().
    if unsafe { PdhAddCounterW(query, path.as_ptr(), 0, &mut counter) } == 0 {
        counter
    } else {
        0
    }
}

fn formatted_counter(counter: isize) -> Option<f64> {
    if counter == 0 {
        return None;
    }
    let mut value = PdhVal::default();
    let mut counter_type: u32 = 0;
    // SAFETY: counter is a PDH counter handle added to the collector query.
    // PdhVal is repr(C) and properly aligned for the API output.
    let ok = unsafe {
        PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, &mut counter_type, &mut value)
    } == 0
        && value.status == 0;
    ok.then_some(value.value)
}

fn push_history(history: &mut std::collections::VecDeque<i64>, value: i64) {
    history.push_back(value);
    while history.len() > MAX_HISTORY {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_collector_new_is_empty() {
        let c = DiskCollector::new();
        assert!(c.data.disks.is_empty());
        assert!(c.data.disks_order.is_empty());
    }

    #[test]
    fn counter_path_uses_logical_disk_instance() {
        let path = counter_path("C:", "Disk Read Bytes/sec");
        let s = String::from_utf16_lossy(&path);
        assert_eq!(s, "\\LogicalDisk(C:)\\Disk Read Bytes/sec\0");
    }

    #[test]
    fn push_history_caps_length() {
        let mut history = std::collections::VecDeque::new();
        for i in 0..(MAX_HISTORY + 10) {
            push_history(&mut history, i as i64);
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history.front(), Some(&10));
    }

    #[test]
    fn collect_finds_at_least_one_drive() {
        let mut c = DiskCollector::new();
        c.collect();
        assert!(!c.data.disks.is_empty(), "expected at least one disk");
        assert!(!c.data.disks_order.is_empty());
        // C: should exist on any Windows system
        assert!(c.data.disks.contains_key("C:"), "expected C: drive");
    }
}
