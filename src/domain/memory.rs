use std::collections::VecDeque;

/// Memory statistics in bytes.
#[derive(Debug, Clone, Default)]
pub struct MemStats {
    pub used: u64,
    pub available: u64,
    pub cached: u64,
    pub free: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub swap_free: u64,
}

/// Percentage histories for each memory category (0-100).
#[derive(Debug, Clone, Default)]
pub struct MemPercent {
    pub used: VecDeque<i64>,
    pub available: VecDeque<i64>,
    pub cached: VecDeque<i64>,
    pub free: VecDeque<i64>,
    pub swap_used: VecDeque<i64>,
    pub swap_free: VecDeque<i64>,
}

/// System memory and swap usage information.
#[derive(Debug, Clone, Default)]
pub struct MemInfo {
    /// Memory statistics in bytes.
    pub stats: MemStats,
    /// Percentage histories for each memory category (0-100).
    pub percent: MemPercent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_info_default_has_zero_stats() {
        let mem = MemInfo::default();
        assert_eq!(mem.stats.used, 0);
        assert_eq!(mem.stats.available, 0);
        assert_eq!(mem.stats.cached, 0);
        assert_eq!(mem.stats.free, 0);
        assert_eq!(mem.stats.swap_total, 0);
        assert_eq!(mem.stats.swap_used, 0);
        assert_eq!(mem.stats.swap_free, 0);
    }

    #[test]
    fn mem_info_default_has_empty_percent() {
        let mem = MemInfo::default();
        assert!(mem.percent.used.is_empty());
        assert!(mem.percent.available.is_empty());
        assert!(mem.percent.cached.is_empty());
        assert!(mem.percent.free.is_empty());
        assert!(mem.percent.swap_used.is_empty());
        assert!(mem.percent.swap_free.is_empty());
    }
}
