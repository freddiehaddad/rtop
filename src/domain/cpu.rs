use std::collections::{HashMap, VecDeque};

/// CPU usage data for all cores and aggregate statistics.
///
/// On Windows, `cpu_percent` contains these keys:
///   - "total", "user", "system", "idle", "irq", "dpc"
///     Linux-only keys ("nice","iowait","softirq","steal","guest","guest_nice")
///     are not populated.
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// Aggregate CPU usage histories by category.
    pub cpu_percent: HashMap<String, VecDeque<i64>>,
    /// Per-core usage history (index = core number).
    pub core_percent: Vec<VecDeque<i64>>,
    /// Emulated load average (1, 5, 15 minute rolling EMA).
    pub load_avg: [f64; 3],
    /// CPU model name (e.g. "Intel Core i9-13900K").
    pub cpu_name: String,
    /// Current CPU frequency as display string (e.g. "5.80 GHz").
    pub cpu_hz: String,
    /// Number of logical cores.
    pub core_count: usize,
    /// System uptime in seconds.
    pub uptime_seconds: u64,
    /// Temperature history: index 0 = package, 1+ = per-core.
    pub temp: Vec<VecDeque<i64>>,
    /// Critical temperature threshold (°C).
    pub temp_max: i64,
}

impl Default for CpuInfo {
    fn default() -> Self {
        let keys = ["total", "user", "system", "idle", "irq", "dpc"];
        Self {
            cpu_percent: keys.iter().map(|k| (k.to_string(), VecDeque::new())).collect(),
            core_percent: Vec::new(),
            load_avg: [0.0; 3],
            cpu_name: String::new(),
            cpu_hz: String::new(),
            core_count: 0,
            uptime_seconds: 0,
            temp: Vec::new(),
            temp_max: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cpu_info_has_correct_keys() {
        let info = CpuInfo::default();
        for key in &["total", "user", "system", "idle", "irq", "dpc"] {
            assert!(info.cpu_percent.contains_key(*key), "missing key: {key}");
        }
        assert_eq!(info.cpu_percent.len(), 6);
    }

    #[test]
    fn default_cpu_info_has_zero_cores() {
        let info = CpuInfo::default();
        assert_eq!(info.core_count, 0);
        assert!(info.core_percent.is_empty());
    }
}
