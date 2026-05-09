use std::collections::VecDeque;

use crate::domain::config_enums::CpuGraphSource;

/// Aggregate CPU usage histories by category.
#[derive(Debug, Clone, Default)]
pub struct CpuPercent {
    pub total: VecDeque<i64>,
    pub user: VecDeque<i64>,
    pub system: VecDeque<i64>,
    pub idle: VecDeque<i64>,
}

impl CpuPercent {
    /// Look up the per-source history for a graph row. `Auto` and
    /// `Total` both map to the aggregate `total` series; `User` and
    /// `System` map to their respective fields. `Idle` is intentionally
    /// not exposed via [`CpuGraphSource`] (the upper/lower-graph
    /// option-menu choices are User/System/Total/Auto only).
    pub fn series(&self, source: CpuGraphSource) -> &VecDeque<i64> {
        match source {
            CpuGraphSource::User => &self.user,
            CpuGraphSource::System => &self.system,
            CpuGraphSource::Auto | CpuGraphSource::Total => &self.total,
        }
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
    /// Temperature history: index 0 = package, 1+ = per-core.
    pub temp: Vec<VecDeque<i64>>,
    /// CPU package power in watts (from LHM), if available.
    pub cpu_watts: Option<f64>,
    /// Maximum observed CPU package power in watts (from LHM), if available.
    pub cpu_max_watts: Option<f64>,
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
            temp: Vec::new(),
            cpu_watts: None,
            cpu_max_watts: None,
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
    fn cpu_percent_series_returns_correct_field() {
        use crate::domain::config_enums::CpuGraphSource;
        let mut pct = CpuPercent::default();
        pct.total.push_back(42);
        pct.user.push_back(10);
        pct.system.push_back(20);
        pct.idle.push_back(30);

        assert_eq!(pct.series(CpuGraphSource::Total).back(), Some(&42));
        assert_eq!(pct.series(CpuGraphSource::User).back(), Some(&10));
        assert_eq!(pct.series(CpuGraphSource::System).back(), Some(&20));
        // Auto resolves to total — same series as Total.
        assert_eq!(pct.series(CpuGraphSource::Auto).back(), Some(&42));
    }

    #[test]
    fn default_cpu_info_has_zero_cores() {
        let info = CpuInfo::default();
        assert_eq!(info.core_count, 0);
        assert!(info.core_percent.is_empty());
    }
}
