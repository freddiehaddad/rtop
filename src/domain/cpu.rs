use std::collections::{HashMap, VecDeque};

/// CPU usage data for all cores and aggregate statistics.
///
/// On Windows, `cpu_percent` contains these keys:
///   - "total", "user", "system", "idle", "irq", "dpc"
/// Linux-only keys ("nice","iowait","softirq","steal","guest","guest_nice")
/// are not populated.
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// Aggregate CPU usage histories by category.
    pub cpu_percent: HashMap<String, VecDeque<i64>>,
    /// Per-core usage history (index = core number).
    pub core_percent: Vec<VecDeque<i64>>,
    /// Per-sensor temperature history in °C (index 0 = package, 1+ = cores).
    pub temp: Vec<VecDeque<i64>>,
    /// Critical temperature threshold (°C).
    pub temp_max: i64,
    /// Emulated load average (1, 5, 15 minute rolling EMA).
    pub load_avg: [f64; 3],
    /// CPU power consumption in watts (0.0 if unavailable).
    pub usage_watts: f32,
    /// CPU model name (e.g. "Intel Core i9-13900K").
    pub cpu_name: String,
    /// Current CPU frequency as display string (e.g. "5.80 GHz").
    pub cpu_hz: String,
    /// Number of logical cores.
    pub core_count: usize,
    /// Whether a battery is present.
    pub has_battery: bool,
    /// Battery status and charge information.
    pub battery: BatteryInfo,
    /// System uptime in seconds.
    pub uptime_seconds: u64,
}

impl Default for CpuInfo {
    fn default() -> Self {
        let keys = ["total", "user", "system", "idle", "irq", "dpc"];
        Self {
            cpu_percent: keys.iter().map(|k| (k.to_string(), VecDeque::new())).collect(),
            core_percent: Vec::new(),
            temp: Vec::new(),
            temp_max: 100,
            load_avg: [0.0; 3],
            usage_watts: 0.0,
            cpu_name: String::new(),
            cpu_hz: String::new(),
            core_count: 0,
            has_battery: false,
            battery: BatteryInfo::default(),
            uptime_seconds: 0,
        }
    }
}

/// Battery status information.
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    /// Battery charge percentage (0-100), or -1 if unavailable.
    pub percent: i32,
    /// Discharge/charge rate in watts.
    pub watts: f32,
    /// Estimated seconds remaining (-1 if unknown).
    pub seconds_remaining: i64,
    /// Status string: "Charging", "Discharging", "Full", "No Battery".
    pub status: String,
    /// Whether AC power is connected.
    pub ac_connected: bool,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            percent: -1,
            watts: 0.0,
            seconds_remaining: -1,
            status: "No Battery".to_string(),
            ac_connected: true,
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

    #[test]
    fn battery_info_default_is_no_battery() {
        let bat = BatteryInfo::default();
        assert_eq!(bat.percent, -1);
        assert_eq!(bat.status, "No Battery");
        assert!(bat.ac_connected);
        assert_eq!(bat.seconds_remaining, -1);
    }
}
