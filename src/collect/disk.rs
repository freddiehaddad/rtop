use crate::domain::disk::{DiskData, DiskInfo};

/// Disk data collector using Windows APIs.
pub struct DiskCollector {
    /// Collected disk data.
    pub data: DiskData,
}

impl Default for DiskCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskCollector {
    /// Create a new disk collector.
    pub fn new() -> Self {
        Self {
            data: DiskData::default(),
        }
    }

    /// Collect information for all fixed and removable drives.
    pub fn collect(&mut self) -> &DiskData {
        use windows::Win32::Storage::FileSystem::*;
        use windows::core::*;

        self.data.disks.clear();
        self.data.disks_order.clear();

        unsafe {
            let mut buf = [0u16; 512];
            let len = GetLogicalDriveStringsW(Some(&mut buf));
            if len == 0 {
                return &self.data;
            }

            let drives_str = String::from_utf16_lossy(&buf[..len as usize]);
            for drive in drives_str.split('\0').filter(|s| !s.is_empty()) {
                let drive_w: Vec<u16> = drive.encode_utf16().chain(std::iter::once(0)).collect();
                let drive_type = GetDriveTypeW(PCWSTR(drive_w.as_ptr()));

                // Only fixed and removable drives
                if drive_type != 3 && drive_type != 2 {
                    continue;
                }

                let mut free_bytes = 0u64;
                let mut total_bytes = 0u64;
                let mut total_free_bytes = 0u64;

                if GetDiskFreeSpaceExW(
                    PCWSTR(drive_w.as_ptr()),
                    Some(&mut free_bytes),
                    Some(&mut total_bytes),
                    Some(&mut total_free_bytes),
                )
                .is_ok()
                {
                    let used = total_bytes.saturating_sub(free_bytes);
                    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;

                    let mut vol_name = [0u16; 256];
                    let mut fs_name = [0u16; 32];
                    let _ = GetVolumeInformationW(
                        PCWSTR(drive_w.as_ptr()),
                        Some(&mut vol_name),
                        None,
                        None,
                        None,
                        Some(&mut fs_name),
                    );

                    let fstype = String::from_utf16_lossy(&fs_name)
                        .trim_end_matches('\0')
                        .to_string();

                    let name = drive.trim_end_matches('\\').to_string();

                    let disk = DiskInfo {
                        name: name.clone(),
                        fstype,
                        total: total_bytes,
                        used,
                        used_percent: used_pct,
                    };

                    self.data.disks_order.push(name.clone());
                    self.data.disks.insert(name, disk);
                }
            }
        }
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_collector_new_is_empty() {
        let c = DiskCollector::new();
        assert!(c.data.disks.is_empty());
        assert!(c.data.disks_order.is_empty());
    }

    #[test]
    fn collect_finds_at_least_one_drive() {
        let mut c = DiskCollector::new();
        c.collect();
        assert!(!c.data.disks.is_empty(), "expected at least one disk");
        assert!(!c.data.disks_order.is_empty());
        // C: should exist on any Windows system
        assert!(c.data.disks.contains_key("C:"), "expected C: drive");
    }
}
