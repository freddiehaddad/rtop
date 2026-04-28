use std::collections::VecDeque;

/// Information about a single disk/volume.
#[derive(Debug, Clone, Default)]
pub struct DiskInfo {
    /// Display name (e.g. "C:", "D:").
    pub name: String,
    /// Filesystem type (e.g. "NTFS", "FAT32", "ReFS").
    pub fstype: String,
    /// Total capacity in bytes.
    pub total: u64,
    /// Used space in bytes.
    pub used: u64,
    /// Used percentage (0-100).
    pub used_percent: i32,
    /// Current read throughput in bytes/sec.
    pub read_bytes_per_sec: u64,
    /// Current write throughput in bytes/sec.
    pub write_bytes_per_sec: u64,
    /// Highest observed read throughput in bytes/sec.
    pub read_top: u64,
    /// Highest observed write throughput in bytes/sec.
    pub write_top: u64,
    /// Recent read throughput history in bytes/sec.
    pub read_history: VecDeque<i64>,
    /// Recent write throughput history in bytes/sec.
    pub write_history: VecDeque<i64>,
    /// Current disk active/busy time percentage (0-100).
    pub busy_percent: i32,
}

/// Aggregated disk data for all detected volumes.
#[derive(Debug, Clone, Default)]
pub struct DiskData {
    /// Disk information in display order.
    pub disks: Vec<DiskInfo>,
}

impl DiskData {
    /// Look up a disk by name (mutable).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut DiskInfo> {
        self.disks.iter_mut().find(|d| d.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_info_percentages_valid_range() {
        let disk = DiskInfo {
            total: 1_000_000,
            used: 600_000,
            used_percent: 60,
            ..Default::default()
        };
        assert!((0..=100).contains(&disk.used_percent));
    }

    #[test]
    fn disk_info_perf_defaults_are_empty() {
        let disk = DiskInfo::default();
        assert_eq!(disk.read_bytes_per_sec, 0);
        assert_eq!(disk.write_bytes_per_sec, 0);
        assert_eq!(disk.busy_percent, 0);
        assert!(disk.read_history.is_empty());
        assert!(disk.write_history.is_empty());
    }

    #[test]
    fn disk_data_default_is_empty() {
        let data = DiskData::default();
        assert!(data.disks.is_empty());
    }
}
