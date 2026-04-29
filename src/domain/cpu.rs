use std::collections::VecDeque;

/// Aggregate CPU usage histories by category.
#[derive(Debug, Clone, Default)]
pub struct CpuPercent {
    pub total: VecDeque<i64>,
    pub user: VecDeque<i64>,
    pub system: VecDeque<i64>,
    pub idle: VecDeque<i64>,
}

/// Look up a `CpuPercent` field by name.
pub fn get_cpu_series<'a>(cpu: &'a CpuPercent, key: &str) -> Option<&'a VecDeque<i64>> {
    match key {
        "total" => Some(&cpu.total),
        "user" => Some(&cpu.user),
        "system" => Some(&cpu.system),
        "idle" => Some(&cpu.idle),
        _ => Some(&cpu.total),
    }
}

/// CPU usage data for all cores and aggregate statistics.
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// Aggregate CPU usage histories by category.
    pub cpu_percent: CpuPercent,
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
    /// CPU package power in watts (from LHM), if available.
    pub cpu_watts: Option<f64>,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self {
            cpu_percent: CpuPercent::default(),
            core_percent: Vec::new(),
            load_avg: [0.0; 3],
            cpu_name: String::new(),
            cpu_hz: String::new(),
            core_count: 0,
            uptime_seconds: 0,
            temp: Vec::new(),
            cpu_watts: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cpu_info_has_empty_percent_fields() {
        let info = CpuInfo::default();
        assert!(info.cpu_percent.total.is_empty());
        assert!(info.cpu_percent.user.is_empty());
        assert!(info.cpu_percent.system.is_empty());
        assert!(info.cpu_percent.idle.is_empty());
    }

    #[test]
    fn get_cpu_series_returns_correct_fields() {
        let mut pct = CpuPercent::default();
        pct.total.push_back(42);
        pct.user.push_back(10);
        pct.system.push_back(20);
        pct.idle.push_back(30);

        assert_eq!(get_cpu_series(&pct, "total").unwrap().back(), Some(&42));
        assert_eq!(get_cpu_series(&pct, "user").unwrap().back(), Some(&10));
        assert_eq!(get_cpu_series(&pct, "system").unwrap().back(), Some(&20));
        assert_eq!(get_cpu_series(&pct, "idle").unwrap().back(), Some(&30));
        // Unknown key falls back to total
        assert_eq!(get_cpu_series(&pct, "unknown").unwrap().back(), Some(&42));
    }

    #[test]
    fn default_cpu_info_has_zero_cores() {
        let info = CpuInfo::default();
        assert_eq!(info.core_count, 0);
        assert!(info.core_percent.is_empty());
    }
}
