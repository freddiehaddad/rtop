use std::collections::{HashMap, VecDeque};

/// System memory, swap, and disk usage information.
#[derive(Debug, Clone)]
pub struct MemInfo {
    /// Memory statistics in bytes. Keys: "used", "available", "cached", "free",
    /// "swap_total", "swap_used", "swap_free".
    pub stats: HashMap<String, u64>,
    /// Percentage histories for each memory category (0-100).
    pub percent: HashMap<String, VecDeque<i64>>,
    /// Disk information keyed by drive/mount name (e.g. "C:").
    pub disks: HashMap<String, DiskInfo>,
    /// Ordered list of disk names for display.
    pub disks_order: Vec<String>,
}

impl Default for MemInfo {
    fn default() -> Self {
        let keys = [
            "used",
            "available",
            "cached",
            "free",
            "swap_total",
            "swap_used",
            "swap_free",
        ];
        Self {
            stats: keys.iter().map(|k| (k.to_string(), 0u64)).collect(),
            percent: keys.iter().map(|k| (k.to_string(), VecDeque::new())).collect(),
            disks: HashMap::new(),
            disks_order: Vec::new(),
        }
    }
}

/// Information about a single disk/volume.
#[derive(Debug, Clone)]
#[allow(dead_code)] // domain model — fields populated by collector
#[derive(Default)]
pub struct DiskInfo {
    /// Display name (e.g. "C:", "D:").
    pub name: String,
    /// Volume label (e.g. "Windows", "Data").
    pub label: String,
    /// Filesystem type (e.g. "NTFS", "FAT32", "ReFS").
    pub fstype: String,
    /// Total capacity in bytes.
    pub total: u64,
    /// Used space in bytes.
    pub used: u64,
    /// Free space in bytes.
    pub free: u64,
    /// Used percentage (0-100).
    pub used_percent: i32,
    /// Free percentage (0-100).
    pub free_percent: i32,
    /// IO read bytes per update history.
    pub io_read: VecDeque<i64>,
    /// IO write bytes per update history.
    pub io_write: VecDeque<i64>,
    /// IO activity percentage history (0-100).
    pub io_activity: VecDeque<i64>,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_info_contains_all_stat_keys() {
        let mem = MemInfo::default();
        for key in &[
            "used",
            "available",
            "cached",
            "free",
            "swap_total",
            "swap_used",
            "swap_free",
        ] {
            assert!(mem.stats.contains_key(*key), "missing stat key: {key}");
            assert!(mem.percent.contains_key(*key), "missing percent key: {key}");
        }
    }

    #[test]
    fn disk_info_percentages_valid_range() {
        let disk = DiskInfo {
            total: 1_000_000,
            used: 600_000,
            free: 400_000,
            used_percent: 60,
            free_percent: 40,
            ..Default::default()
        };
        assert!((0..=100).contains(&disk.used_percent));
        assert!((0..=100).contains(&disk.free_percent));
        assert_eq!(disk.used_percent + disk.free_percent, 100);
    }
}
