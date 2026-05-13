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
    /// Permanent adapter identifier from Windows. The string form
    /// of `IP_ADAPTER_ADDRESSES_LH.AdapterName` — a GUID assigned
    /// at adapter installation that is documented as "permanent and
    /// cannot be modified by the user." This is the stable key
    /// used for cycling and persistence (`RuntimeView::net_iface`,
    /// `ViewConfig::net_iface`). It survives reboots, driver
    /// updates, IP changes, and connection renames; it changes
    /// only when an adapter is uninstalled and reinstalled.
    /// Two physically distinct NICs (even if identical make/model)
    /// always have different `stable_id`s — the only field on the
    /// adapter struct that guarantees uniqueness.
    pub stable_id: String,
    /// Adapter description from the driver's INF (e.g. "Realtek
    /// PCIe GbE Family Controller", "Intel(R) Ethernet I225-V").
    /// Used as the human-readable display name in the widget chip.
    /// Two identical adapters in the same machine share the same
    /// description — uniqueness is provided by `stable_id`, not
    /// this field.
    pub description: String,
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
    /// Bytes transferred in this direction since rtop started.
    pub total: u64,
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
        assert_eq!(stat.total, 0);
    }

    #[test]
    fn net_info_default_has_empty_bandwidth() {
        let info = NetInfo::default();
        assert!(info.bandwidth.download.is_empty());
        assert!(info.bandwidth.upload.is_empty());
        assert_eq!(info.stat.download.speed, 0);
        assert_eq!(info.stat.upload.speed, 0);
    }
}
