use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

// ---------------------------------------------------------------------------
// ProcSort — typed sort key for the process list
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("invalid ProcSort value '{0}'")]
pub struct ParseProcSortError(pub String);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProcSort {
    Pid,
    Name,
    Command,
    Threads,
    User,
    Memory,
    #[default]
    CpuLazy,
    CpuDirect,
}

impl ProcSort {
    pub const ALL: &'static [ProcSort] = &[
        ProcSort::Pid,
        ProcSort::Name,
        ProcSort::Command,
        ProcSort::Threads,
        ProcSort::User,
        ProcSort::Memory,
        ProcSort::CpuLazy,
        ProcSort::CpuDirect,
    ];

    pub const NAMES: &'static [&'static str] = &[
        "pid",
        "name",
        "command",
        "threads",
        "user",
        "memory",
        "cpu lazy",
        "cpu direct",
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ProcSort::Pid => "pid",
            ProcSort::Name => "name",
            ProcSort::Command => "command",
            ProcSort::Threads => "threads",
            ProcSort::User => "user",
            ProcSort::Memory => "memory",
            ProcSort::CpuLazy => "cpu lazy",
            ProcSort::CpuDirect => "cpu direct",
        }
    }
}

impl fmt::Display for ProcSort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProcSort {
    type Err = ParseProcSortError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProcSort::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseProcSortError(s.to_string()))
    }
}

impl Serialize for ProcSort {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProcSort {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <String>::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Pre-parsed process filter for efficient per-process matching.
pub enum ParsedFilter {
    None,
    /// Case-insensitive substring match (already lowercased).
    Substring(String),
    /// Regex match (from `!pattern` syntax).
    Regex(regex::Regex),
}

impl ParsedFilter {
    /// Parse a raw filter string into a `ParsedFilter`.
    pub fn parse(filter: &str) -> Self {
        if filter.is_empty() {
            Self::None
        } else if let Some(pattern) = filter.strip_prefix('!') {
            match regex::Regex::new(pattern) {
                Ok(re) => Self::Regex(re),
                Err(_) => Self::None,
            }
        } else {
            Self::Substring(filter.to_lowercase())
        }
    }

    /// Return true if the given process matches this filter.
    pub fn matches(&self, proc: &ProcInfo) -> bool {
        match self {
            Self::None => true,
            Self::Substring(s) => {
                proc.name.to_lowercase().contains(s.as_str())
                    || proc.cmd.to_lowercase().contains(s.as_str())
            }
            Self::Regex(re) => re.is_match(&proc.name) || re.is_match(&proc.cmd),
        }
    }
}

/// Build process display entries from raw process data and current view settings.
pub fn build_proc_display_entries(
    procs: &[ProcInfo],
    sort_by: ProcSort,
    reversed: bool,
    filter: &str,
    tree_mode: bool,
    aggregate: bool,
) -> Vec<ProcDisplayEntry> {
    let parsed = ParsedFilter::parse(filter);
    let mut indices: Vec<usize> = procs
        .iter()
        .enumerate()
        .filter(|(_, proc)| filter.is_empty() || parsed.matches(proc))
        .map(|(idx, _)| idx)
        .collect();
    sort_proc_indices(&mut indices, procs, sort_by, reversed);

    if tree_mode {
        let entries = build_tree_display_entries(procs, &indices);
        if aggregate {
            aggregate_tree_entries(procs, &entries)
        } else {
            entries
        }
    } else {
        indices.into_iter().map(ProcDisplayEntry::flat).collect()
    }
}

/// Build a parent-PID-to-raw-process-indices map for process tree display.
pub fn build_tree(procs: &[ProcInfo], indices: &[usize]) -> HashMap<u32, Vec<usize>> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for &idx in indices {
        if let Some(proc) = procs.get(idx) {
            children.entry(proc.ppid).or_default().push(idx);
        }
    }
    children
}

/// Build flattened tree display entries from sorted raw process indices.
pub fn build_tree_display_entries(
    procs: &[ProcInfo],
    sorted_indices: &[usize],
) -> Vec<ProcDisplayEntry> {
    let pids: HashSet<u32> = sorted_indices
        .iter()
        .filter_map(|&idx| procs.get(idx).map(|proc| proc.pid))
        .collect();
    let children = build_tree(procs, sorted_indices);
    let roots: Vec<usize> = sorted_indices
        .iter()
        .copied()
        .filter(|&idx| {
            procs
                .get(idx)
                .is_some_and(|proc| !pids.contains(&proc.ppid))
        })
        .collect();

    let mut entries = Vec::with_capacity(sorted_indices.len());
    {
        let mut builder = TreeDisplayBuilder {
            procs,
            children: &children,
            entries: &mut entries,
        };
        for &root_idx in &roots {
            builder.push(root_idx, "", "", 0, true);
        }
    }
    entries
}

struct TreeDisplayBuilder<'a, 'b> {
    procs: &'a [ProcInfo],
    children: &'a HashMap<u32, Vec<usize>>,
    entries: &'b mut Vec<ProcDisplayEntry>,
}

impl TreeDisplayBuilder<'_, '_> {
    fn push(&mut self, idx: usize, prefix: &str, child_header: &str, depth: usize, is_last: bool) {
        let Some(proc) = self.procs.get(idx) else {
            return;
        };

        self.entries
            .push(ProcDisplayEntry::tree(idx, prefix.to_string(), depth));

        if let Some(child_indices) = self.children.get(&proc.pid) {
            let len = child_indices.len();
            for (i, &child_idx) in child_indices.iter().enumerate() {
                let last = i == len - 1;
                let connector = if last { "└─ " } else { "├─ " };
                let next_child_header = if is_last {
                    format!("{child_header}   ")
                } else {
                    format!("{child_header}│  ")
                };
                let child_prefix = format!("{next_child_header}{connector}");
                self.push(
                    child_idx,
                    &child_prefix,
                    &next_child_header,
                    depth + 1,
                    last,
                );
            }
        }
    }
}

/// Sort raw process indices by the given process column.
pub fn sort_proc_indices(
    indices: &mut [usize],
    procs: &[ProcInfo],
    sort_by: ProcSort,
    reverse: bool,
) {
    indices.sort_by(|&a_idx, &b_idx| {
        let cmp = match (procs.get(a_idx), procs.get(b_idx)) {
            (Some(a), Some(b)) => compare_procs(a, b, sort_by),
            _ => a_idx.cmp(&b_idx),
        };
        if reverse { cmp.reverse() } else { cmp }
    });
}

/// Aggregate child CPU% and memory into parent entries in tree mode.
///
/// Walks the flat tree list bottom-up so that children are accumulated before
/// their parents. Only modifies display overrides, not the raw data.
fn aggregate_tree_entries(
    procs: &[ProcInfo],
    entries: &[ProcDisplayEntry],
) -> Vec<ProcDisplayEntry> {
    let mut result: Vec<ProcDisplayEntry> = entries.to_vec();
    let len = result.len();

    // Build PID→entry-index map for parent lookup.
    let mut pid_to_idx: HashMap<u32, usize> = HashMap::new();
    for (i, e) in result.iter().enumerate() {
        if let Some(p) = procs.get(e.proc_index) {
            pid_to_idx.insert(p.pid, i);
        }
    }

    // Initialize aggregates from raw values.
    let mut agg_cpu: Vec<f64> = result
        .iter()
        .map(|e| procs.get(e.proc_index).map_or(0.0, |p| p.cpu_p))
        .collect();
    let mut agg_mem: Vec<u64> = result
        .iter()
        .map(|e| procs.get(e.proc_index).map_or(0, |p| p.mem))
        .collect();

    // Walk bottom-up: accumulate each entry's values into its parent.
    for i in (0..len).rev() {
        let Some(p) = procs.get(result[i].proc_index) else {
            continue;
        };
        if let Some(&parent_idx) = pid_to_idx.get(&p.ppid) {
            agg_cpu[parent_idx] += agg_cpu[i];
            agg_mem[parent_idx] += agg_mem[i];
        }
    }

    // Only set overrides on entries that actually have children (aggregated
    // values differ from the raw ones).
    for i in 0..len {
        let raw_cpu = procs.get(result[i].proc_index).map_or(0.0, |p| p.cpu_p);
        let raw_mem = procs.get(result[i].proc_index).map_or(0, |p| p.mem);
        if (agg_cpu[i] - raw_cpu).abs() > f64::EPSILON {
            result[i].cpu_override = Some(agg_cpu[i]);
        }
        if agg_mem[i] != raw_mem {
            result[i].mem_override = Some(agg_mem[i]);
        }
    }

    result
}

fn compare_procs(a: &ProcInfo, b: &ProcInfo, sort_by: ProcSort) -> std::cmp::Ordering {
    match sort_by {
        ProcSort::Pid => a.pid.cmp(&b.pid),
        ProcSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        ProcSort::Command => a.cmd.to_lowercase().cmp(&b.cmd.to_lowercase()),
        ProcSort::Threads => a.threads.cmp(&b.threads),
        ProcSort::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
        ProcSort::Memory => a.mem.cmp(&b.mem),
        ProcSort::CpuDirect | ProcSort::CpuLazy => a
            .cpu_p
            .partial_cmp(&b.cpu_p)
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

/// Test if a process matches a filter string.
/// Return true if a process matches the filter (substring or `!regex`).
///
/// Deprecated: prefer `ParsedFilter::parse(filter).matches(proc)` to avoid
/// re-compiling the regex on every call.
#[cfg(test)]
pub fn matches_filter(proc: &ProcInfo, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    if let Some(pattern) = filter.strip_prefix('!') {
        // Regex mode
        if let Ok(re) = regex::Regex::new(pattern) {
            re.is_match(&proc.name) || re.is_match(&proc.cmd)
        } else {
            false
        }
    } else {
        // Substring match (case-insensitive)
        let lower_filter = filter.to_lowercase();
        proc.name.to_lowercase().contains(&lower_filter)
            || proc.cmd.to_lowercase().contains(&lower_filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tree_from_ppid_map() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                ppid: 0,
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                ppid: 1,
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                ppid: 1,
                ..Default::default()
            },
            ProcInfo {
                pid: 4,
                ppid: 2,
                ..Default::default()
            },
        ];
        let indices = vec![0, 1, 2, 3];
        let tree = build_tree(&procs, &indices);
        assert_eq!(tree.get(&1).unwrap().len(), 2); // pid 1 has 2 children
        assert_eq!(tree.get(&2).unwrap().len(), 1); // pid 2 has 1 child
    }

    #[test]
    fn sort_by_cpu() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                cpu_p: 10.0,
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                cpu_p: 50.0,
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                cpu_p: 5.0,
                ..Default::default()
            },
        ];
        let mut indices = vec![0, 1, 2];
        sort_proc_indices(&mut indices, &procs, ProcSort::CpuLazy, false);
        assert_eq!(procs[indices[0]].pid, 3);
        assert_eq!(procs[indices[2]].pid, 2);
    }

    #[test]
    fn sort_by_memory() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                mem: 1000,
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                mem: 5000,
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                mem: 100,
                ..Default::default()
            },
        ];
        let mut indices = vec![0, 1, 2];
        sort_proc_indices(&mut indices, &procs, ProcSort::Memory, true); // Reverse = descending
        assert_eq!(procs[indices[0]].pid, 2);
        assert_eq!(procs[indices[2]].pid, 3);
    }

    #[test]
    fn sort_by_name() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                name: "chrome.exe".into(),
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                name: "Acrobat.exe".into(),
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                name: "zsh.exe".into(),
                ..Default::default()
            },
        ];
        let mut indices = vec![0, 1, 2];
        sort_proc_indices(&mut indices, &procs, ProcSort::Name, false);
        assert_eq!(procs[indices[0]].name, "Acrobat.exe");
        assert_eq!(procs[indices[2]].name, "zsh.exe");
    }

    #[test]
    fn filter_regex_match() {
        let proc = ProcInfo {
            name: "chrome.exe".into(),
            cmd: "chrome.exe --headless".into(),
            ..Default::default()
        };
        assert!(matches_filter(&proc, "chrome"));
        assert!(matches_filter(&proc, "!chrome\\.exe"));
        assert!(!matches_filter(&proc, "firefox"));
    }

    #[test]
    fn filter_regex_negation() {
        let proc = ProcInfo {
            name: "explorer.exe".into(),
            cmd: "explorer.exe".into(),
            ..Default::default()
        };
        assert!(matches_filter(&proc, "!^explorer"));
        assert!(!matches_filter(&proc, "!^chrome"));
    }

    #[test]
    fn filter_empty_matches_all() {
        let proc = ProcInfo::default();
        assert!(matches_filter(&proc, ""));
    }

    #[test]
    fn tree_prefix_generation() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                ppid: 0,
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                ppid: 1,
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                ppid: 1,
                ..Default::default()
            },
        ];
        let entries = build_tree_display_entries(&procs, &[0, 1, 2]);
        // Root has no prefix
        assert_eq!(entries[0].depth, 0);
        assert_eq!(entries[0].prefix, "");
        // Children have depth 1
        assert_eq!(entries[1].depth, 1);
        assert_eq!(entries[2].depth, 1);
        assert_eq!(entries[1].prefix, "   ├─ ");
        assert_eq!(entries[2].prefix, "   └─ ");
    }

    #[test]
    fn build_proc_display_entries_uses_raw_indices() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                name: "alpha.exe".into(),
                cpu_p: 10.0,
                ..Default::default()
            },
            ProcInfo {
                pid: 2,
                name: "beta.exe".into(),
                cpu_p: 30.0,
                ..Default::default()
            },
            ProcInfo {
                pid: 3,
                name: "gamma.exe".into(),
                cpu_p: 20.0,
                ..Default::default()
            },
        ];

        let entries = build_proc_display_entries(&procs, ProcSort::CpuLazy, true, "", false, false);

        let pids: Vec<u32> = entries
            .iter()
            .filter_map(|entry| procs.get(entry.proc_index))
            .map(|proc| proc.pid)
            .collect();
        assert_eq!(pids, vec![2, 3, 1]);
    }
}
