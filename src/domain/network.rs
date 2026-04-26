use std::collections::{HashMap, VecDeque};

/// Network interface statistics.
#[derive(Debug, Clone)]
pub struct NetInfo {
    /// Bandwidth history. Keys: "download", "upload" (values in bytes/sec).
    pub bandwidth: HashMap<String, VecDeque<i64>>,
    /// Cumulative statistics. Keys: "download", "upload".
    pub stat: HashMap<String, NetStat>,
    /// IPv4 address of the interface.
    pub ipv4: String,
    /// IPv6 address of the interface.
    pub ipv6: String,
    /// Whether the interface is connected/operational.
    pub connected: bool,
}

impl Default for NetInfo {
    fn default() -> Self {
        Self {
            bandwidth: [
                ("download".into(), VecDeque::new()),
                ("upload".into(), VecDeque::new()),
            ]
            .into_iter()
            .collect(),
            stat: [
                ("download".into(), NetStat::default()),
                ("upload".into(), NetStat::default()),
            ]
            .into_iter()
            .collect(),
            ipv4: String::new(),
            ipv6: String::new(),
            connected: false,
        }
    }
}

/// Cumulative transfer statistics for one direction (download or upload).
#[derive(Debug, Clone, Default)]
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
    fn net_info_has_download_upload_keys() {
        let info = NetInfo::default();
        assert!(info.bandwidth.contains_key("download"));
        assert!(info.bandwidth.contains_key("upload"));
        assert!(info.stat.contains_key("download"));
        assert!(info.stat.contains_key("upload"));
    }
}
