use crate::domain::memory::MemInfo;
use std::collections::VecDeque;

use super::{Collector, win::percent_u64};

const MAX_HISTORY: usize = 300;

/// Memory data collector using Windows APIs.
pub struct MemCollector {
    pub info: MemInfo,
    pub status: super::CollectStatus,
}

impl Default for MemCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MemCollector {
    /// Create a new memory collector.
    pub fn new() -> Self {
        Self {
            info: MemInfo::default(),
            status: super::CollectStatus::Ok,
        }
    }

    fn collect_impl(&mut self) {
        self.status = super::CollectStatus::Ok;
        self.collect_memory();
    }

    fn collect_memory(&mut self) {
        use windows::Win32::System::SystemInformation::*;

        let mut mem_status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };

        // SAFETY: mem_status is a properly initialized MEMORYSTATUSEX with
        // dwLength set to the struct size. GlobalMemoryStatusEx writes to
        // this valid, properly-aligned struct and the return value is checked.
        unsafe {
            if GlobalMemoryStatusEx(&mut mem_status).is_ok() {
                let total = mem_status.ullTotalPhys;
                let available = mem_status.ullAvailPhys;
                let used = total.saturating_sub(available);

                self.info.stats.used = used;
                self.info.stats.available = available;
                self.info.stats.free = available;

                // Swap = Page file - Physical memory
                let swap_total = mem_status.ullTotalPageFile.saturating_sub(total);
                let swap_avail = mem_status.ullAvailPageFile.saturating_sub(available);
                let swap_used = swap_total.saturating_sub(swap_avail);

                self.info.stats.swap_total = swap_total;
                self.info.stats.swap_used = swap_used;
                self.info.stats.swap_free = swap_avail;

                // Percentages
                if total > 0 {
                    push_pct(&mut self.info.percent.used, used, total);
                    push_pct(&mut self.info.percent.available, available, total);
                    push_pct(&mut self.info.percent.free, available, total);
                }
                if swap_total > 0 {
                    push_pct(&mut self.info.percent.swap_used, swap_used, swap_total);
                    push_pct(&mut self.info.percent.swap_free, swap_avail, swap_total);
                }
            } else {
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Memory,
                    "GlobalMemoryStatusEx failed",
                );
                self.status
                    .downgrade(super::CollectStatus::Failed("memory query failed"));
            }
        }

        // Cache from GetPerformanceInfo
        use windows::Win32::System::ProcessStatus::*;

        let mut perf = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        // SAFETY: perf is a properly initialized PERFORMANCE_INFORMATION with
        // cb set to the struct size. GetPerformanceInfo writes to this valid,
        // properly-aligned struct and the return value is checked.
        unsafe {
            if GetPerformanceInfo(&mut perf, perf.cb).is_ok() {
                let cached = cache_bytes(perf.SystemCache as u64, perf.PageSize as u64);
                self.info.stats.cached = cached;
                let total = self
                    .info
                    .stats
                    .used
                    .saturating_add(self.info.stats.available);
                if total > 0 {
                    push_pct(&mut self.info.percent.cached, cached, total);
                }
            } else {
                self.info.stats.cached = 0;
                self.status
                    .downgrade(super::CollectStatus::Degraded("cache query failed"));
            }
        }
    }
}

impl Collector for MemCollector {
    type Snapshot = crate::runner::MemSnapshot;

    fn collect(&mut self) {
        self.collect_impl();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::MemSnapshot {
            info: self.info.clone(),
            status: self.status.clone(),
        }
    }
}

fn push_pct(deque: &mut VecDeque<i64>, value: u64, total: u64) {
    let pct = percent_u64(value, total).min(100);
    deque.push_back(pct);
    while deque.len() > MAX_HISTORY {
        deque.pop_front();
    }
}

fn cache_bytes(system_cache_pages: u64, page_size: u64) -> u64 {
    system_cache_pages.saturating_mul(page_size)
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
    fn cache_bytes_saturates() {
        assert_eq!(cache_bytes(10, 4096), 40_960);
        assert_eq!(cache_bytes(u64::MAX, 2), u64::MAX);
    }

    #[test]
    fn collect_returns_valid_mem_info() {
        let mut collector = MemCollector::new();
        collector.collect();
        assert!(collector.info.stats.used > 0);
    }
}
