mod nvidia;

use crate::collect::{CollectStatus, Collector};
use crate::domain::gpu::GpuInfo;

const MAX_HISTORY: usize = 300;

/// Push a value onto a history deque, capping at `MAX_HISTORY`.
fn push_history(history: &mut std::collections::VecDeque<i64>, value: i64) {
    history.push_back(value);
    if history.len() > MAX_HISTORY {
        history.pop_front();
    }
}

/// Clamp a percentage value to [0, 100].
fn clamp_percent(value: u32) -> i64 {
    value.min(100) as i64
}

/// Calculate power percentage from current draw and max limit (both in mW).
fn power_percent(power_mw: u64, max_power_mw: u64) -> i64 {
    if max_power_mw == 0 {
        return 0;
    }
    super::win::percent_u64(power_mw, max_power_mw).min(100)
}

/// A vendor-specific GPU monitoring backend.
///
/// Each backend dynamically loads a vendor library at runtime and provides
/// GPU discovery and per-cycle metric collection. Backends are implementation
/// details of `GpuCollector` — not exposed outside this module.
trait GpuBackend {
    /// Discover GPUs and return initial info (name, max clock, power limit).
    fn init_devices(&mut self) -> Vec<GpuInfo>;

    /// Update dynamic metrics for each GPU. `gpus` slice length matches
    /// the count returned by `init_devices`.
    fn collect(&mut self, gpus: &mut [GpuInfo]);

    /// Clean shutdown of the vendor library.
    fn shutdown(&mut self);
}

/// GPU data collector that aggregates all available vendor backends.
///
/// At startup, each vendor backend is probed via dynamic library loading.
/// Detected GPUs from all backends are combined into a single flat list.
pub struct GpuCollector {
    backends: Vec<BackendSlice>,
    pub gpus: Vec<GpuInfo>,
    pub status: CollectStatus,
}

/// Maps a backend to its range within the `gpus` vec.
struct BackendSlice {
    backend: Box<dyn GpuBackend>,
    start: usize,
    count: usize,
}

impl GpuCollector {
    pub fn new() -> Self {
        let mut gpus = Vec::new();
        let mut backends = Vec::new();

        // Probe each vendor backend in order.
        if let Some(mut nvidia) = nvidia::NvidiaBackend::load() {
            let devices = nvidia.init_devices();
            let start = gpus.len();
            let count = devices.len();
            gpus.extend(devices);
            backends.push(BackendSlice {
                backend: Box::new(nvidia),
                start,
                count,
            });
        }

        // Future: AMD and Intel backends added here.

        Self {
            backends,
            gpus,
            status: CollectStatus::Ok,
        }
    }

    fn collect_impl(&mut self) {
        if self.backends.is_empty() {
            self.status = CollectStatus::Failed("no GPU backend");
            return;
        }

        self.status = CollectStatus::Ok;
        for slice in &mut self.backends {
            let range = slice.start..slice.start + slice.count;
            slice.backend.collect(&mut self.gpus[range]);
        }
    }

    pub fn shutdown(&mut self) {
        for slice in &mut self.backends {
            slice.backend.shutdown();
        }
    }
}

impl Default for GpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for GpuCollector {
    fn collect(&mut self) {
        self.collect_impl();
    }
}

impl Drop for GpuCollector {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_collector_new_does_not_panic() {
        let collector = GpuCollector::new();
        let _ = collector.gpus.len();
    }

    #[test]
    fn gpu_collector_collect_without_backends() {
        let mut collector = GpuCollector {
            backends: Vec::new(),
            gpus: Vec::new(),
            status: CollectStatus::Ok,
        };
        collector.collect();
        assert_eq!(collector.status, CollectStatus::Failed("no GPU backend"));
    }

    #[test]
    fn clamp_percent_caps_at_100() {
        assert_eq!(clamp_percent(42), 42);
        assert_eq!(clamp_percent(150), 100);
    }

    #[test]
    fn power_percent_zero_on_zero_limit() {
        assert_eq!(power_percent(50, 0), 0);
    }

    #[test]
    fn power_percent_calculates_correctly() {
        assert_eq!(power_percent(50, 100), 50);
        assert_eq!(power_percent(200, 100), 100);
    }

    #[test]
    fn push_history_caps_at_max() {
        let mut history = std::collections::VecDeque::new();
        for i in 0..MAX_HISTORY + 10 {
            push_history(&mut history, i as i64);
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(*history.front().unwrap(), 10);
    }
}
