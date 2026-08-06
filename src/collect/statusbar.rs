//! Collects statusbar data at a fixed [`STATUSBAR_UPDATE_MS`] cadence.
//!
//! Currently snapshots system uptime from
//! [`crate::tools::system_uptime_secs`] (Win32 `GetTickCount64`).

use super::Collector;

/// Fixed statusbar collection interval. The clock's seconds digit
/// advances at wall-clock cadence; uptime stays in sync.
pub const STATUSBAR_UPDATE_MS: u64 = 1000;

/// Statusbar snapshot data, cached at 1 Hz.
#[derive(Debug, Clone, Default)]
pub struct StatusbarInfo {
    /// System uptime in seconds since boot.
    pub uptime_seconds: u64,
}

/// Statusbar collector. Stateless apart from the most recent snapshot.
///
/// `GetTickCount64` is infallible — there is no degraded mode to
/// surface — so the collector and snapshot carry no `CollectStatus`.
pub struct StatusbarCollector {
    pub info: StatusbarInfo,
}

impl StatusbarCollector {
    pub fn new() -> Self {
        Self {
            info: StatusbarInfo::default(),
        }
    }
}

impl Collector for StatusbarCollector {
    type Snapshot = crate::runner::StatusbarSnapshot;

    fn collect(&mut self) {
        self.info.uptime_seconds = crate::tools::system_uptime_secs();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::StatusbarSnapshot {
            info: self.info.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_publishes_nonzero_uptime() {
        // Any process that has begun running has a non-zero uptime
        // by the time the test executes.
        let mut c = StatusbarCollector::new();
        c.collect();
        assert!(c.info.uptime_seconds > 0);
    }

    #[test]
    fn collect_is_monotonic_across_two_cycles() {
        let mut c = StatusbarCollector::new();
        c.collect();
        let first = c.info.uptime_seconds;
        // Uptime cannot decrease across collection cycles.
        std::thread::sleep(std::time::Duration::from_millis(50));
        c.collect();
        assert!(c.info.uptime_seconds >= first);
    }

    #[test]
    fn statusbar_update_ms_is_one_second() {
        // Pin the contract so a future edit can't silently drift
        // the cadence.
        assert_eq!(STATUSBAR_UPDATE_MS, 1000);
    }
}
