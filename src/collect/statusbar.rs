//! Statusbar subsystem collector.
//!
//! Snapshots the values the borderless statusbar widget consumes
//! at a fixed [`STATUSBAR_UPDATE_MS`] cadence:
//!
//! * `info.uptime_seconds` — system uptime in seconds, sourced
//!   from [`crate::tools::system_uptime_secs`] (Win32
//!   `GetTickCount64`).
//!
//! Driven by the standard [`crate::collect::Collector`] /
//! [`crate::runner::CollectorManager`] pipeline like every other
//! subsystem; the cadence is hardcoded here because the user
//! requirement is "fixed at 1 second" and `RefreshConfig` is
//! intentionally not extended for it.

use super::Collector;

/// Hardcoded statusbar collection interval. The clock's seconds
/// digit advances at wall-clock cadence; uptime stays in sync.
/// If a future change wants user-configurable cadence, replace
/// this constant with a `RefreshConfig::statusbar_update_ms` lookup
/// at the spawn site in `runner.rs::CollectorManager::start`.
pub const STATUSBAR_UPDATE_MS: u64 = 1000;

/// Statusbar snapshot data. Today only carries uptime; if future
/// statusbar items need additional pre-computed values they belong
/// here (cached at 1 Hz, pulled by the renderer at frame time).
#[derive(Debug, Clone, Default)]
pub struct StatusbarInfo {
    /// System uptime in seconds since boot.
    pub uptime_seconds: u64,
}

/// Statusbar subsystem collector. Stateless apart from the most
/// recent snapshot; implements [`Collector`] for participation in
/// the standard collector loop.
///
/// `GetTickCount64` is infallible — there is no degraded mode to
/// surface — so the collector does not carry a `CollectStatus`
/// field. The snapshot type ([`crate::runner::StatusbarSnapshot`])
/// matches: no `status`, just the cached uptime.
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
        // Sleep a short interval then re-collect; uptime cannot
        // decrease.
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
