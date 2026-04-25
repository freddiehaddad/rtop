use std::collections::VecDeque;
use std::fmt;

/// Information about a single process.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    /// Process ID.
    pub pid: u32,
    /// Process name (e.g. "chrome.exe").
    pub name: String,
    /// Full command line.
    pub cmd: String,
    /// Shortened command for display.
    pub short_cmd: String,
    /// Thread count.
    pub threads: usize,
    /// Username of the process owner.
    pub user: String,
    /// Memory usage in bytes (working set).
    pub mem: u64,
    /// Current CPU usage percentage (may exceed 100% if per-core).
    pub cpu_p: f64,
    /// Cumulative CPU usage percentage.
    pub cpu_c: f64,
    /// Current process state.
    pub state: ProcState,
    /// Process priority class.
    pub priority: PriorityClass,
    /// Parent process ID.
    pub ppid: u32,
    /// Process start time (FILETIME as u64, 100ns intervals since 1601).
    pub start_time: u64,
    /// Total CPU time consumed (100ns intervals).
    pub cpu_time: u64,
    /// Total IO bytes read.
    pub io_read: u64,
    /// Total IO bytes written.
    pub io_write: u64,
    /// Tree view prefix string (e.g. "├─ ", "│  └─ ").
    pub prefix: String,
    /// Depth in process tree (0 = root).
    pub depth: usize,
    /// Index in flattened tree list.
    pub tree_index: usize,
    /// Whether this tree node is collapsed.
    pub collapsed: bool,
    /// Whether this process is hidden by the current filter.
    pub filtered: bool,
}

impl Default for ProcInfo {
    fn default() -> Self {
        Self {
            pid: 0,
            name: String::new(),
            cmd: String::new(),
            short_cmd: String::new(),
            threads: 0,
            user: String::new(),
            mem: 0,
            cpu_p: 0.0,
            cpu_c: 0.0,
            state: ProcState::Unknown,
            priority: PriorityClass::Normal,
            ppid: 0,
            start_time: 0,
            cpu_time: 0,
            io_read: 0,
            io_write: 0,
            prefix: String::new(),
            depth: 0,
            tree_index: 0,
            collapsed: false,
            filtered: false,
        }
    }
}

/// Process state on Windows.
///
/// Unlike Linux (R/S/D/Z/T/t/X/K/W/P), Windows has fewer observable states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Running,
    Suspended,
    NotResponding,
    Unknown,
}

impl fmt::Display for ProcState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Suspended => write!(f, "Suspended"),
            Self::NotResponding => write!(f, "Not Responding"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Windows process priority class.
///
/// Replaces Unix nice values (-20..19) with 6 discrete classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityClass {
    Idle = 0,
    BelowNormal = 1,
    Normal = 2,
    AboveNormal = 3,
    High = 4,
    Realtime = 5,
}

impl fmt::Display for PriorityClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::BelowNormal => write!(f, "Below Normal"),
            Self::Normal => write!(f, "Normal"),
            Self::AboveNormal => write!(f, "Above Normal"),
            Self::High => write!(f, "High"),
            Self::Realtime => write!(f, "Realtime"),
        }
    }
}

/// Detailed view data for a selected process.
#[derive(Debug, Clone)]
pub struct DetailContainer {
    /// PID of the last detailed process.
    pub last_pid: u32,
    /// Full process entry.
    pub entry: ProcInfo,
    /// Formatted elapsed time since start.
    pub elapsed: String,
    /// Parent process name.
    pub parent: String,
    /// Status display string.
    pub status: String,
    /// Formatted IO read total.
    pub io_read: String,
    /// Formatted IO write total.
    pub io_write: String,
    /// Formatted memory usage.
    pub memory: String,
    /// CPU usage history for the detailed graph.
    pub cpu_percent: VecDeque<i64>,
    /// Memory usage history (bytes) for the detailed graph.
    pub mem_bytes: VecDeque<i64>,
}

impl Default for DetailContainer {
    fn default() -> Self {
        Self {
            last_pid: 0,
            entry: ProcInfo::default(),
            elapsed: String::new(),
            parent: String::new(),
            status: String::new(),
            io_read: String::new(),
            io_write: String::new(),
            memory: String::new(),
            cpu_percent: VecDeque::new(),
            mem_bytes: VecDeque::new(),
        }
    }
}

/// Process sort options (matching btop's sort_vector).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcSorting {
    Pid,
    Name,
    Command,
    Threads,
    User,
    Memory,
    CpuDirect,
    CpuLazy,
}

impl ProcSorting {
    pub const ALL: &[Self] = &[
        Self::Pid,
        Self::Name,
        Self::Command,
        Self::Threads,
        Self::User,
        Self::Memory,
        Self::CpuDirect,
        Self::CpuLazy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Name => "name",
            Self::Command => "command",
            Self::Threads => "threads",
            Self::User => "user",
            Self::Memory => "memory",
            Self::CpuDirect => "cpu direct",
            Self::CpuLazy => "cpu lazy",
        }
    }
}

/// Process actions available on Windows (replaces Unix signals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessAction {
    /// Gracefully ask the process to close (WM_CLOSE to main window).
    EndTask,
    /// Forcefully terminate the process.
    Terminate,
    /// Suspend all threads.
    Suspend,
    /// Resume all threads.
    Resume,
}

impl fmt::Display for ProcessAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndTask => write!(f, "End Task"),
            Self::Terminate => write!(f, "Terminate"),
            Self::Suspend => write!(f, "Suspend"),
            Self::Resume => write!(f, "Resume"),
        }
    }
}

/// A node in the process tree used during tree construction.
#[derive(Debug, Clone)]
pub struct TreeProc {
    /// Index into the flat process list.
    pub proc_index: usize,
    /// Child nodes.
    pub children: Vec<TreeProc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_state_display_names() {
        assert_eq!(ProcState::Running.to_string(), "Running");
        assert_eq!(ProcState::Suspended.to_string(), "Suspended");
        assert_eq!(ProcState::NotResponding.to_string(), "Not Responding");
        assert_eq!(ProcState::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn priority_class_ordering() {
        assert!(PriorityClass::Idle < PriorityClass::BelowNormal);
        assert!(PriorityClass::BelowNormal < PriorityClass::Normal);
        assert!(PriorityClass::Normal < PriorityClass::AboveNormal);
        assert!(PriorityClass::AboveNormal < PriorityClass::High);
        assert!(PriorityClass::High < PriorityClass::Realtime);
    }

    #[test]
    fn priority_class_display() {
        assert_eq!(PriorityClass::Normal.to_string(), "Normal");
        assert_eq!(PriorityClass::Realtime.to_string(), "Realtime");
        assert_eq!(PriorityClass::Idle.to_string(), "Idle");
    }

    #[test]
    fn detail_container_default() {
        let detail = DetailContainer::default();
        assert_eq!(detail.last_pid, 0);
        assert!(detail.cpu_percent.is_empty());
        assert!(detail.mem_bytes.is_empty());
    }

    #[test]
    fn proc_sorting_labels() {
        assert_eq!(ProcSorting::Pid.label(), "pid");
        assert_eq!(ProcSorting::CpuLazy.label(), "cpu lazy");
        assert_eq!(ProcSorting::ALL.len(), 8);
    }

    #[test]
    fn process_action_display() {
        assert_eq!(ProcessAction::EndTask.to_string(), "End Task");
        assert_eq!(ProcessAction::Terminate.to_string(), "Terminate");
        assert_eq!(ProcessAction::Suspend.to_string(), "Suspend");
        assert_eq!(ProcessAction::Resume.to_string(), "Resume");
    }
}
