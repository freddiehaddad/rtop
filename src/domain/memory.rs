use std::collections::{HashMap, VecDeque};

/// System memory and swap usage information.
#[derive(Debug, Clone)]
pub struct MemInfo {
    /// Memory statistics in bytes. Keys: "used", "available", "cached", "free",
    /// "swap_total", "swap_used", "swap_free".
    pub stats: HashMap<String, u64>,
    /// Percentage histories for each memory category (0-100).
    pub percent: HashMap<String, VecDeque<i64>>,
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
            percent: keys
                .iter()
                .map(|k| (k.to_string(), VecDeque::new()))
                .collect(),
        }
    }
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
}
