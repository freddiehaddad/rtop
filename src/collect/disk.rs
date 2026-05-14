use crate::domain::disk::DiskData;
use std::collections::HashMap;

use super::{
    Collector,
    counters::percent_u64,
    win::{PdhCounter, PdhQuery, string_from_utf16_buf},
};

const MAX_HISTORY: usize = 300;

#[derive(Clone, Copy, Default)]
struct DiskPerfCounters {
    read: PdhCounter,
    write: PdhCounter,
    busy: PdhCounter,
}

/// Disk data collector using Windows APIs.
pub struct DiskCollector {
    /// Collected disk data.
    pub info: DiskData,
    pub status: super::CollectStatus,
    pdh_query: Option<PdhQuery>,
    pdh_counters: HashMap<String, DiskPerfCounters>,
    pdh_drive_order: Vec<String>,
    pdh_initialized: bool,
    pdh_has_first_sample: bool,
}

impl DiskCollector {
    /// Create a new disk collector.
    pub fn new() -> Self {
        Self {
            info: DiskData::default(),
            status: super::CollectStatus::Ok,
            pdh_query: None,
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

        let previous_disks = std::mem::take(&mut self.info.disks);

        let Some(drives_buf) = logical_drive_strings() else {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Disk,
                "GetLogicalDriveStringsW returned no drives",
            );
            self.status
                .downgrade(super::CollectStatus::Failed("drive query failed"));
            return;
        };
        let drives_str = String::from_utf16_lossy(&drives_buf);

        // SAFETY: GetDriveTypeW and GetDiskFreeSpaceExW receive valid
        // null-terminated wide strings produced from the bounded drive list.
        // Return values are checked before using output buffers.
        unsafe {
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
                    let used_pct = percent_u64(used, total_bytes).min(100) as i32;

                    let mut vol_name = [0u16; 256];
                    let mut fs_name = [0u16; 32];
                    let fstype = if GetVolumeInformationW(
                        PCWSTR(drive_w.as_ptr()),
                        Some(&mut vol_name),
                        None,
                        None,
                        None,
                        Some(&mut fs_name),
                    )
                    .is_ok()
                    {
                        string_from_utf16_buf(&fs_name)
                    } else {
                        String::new()
                    };

                    let name = drive.trim_end_matches('\\').to_string();

                    let mut disk = previous_disks
                        .iter()
                        .find(|d| d.name == name)
                        .cloned()
                        .unwrap_or_default();
                    disk.name = name;
                    disk.fstype = fstype;
                    disk.total = total_bytes;
                    disk.used = used;
                    disk.used_percent = used_pct;

                    self.info.disks.push(disk);
                }
            }
        }

        self.ensure_perf_query();
        self.collect_perf();
    }

    fn ensure_perf_query(&mut self) {
        let drives: Vec<String> = self.info.disks.iter().map(|d| d.name.clone()).collect();
        if self.pdh_initialized && self.pdh_drive_order == drives {
            return;
        }

        self.close_perf_query();
        if drives.is_empty() {
            return;
        }

        let Ok(query) = PdhQuery::open() else {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Disk,
                "PdhOpenQueryW failed",
            );
            self.status
                .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
            return;
        };

        for drive in &drives {
            let counters = DiskPerfCounters {
                read: add_pdh_counter(&query, &counter_path(drive, "Disk Read Bytes/sec")),
                write: add_pdh_counter(&query, &counter_path(drive, "Disk Write Bytes/sec")),
                busy: add_pdh_counter(&query, &counter_path(drive, "% Disk Time")),
            };

            if counters.read.is_valid() || counters.write.is_valid() || counters.busy.is_valid() {
                self.pdh_counters.insert(drive.clone(), counters);
            }
        }

        if self.pdh_counters.is_empty() {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Disk,
                "no logical disk performance counters available",
            );
            self.status
                .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
            return;
        }

        let _ = query.collect();
        self.pdh_query = Some(query);
        self.pdh_drive_order = drives;
        self.pdh_initialized = true;
        self.pdh_has_first_sample = false;
    }

    fn collect_perf(&mut self) {
        let Some(query) = self.pdh_query.as_ref() else {
            return;
        };

        if !self.pdh_has_first_sample {
            self.pdh_has_first_sample = true;
            return;
        }

        if query.collect().is_err() {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Disk,
                "PdhCollectQueryData failed",
            );
            self.status
                .downgrade(super::CollectStatus::Degraded("disk perf unavailable"));
            return;
        }

        let counters: Vec<(String, DiskPerfCounters)> = self
            .pdh_counters
            .iter()
            .map(|(drive, counters)| (drive.clone(), *counters))
            .collect();

        for (drive, counters) in counters {
            let read = counter_value_to_u64(counters.read.formatted_f64());
            let write = counter_value_to_u64(counters.write.formatted_f64());
            let busy = counter_value_to_percent(counters.busy.formatted_f64());

            if let Some(disk) = self.info.get_mut(&drive) {
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
        self.pdh_query = None;
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
    type Snapshot = crate::runner::DiskSnapshot;

    fn collect(&mut self) {
        self.collect_impl();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::DiskSnapshot {
            info: self.info.clone(),
            status: self.status.clone(),
        }
    }
}

fn logical_drive_strings() -> Option<Vec<u16>> {
    use windows::Win32::Storage::FileSystem::GetLogicalDriveStringsW;

    let mut buf = [0u16; 512];
    // SAFETY: buf is a valid writable UTF-16 buffer. The return value is checked
    // before slicing, and an oversized result is retried with the required size.
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) };
    if len == 0 {
        return None;
    }
    if (len as usize) <= buf.len() {
        return Some(buf[..len as usize].to_vec());
    }

    let mut dynamic_buf = vec![0u16; len as usize];
    // SAFETY: dynamic_buf is allocated to the size requested by the first call.
    // The copied length is checked against the allocation before slicing.
    let copied = unsafe { GetLogicalDriveStringsW(Some(&mut dynamic_buf)) };
    if copied == 0 || (copied as usize) > dynamic_buf.len() {
        return None;
    }
    dynamic_buf.truncate(copied as usize);
    Some(dynamic_buf)
}

fn counter_path(drive: &str, counter: &str) -> Vec<u16> {
    format!("\\LogicalDisk({drive})\\{counter}\0")
        .encode_utf16()
        .collect()
}

fn add_pdh_counter(query: &PdhQuery, path: &[u16]) -> PdhCounter {
    query.add_counter(path).unwrap_or_default()
}

fn counter_value_to_u64(value: Option<f64>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    if value.is_finite() && value > 0.0 {
        value.clamp(0.0, u64::MAX as f64) as u64
    } else {
        0
    }
}

fn counter_value_to_percent(value: Option<f64>) -> i32 {
    let Some(value) = value else {
        return 0;
    };
    if value.is_finite() {
        (value.round() as i32).clamp(0, 100)
    } else {
        0
    }
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
        assert!(c.info.disks.is_empty());
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
        assert!(!c.info.disks.is_empty(), "expected at least one disk");
        assert!(
            c.info.disks.iter().any(|d| d.name == "C:"),
            "expected C: drive"
        );
    }
}
