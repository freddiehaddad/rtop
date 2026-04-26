use std::collections::HashMap;

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
}

/// Aggregated disk data for all detected volumes.
#[derive(Debug, Clone, Default)]
pub struct DiskData {
    /// Disk information keyed by drive/mount name (e.g. "C:").
    pub disks: HashMap<String, DiskInfo>,
    /// Ordered list of disk names for consistent display order.
    pub disks_order: Vec<String>,
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
    fn disk_data_default_is_empty() {
        let data = DiskData::default();
        assert!(data.disks.is_empty());
        assert!(data.disks_order.is_empty());
    }
}
