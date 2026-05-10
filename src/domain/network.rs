use std::collections::VecDeque;

/// Bandwidth history for download and upload.
#[derive(Debug, Clone, Default)]
pub struct NetBandwidth {
    pub download: VecDeque<i64>,
    pub upload: VecDeque<i64>,
}

/// Cumulative statistics for download and upload.
#[derive(Debug, Copy, Clone, Default)]
pub struct NetStatPair {
    pub download: NetStat,
    pub upload: NetStat,
}

/// Network interface statistics.
#[derive(Debug, Clone, Default)]
pub struct NetInfo {
    /// Interface display name.
    pub name: String,
    /// Bandwidth history (values in bytes/sec).
    pub bandwidth: NetBandwidth,
    /// Cumulative statistics.
    pub stat: NetStatPair,
    /// IPv4 address of the interface.
    pub ipv4: String,
    /// IPv6 address of the interface.
    pub ipv6: String,
    /// Whether the interface is connected/operational.
    pub connected: bool,
    /// Link speed in bits per second.
    pub link_speed: u64,
}

/// Cumulative transfer statistics for one direction (download or upload).
#[derive(Debug, Copy, Clone, Default)]
pub struct NetStat {
    /// Current speed in bytes/sec.
    pub speed: u64,
    /// Peak speed in bytes/sec.
    pub top: u64,
    /// Last raw counter value from OS.
    pub last: u64,
    /// Offset for manual total reset.
    pub offset: u64,
    /// Accumulated bytes from counter rollovers.
    pub rollover: u64,
}

impl NetStat {
    /// Total bytes transferred in this direction since the last
    /// reset (or since program start if `z` has never been pressed).
    ///
    /// `last + rollover - offset`, saturating on underflow. Right
    /// after `z` zeroes the displayed total, `offset` equals
    /// `last + rollover`; then `last` updates as the OS counter
    /// continues. If the OS counter ever moves backwards (rollover
    /// or interface re-binding), `saturating_sub` keeps the
    /// displayed value at zero rather than wrapping.
    pub fn displayed_total(&self) -> u64 {
        self.last
            .saturating_add(self.rollover)
            .saturating_sub(self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_stat_default_zeroed() {
        let stat = NetStat::default();
        assert_eq!(stat.speed, 0);
        assert_eq!(stat.top, 0);
        assert_eq!(stat.last, 0);
        assert_eq!(stat.offset, 0);
        assert_eq!(stat.rollover, 0);
    }

    #[test]
    fn net_info_default_has_empty_bandwidth() {
        let info = NetInfo::default();
        assert!(info.bandwidth.download.is_empty());
        assert!(info.bandwidth.upload.is_empty());
        assert_eq!(info.stat.download.speed, 0);
        assert_eq!(info.stat.upload.speed, 0);
    }

    #[test]
    fn displayed_total_with_no_offset_returns_last_plus_rollover() {
        let stat = NetStat {
            last: 1_000,
            rollover: 200,
            ..NetStat::default()
        };
        assert_eq!(stat.displayed_total(), 1_200);
    }

    #[test]
    fn displayed_total_subtracts_offset() {
        let stat = NetStat {
            last: 5_000,
            rollover: 100,
            offset: 3_000,
            ..NetStat::default()
        };
        assert_eq!(stat.displayed_total(), 2_100);
    }

    #[test]
    fn displayed_total_saturates_when_offset_exceeds_counters() {
        // Right after `z` zeroes totals, offset == last + rollover.
        // If the OS counter then moves backward (e.g. interface
        // re-bind), `last + rollover` < `offset`. Displayed total
        // must stay at 0, not wrap.
        let stat = NetStat {
            last: 100,
            rollover: 0,
            offset: 5_000,
            ..NetStat::default()
        };
        assert_eq!(stat.displayed_total(), 0);
    }
}
