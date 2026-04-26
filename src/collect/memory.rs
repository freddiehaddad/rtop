use crate::domain::memory::{DiskInfo, MemInfo};
use std::collections::VecDeque;

const MAX_HISTORY: usize = 300;

/// Memory and disk data collector using Windows APIs.
pub struct MemCollector {
    pub info: MemInfo,
}

impl MemCollector {
    pub fn new() -> Self {
        Self {
            info: MemInfo::default(),
        }
    }

    /// Collect current memory and disk data.
    pub fn collect(&mut self) -> &MemInfo {
        self.collect_memory();
        self.collect_disks();
        &self.info
    }

    fn collect_memory(&mut self) {
        use windows::Win32::System::SystemInformation::*;

        let mut mem_status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };

        unsafe {
            if GlobalMemoryStatusEx(&mut mem_status).is_ok() {
                let total = mem_status.ullTotalPhys;
                let available = mem_status.ullAvailPhys;
                let used = total - available;

                self.info.stats.insert("used".into(), used);
                self.info.stats.insert("available".into(), available);
                self.info.stats.insert("free".into(), available);

                // Swap = Page file - Physical memory
                let swap_total = mem_status
                    .ullTotalPageFile
                    .saturating_sub(total);
                let swap_avail = mem_status
                    .ullAvailPageFile
                    .saturating_sub(available);
                let swap_used = swap_total.saturating_sub(swap_avail);

                self.info.stats.insert("swap_total".into(), swap_total);
                self.info.stats.insert("swap_used".into(), swap_used);
                self.info.stats.insert("swap_free".into(), swap_avail);

                // Percentages
                if total > 0 {
                    push_pct(&mut self.info.percent, "used", used, total);
                    push_pct(&mut self.info.percent, "available", available, total);
                    push_pct(&mut self.info.percent, "free", available, total);
                }
                if swap_total > 0 {
                    push_pct(&mut self.info.percent, "swap_used", swap_used, swap_total);
                    push_pct(&mut self.info.percent, "swap_free", swap_avail, swap_total);
                }
            }
        }

        // Cache from GetPerformanceInfo
        use windows::Win32::System::ProcessStatus::*;

        let mut perf = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        unsafe {
            if GetPerformanceInfo(&mut perf, perf.cb).is_ok() {
                let cached = perf.SystemCache as u64 * perf.PageSize as u64;
                self.info.stats.insert("cached".into(), cached);
                let total = *self.info.stats.get("used").unwrap_or(&0)
                    + *self.info.stats.get("available").unwrap_or(&0);
                if total > 0 {
                    push_pct(&mut self.info.percent, "cached", cached, total);
                }
            }
        }
    }

    fn collect_disks(&mut self) {
        use windows::Win32::Storage::FileSystem::*;
        use windows::core::*;

        self.info.disks.clear();
        self.info.disks_order.clear();

        unsafe {
            let mut buf = [0u16; 512];
            let len = GetLogicalDriveStringsW(Some(&mut buf));
            if len == 0 {
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

                    // Get volume info
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

                    let disk = DiskInfo {
                        name: name.clone(),
                        fstype,
                        total: total_bytes,
                        used,
                        used_percent: used_pct,
                    };

                    self.info.disks_order.push(name.clone());
                    self.info.disks.insert(name, disk);
                }
            }
        }
    }
}

fn push_pct(
    map: &mut std::collections::HashMap<String, VecDeque<i64>>,
    key: &str,
    value: u64,
    total: u64,
) {
    let pct = (value * 100 / total.max(1)) as i64;
    let deque = map.entry(key.to_string()).or_default();
    deque.push_back(pct);
    while deque.len() > MAX_HISTORY {
        deque.pop_front();
    }
}

#[cfg(test)]
/// Calculate used memory from total and available (for unit testing).
pub fn calculate_used(total: u64, available: u64) -> u64 {
    total.saturating_sub(available)
}

#[cfg(test)]
/// Calculate swap from page file values (for unit testing).
pub fn calculate_swap(total_page: u64, total_phys: u64, avail_page: u64, avail_phys: u64) -> (u64, u64, u64) {
    let swap_total = total_page.saturating_sub(total_phys);
    let swap_avail = avail_page.saturating_sub(avail_phys);
    let swap_used = swap_total.saturating_sub(swap_avail);
    (swap_total, swap_used, swap_avail)
}

#[cfg(test)]
/// Calculate disk usage percentage (for unit testing).
pub fn disk_percent(used: u64, total: u64) -> i32 {
    if total == 0 {
        return 0;
    }
    (used * 100 / total) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_used_from_total_available() {
        assert_eq!(calculate_used(16_000_000_000, 8_000_000_000), 8_000_000_000);
        assert_eq!(calculate_used(100, 150), 0); // Saturating
    }

    #[test]
    fn calculate_swap_from_pagefile() {
        let (total, used, free) = calculate_swap(32_000, 16_000, 20_000, 8_000);
        assert_eq!(total, 16_000);
        assert_eq!(free, 12_000);
        assert_eq!(used, 4_000);
    }

    #[test]
    fn disk_percent_calculation() {
        assert_eq!(disk_percent(600, 1000), 60);
        assert_eq!(disk_percent(0, 1000), 0);
        assert_eq!(disk_percent(1000, 1000), 100);
        assert_eq!(disk_percent(0, 0), 0);
    }

    #[test]
    #[ignore]
    fn collect_returns_valid_mem_info() {
        let mut collector = MemCollector::new();
        collector.collect();
        assert!(*collector.info.stats.get("used").unwrap() > 0);
    }
}
