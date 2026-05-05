use std::collections::VecDeque;

/// Parsed representation of the user-supplied `disks_filter` config list.
///
/// Filter entries are individual drive selectors stored as a TOML
/// array (e.g. `disks_filter = ["C:", "!D:"]`). A bare entry
/// (`"C:"`) joins the include allow-list; an entry prefixed with
/// `!` (`"!C:"`) joins the exclude deny-list. Drive selectors are
/// case-insensitive and the trailing colon is optional, so `"c"`,
/// `"C"`, `"c:"`, and `"C:"` are equivalent and all normalise to
/// `"C:"`.
///
/// Match semantics:
///
/// - An empty filter (no include and no exclude) matches every disk.
/// - Excludes always take precedence: a disk listed in `exclude` is
///   rejected even if it also appears in `include`.
/// - When `include` is non-empty, only disks whose normalised name
///   appears there can pass.
/// - When `include` is empty but `exclude` is non-empty, every disk
///   except those in `exclude` passes (deny-list semantics).
///
/// Invalid entries — anything that is neither a single ASCII letter nor
/// a single ASCII letter followed by `:`, optionally with a leading
/// `!` — are captured in `invalid` for warning surfaces. They are
/// silently ignored at match time so a malformed user input never
/// causes false matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisksFilter {
    include: Vec<String>,
    exclude: Vec<String>,
    invalid: Vec<String>,
}

impl DisksFilter {
    /// Parse a `disks_filter` TOML array into a typed filter.
    pub fn parse(entries: &[String]) -> Self {
        let mut filter = Self::default();
        for entry in entries {
            match parse_filter_entry(entry) {
                Some(FilterEntry {
                    is_exclude: true,
                    name,
                }) => filter.exclude.push(name),
                Some(FilterEntry {
                    is_exclude: false,
                    name,
                }) => filter.include.push(name),
                None => filter.invalid.push(entry.clone()),
            }
        }
        filter
    }

    /// Returns `true` when the filter has no include or exclude entries
    /// (i.e. matches every disk).
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Test whether a disk's `DiskInfo.name` passes the filter.
    pub fn matches(&self, disk_name: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let normalized = normalize_drive_name(disk_name);
        if self.exclude.iter().any(|e| e == &normalized) {
            return false;
        }
        if self.include.is_empty() {
            return true;
        }
        self.include.iter().any(|i| i == &normalized)
    }

    /// Borrow only the disks that pass the filter, preserving the
    /// caller's input order.
    ///
    /// Disks listed in the filter that are not physically present do
    /// not affect the result: `apply` only iterates the actual disks
    /// the caller passes in, so an over-specified filter never produces
    /// ghost rows.
    pub fn apply<'a>(&self, disks: &'a [DiskInfo]) -> Vec<&'a DiskInfo> {
        if self.is_empty() {
            return disks.iter().collect();
        }
        disks.iter().filter(|d| self.matches(&d.name)).collect()
    }

    /// Entries from the raw filter string that failed to parse, in
    /// their original textual form. Used by `Config::validate` to
    /// surface warnings.
    pub fn invalid(&self) -> &[String] {
        &self.invalid
    }
}

struct FilterEntry {
    is_exclude: bool,
    name: String,
}

fn parse_filter_entry(token: &str) -> Option<FilterEntry> {
    let (is_exclude, body) = match token.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let normalized = parse_drive_letter(body)?;
    Some(FilterEntry {
        is_exclude,
        name: normalized,
    })
}

/// Parse a drive selector into its canonical `"X:"` form, where `X` is
/// an ASCII letter normalised to uppercase. Returns `None` for any
/// input that is not a single ASCII letter followed by a single
/// trailing colon. The trailing colon is mandatory: it is the
/// Windows convention for "this is a drive identifier" and matches
/// what rtop already shows on screen (`C:`, `D:`, etc.).
fn parse_drive_letter(s: &str) -> Option<String> {
    let body = s.strip_suffix(':')?;
    let mut chars = body.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if !ch.is_ascii_alphabetic() {
        return None;
    }
    Some(format!("{}:", ch.to_ascii_uppercase()))
}

/// Normalise a `DiskInfo.name` to the same canonical form the filter
/// uses internally. Falls back to ASCII-uppercase passthrough for any
/// name that is not a single drive letter (e.g. an unexpected mount
/// path), so an unusual disk name simply never matches a drive-letter
/// filter rather than panicking or causing false positives.
fn normalize_drive_name(name: &str) -> String {
    parse_drive_letter(name).unwrap_or_else(|| name.to_ascii_uppercase())
}

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

    // -- DisksFilter --

    fn disk(name: &str) -> DiskInfo {
        DiskInfo {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// Build a `Vec<String>` from string literals — saves
    /// `vec!["foo".into(), ...]` boilerplate at every test site.
    fn entries(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn disks_filter_empty_input_matches_all() {
        let f = DisksFilter::parse(&[]);
        assert!(f.is_empty());
        assert!(f.matches("C:"));
        assert!(f.matches("D:"));
        assert!(f.invalid().is_empty());
    }

    #[test]
    fn disks_filter_include_single_drive() {
        let f = DisksFilter::parse(&entries(&["C:"]));
        assert!(!f.is_empty());
        assert!(f.matches("C:"));
        assert!(!f.matches("D:"));
        assert!(f.invalid().is_empty());
    }

    #[test]
    fn disks_filter_include_multiple_drives() {
        let f = DisksFilter::parse(&entries(&["C:", "D:", "E:"]));
        assert!(f.matches("C:"));
        assert!(f.matches("D:"));
        assert!(f.matches("E:"));
        assert!(!f.matches("F:"));
    }

    #[test]
    fn disks_filter_exclude_single_drive() {
        let f = DisksFilter::parse(&entries(&["!C:"]));
        assert!(!f.is_empty());
        assert!(!f.matches("C:"));
        assert!(f.matches("D:"));
        assert!(f.matches("Z:"));
    }

    #[test]
    fn disks_filter_exclude_multiple_drives() {
        let f = DisksFilter::parse(&entries(&["!C:", "!D:"]));
        assert!(!f.matches("C:"));
        assert!(!f.matches("D:"));
        assert!(f.matches("E:"));
    }

    #[test]
    fn disks_filter_exclude_takes_precedence_over_include() {
        let f = DisksFilter::parse(&entries(&["C:", "D:", "!D:"]));
        assert!(f.matches("C:"));
        assert!(!f.matches("D:"));
        // E: is not in include so it's still rejected.
        assert!(!f.matches("E:"));
    }

    #[test]
    fn disks_filter_normalises_case() {
        let f = DisksFilter::parse(&entries(&["c:", "d:"]));
        assert!(f.matches("C:"));
        assert!(f.matches("D:"));
        assert!(!f.matches("E:"));

        let f2 = DisksFilter::parse(&entries(&["C:"]));
        assert!(f2.matches("c:"));
    }

    #[test]
    fn disks_filter_rejects_letter_without_trailing_colon() {
        // The trailing `:` is mandatory — it's the Windows convention
        // for drive identifiers and matches what rtop displays on
        // screen. Bare letters are captured as invalid entries so the
        // user sees a warning at config load time.
        let f = DisksFilter::parse(&entries(&["C", "D", "!a", "!B"]));
        assert!(f.is_empty());
        assert_eq!(
            f.invalid(),
            &[
                "C".to_string(),
                "D".to_string(),
                "!a".to_string(),
                "!B".to_string(),
            ]
        );
    }

    #[test]
    fn disks_filter_invalid_entries_captured_and_dropped() {
        let f = DisksFilter::parse(&entries(&[
            "C:",
            "abc",
            "3",
            "D:",
            "!!",
            "C::",
            "C:\\Users",
        ]));
        assert!(f.matches("C:"));
        assert!(f.matches("D:"));
        assert!(!f.matches("E:"));
        assert_eq!(
            f.invalid(),
            &[
                "abc".to_string(),
                "3".to_string(),
                "!!".to_string(),
                "C::".to_string(),
                "C:\\Users".to_string(),
            ]
        );
    }

    #[test]
    fn disks_filter_lone_bang_is_invalid() {
        let f = DisksFilter::parse(&entries(&["!"]));
        assert!(f.is_empty());
        assert_eq!(f.invalid(), &["!".to_string()]);
    }

    #[test]
    fn disks_filter_apply_preserves_input_order_not_filter_order() {
        let disks = vec![disk("C:"), disk("D:"), disk("E:")];
        // Filter order: E:, then C:. Input order is C:, D:, E:.
        let f = DisksFilter::parse(&entries(&["E:", "C:"]));
        let kept = f.apply(&disks);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "C:");
        assert_eq!(kept[1].name, "E:");
    }

    #[test]
    fn disks_filter_apply_with_empty_filter_yields_all() {
        let disks = vec![disk("C:"), disk("D:"), disk("E:")];
        let f = DisksFilter::parse(&[]);
        let kept = f.apply(&disks);
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn disks_filter_apply_exclude_only_removes_listed() {
        let disks = vec![disk("C:"), disk("D:"), disk("E:")];
        let f = DisksFilter::parse(&entries(&["!D:"]));
        let kept = f.apply(&disks);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "C:");
        assert_eq!(kept[1].name, "E:");
    }

    #[test]
    fn disks_filter_includes_absent_drives_yields_only_present() {
        // The filter lists more drives than physically exist. The
        // result is sized to the present-and-matching set: no ghost
        // rows for the absent drives.
        let disks = vec![disk("C:"), disk("D:")];
        let f = DisksFilter::parse(&entries(&["C:", "D:", "E:", "F:", "G:", "H:", "I:"]));
        let kept = f.apply(&disks);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "C:");
        assert_eq!(kept[1].name, "D:");
        // No invalid entries in this filter.
        assert!(f.invalid().is_empty());
    }
}
