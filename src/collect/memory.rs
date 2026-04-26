use crate::domain::memory::MemInfo;
use std::collections::VecDeque;

const MAX_HISTORY: usize = 300;

/// Memory data collector using Windows APIs.
pub struct MemCollector {
    pub info: MemInfo,
}

impl MemCollector {
    /// Create a new memory collector.
    pub fn new() -> Self {
        Self {
            info: MemInfo::default(),
        }
    }

    /// Collect current memory and swap data.
    pub fn collect(&mut self) -> &MemInfo {
        self.collect_memory();
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
                let swap_total = mem_status.ullTotalPageFile.saturating_sub(total);
                let swap_avail = mem_status.ullAvailPageFile.saturating_sub(available);
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
pub fn calculate_swap(
    total_page: u64,
    total_phys: u64,
    avail_page: u64,
    avail_phys: u64,
) -> (u64, u64, u64) {
    let swap_total = total_page.saturating_sub(total_phys);
    let swap_avail = avail_page.saturating_sub(avail_phys);
    let swap_used = swap_total.saturating_sub(swap_avail);
    (swap_total, swap_used, swap_avail)
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
    #[ignore]
    fn collect_returns_valid_mem_info() {
        let mut collector = MemCollector::new();
        collector.collect();
        assert!(*collector.info.stats.get("used").unwrap() > 0);
    }
}
