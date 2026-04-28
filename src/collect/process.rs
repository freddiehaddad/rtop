use crate::domain::process::{PriorityClass, ProcDisplayEntry, ProcInfo, ProcState};
use std::collections::{HashMap, HashSet};

use super::{
    Collector,
    win::{CounterDelta, OwnedHandle, counter_delta},
};

/// Canonical sort options for the process list.
/// Used by app.rs keybinds, options_menu.rs browsable values, and display sorting.
pub const SORT_OPTIONS: &[&str] = &[
    "pid",
    "name",
    "command",
    "threads",
    "user",
    "memory",
    "cpu lazy",
    "cpu direct",
];

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

/// Process data collector using Windows APIs.
pub struct ProcCollector {
    /// Raw collected process data — never sorted/filtered in place.
    pub procs: Vec<ProcInfo>,
    pub status: super::CollectStatus,
    prev_times: HashMap<u32, (u64, u64)>, // pid → (kernel_time, user_time)
    last_collect: std::time::Instant,
    core_count: usize,
}

impl Default for ProcCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcCollector {
    /// Create a new process collector.
    pub fn new() -> Self {
        Self {
            procs: Vec::new(),
            status: super::CollectStatus::Ok,
            prev_times: HashMap::new(),
            last_collect: std::time::Instant::now(),
            core_count: 1,
        }
    }

    /// Set the core count used for CPU percentage calculation.
    pub fn set_core_count(&mut self, count: usize) {
        self.core_count = count;
    }

    fn collect_impl(&mut self) {
        self.status = super::CollectStatus::Ok;

        use windows::Win32::System::Diagnostics::ToolHelp::*;

        let now = std::time::Instant::now();
        let elapsed = now
            .duration_since(self.last_collect)
            .as_secs_f64()
            .max(0.001);
        self.last_collect = now;

        let core_count = self.core_count;
        let mut new_procs = Vec::new();

        // SAFETY: CreateToolhelp32Snapshot returns a valid handle (checked via
        // Err). PROCESSENTRY32W has dwSize set correctly. Process32FirstW and
        // Process32NextW iterate the snapshot using the OS-managed list. The
        // snapshot handle is closed by OwnedHandle after iteration.
        unsafe {
            let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(h) => match OwnedHandle::new(h) {
                    Some(handle) => handle,
                    None => {
                        tracing::warn!("Process: CreateToolhelp32Snapshot returned invalid handle");
                        self.status
                            .downgrade(super::CollectStatus::Failed("snapshot failed"));
                        return;
                    }
                },
                Err(_) => {
                    tracing::warn!("Process: CreateToolhelp32Snapshot failed");
                    self.status
                        .downgrade(super::CollectStatus::Failed("snapshot failed"));
                    return;
                }
            };

            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            if Process32FirstW(snapshot.get(), &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(
                        &entry.szExeFile[..entry
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(entry.szExeFile.len())],
                    );

                    let pid = entry.th32ProcessID;
                    let ppid = entry.th32ParentProcessID;
                    let threads = entry.cntThreads as usize;

                    // Get process times and memory
                    let (cpu_p, cpu_time, mem, priority, user, cmdline) =
                        get_process_details(pid, &self.prev_times, elapsed, core_count);

                    new_procs.push(ProcInfo {
                        pid,
                        name: name.clone(),
                        cmd: if cmdline.is_empty() { name } else { cmdline },
                        threads,
                        user,
                        mem,
                        cpu_p,
                        state: ProcState::Running,
                        priority,
                        ppid,
                        cpu_time,
                        io_read: 0,
                        io_write: 0,
                    });

                    if Process32NextW(snapshot.get(), &mut entry).is_err() {
                        break;
                    }
                }
            }
        }

        // Update prev_times for next delta
        self.prev_times.clear();
        for p in &new_procs {
            self.prev_times.insert(p.pid, (0, p.cpu_time));
        }

        self.procs = new_procs;
    }
}

/// Build process display entries from raw process data and current view settings.
pub fn build_proc_display_entries(
    procs: &[ProcInfo],
    sort_by: &str,
    reversed: bool,
    filter: &str,
    tree_mode: bool,
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
        build_tree_display_entries(procs, &indices)
    } else {
        indices.into_iter().map(ProcDisplayEntry::flat).collect()
    }
}

impl Collector for ProcCollector {
    fn collect(&mut self) {
        self.collect_impl();
    }
}

fn get_process_details(
    pid: u32,
    prev_times: &HashMap<u32, (u64, u64)>,
    elapsed: f64,
    core_count: usize,
) -> (f64, u64, u64, PriorityClass, String, String) {
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Threading::*;

    let mut cpu_p = 0.0;
    let mut cpu_time = 0u64;
    let mut mem = 0u64;
    let mut priority = PriorityClass::Normal;
    let mut user = String::new();
    let mut cmd = String::new();

    // SAFETY: OpenProcess is called with limited query rights. The returned
    // handle is checked before use. FILETIME and PROCESS_MEMORY_COUNTERS are
    // stack-allocated with correct sizes. All API return values are checked.
    // The handle is closed by OwnedHandle on all paths.
    unsafe {
        if let Some(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .ok()
            .and_then(OwnedHandle::new)
        {
            // CPU times
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user_time = FILETIME::default();

            if GetProcessTimes(
                handle.get(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user_time,
            )
            .is_ok()
            {
                let kt = filetime_to_u64(&kernel);
                let ut = filetime_to_u64(&user_time);
                cpu_time = kt.saturating_add(ut);

                if let Some(&(_, prev_total)) = prev_times.get(&pid) {
                    cpu_p = process_cpu_percent(prev_total, cpu_time, elapsed, core_count);
                }
            }

            // Memory
            use windows::Win32::System::ProcessStatus::*;
            let mut mem_counters = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            if GetProcessMemoryInfo(handle.get(), &mut mem_counters, mem_counters.cb).is_ok() {
                mem = mem_counters.WorkingSetSize as u64;
            }

            // Priority
            let pclass = GetPriorityClass(handle.get());
            priority = priority_from_u32(pclass);

            // Username
            user = get_process_user(handle.get());

            // Command line
            cmd = get_process_cmdline(pid);
        }
    }

    (cpu_p, cpu_time, mem, priority, user, cmd)
}

fn get_process_user(handle: windows::Win32::Foundation::HANDLE) -> String {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::*;

    // SAFETY: The process handle is valid (passed from caller). Token handle
    // lifetime is scoped to this function and closed by OwnedHandle. Buffer
    // size is queried first, then allocated accordingly. TOKEN_USER is cast
    // from a buffer that was filled by GetTokenInformation with verified size.
    unsafe {
        let mut raw_token = HANDLE::default();
        // SAFETY: FFI declaration for advapi32 OpenProcessToken; signature
        // matches the Windows API with a process handle, access mask, and
        // output token handle pointer.
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
        }

        if OpenProcessToken(handle, 0x0008 /* TOKEN_QUERY */, &mut raw_token) == 0 {
            return String::new();
        }
        let Some(token) = OwnedHandle::new(raw_token) else {
            return String::new();
        };

        let mut size = 0u32;
        let _ = GetTokenInformation(token.get(), TokenUser, None, 0, &mut size);

        if size == 0 {
            return String::new();
        }

        let mut buffer = vec![0u8; size as usize];
        if GetTokenInformation(
            token.get(),
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            size,
            &mut size,
        )
        .is_err()
        {
            return String::new();
        }

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let sid = token_user.User.Sid;

        let mut name_buf = [0u16; 256];
        let mut domain_buf = [0u16; 256];
        let mut name_len = name_buf.len() as u32;
        let mut domain_len = domain_buf.len() as u32;
        let mut sid_type = SID_NAME_USE::default();

        if LookupAccountSidW(
            None,
            sid,
            Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
            &mut name_len,
            Some(windows::core::PWSTR(domain_buf.as_mut_ptr())),
            &mut domain_len,
            &mut sid_type,
        )
        .is_ok()
        {
            return String::from_utf16_lossy(&name_buf[..name_len as usize]);
        }
    }
    String::new()
}

/// Read the full command line of a process via NtQueryInformationProcess.
/// Returns empty string if access is denied or the process is protected.
fn get_process_cmdline(pid: u32) -> String {
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Threading::*;

    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: usize,
        peb_base_address: usize,
        reserved2: [usize; 2],
        unique_process_id: usize,
        reserved3: usize,
    }

    // SAFETY: FFI declaration for ntdll NtQueryInformationProcess; signature
    // matches the NT API with correctly typed parameters.
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process: HANDLE,
            info_class: u32,
            info: *mut std::ffi::c_void,
            info_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    // SAFETY: OpenProcess is called with VM_READ rights; the handle is checked.
    // NtQueryInformationProcess fills a properly-sized repr(C) struct.
    // ReadProcessMemory calls pass valid local buffer pointers and check return
    // values before proceeding. All reads use sizes derived from the target
    // process's own data structures (PEB offsets for 64-bit Windows). The
    // handle is closed by OwnedHandle on all exit paths.
    unsafe {
        // Need PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
        let Some(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
            .ok()
            .and_then(OwnedHandle::new)
        else {
            return String::new();
        };

        // Get PEB address
        let mut pbi = std::mem::zeroed::<ProcessBasicInformation>();
        let mut ret_len: u32 = 0;
        let status = NtQueryInformationProcess(
            handle.get(),
            0, // ProcessBasicInformation
            &mut pbi as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut ret_len,
        );
        if status != 0 || pbi.peb_base_address == 0 {
            return String::new();
        }

        // Read ProcessParameters pointer from PEB (offset 0x20 on 64-bit)
        let params_ptr_addr = pbi.peb_base_address + 0x20;
        let mut params_ptr: usize = 0;
        let mut bytes_read: usize = 0;

        // SAFETY: FFI declaration for kernel32 ReadProcessMemory; signature
        // matches the Windows API with a process handle, remote address,
        // local buffer, size, and bytes-read output pointer.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn ReadProcessMemory(
                process: HANDLE,
                base: usize,
                buffer: *mut std::ffi::c_void,
                size: usize,
                bytes_read: *mut usize,
            ) -> i32;
        }

        if ReadProcessMemory(
            handle.get(),
            params_ptr_addr,
            &mut params_ptr as *mut usize as *mut std::ffi::c_void,
            std::mem::size_of::<usize>(),
            &mut bytes_read,
        ) == 0
        {
            return String::new();
        }

        // Read CommandLine UNICODE_STRING from RTL_USER_PROCESS_PARAMETERS
        // Offset 0x70 on 64-bit: UNICODE_STRING { Length: u16, MaxLength: u16, Pad: u32, Buffer: *mut u16 }
        let cmdline_offset = params_ptr + 0x70;
        let mut cmd_length: u16 = 0;
        if ReadProcessMemory(
            handle.get(),
            cmdline_offset,
            &mut cmd_length as *mut u16 as *mut std::ffi::c_void,
            2,
            &mut bytes_read,
        ) == 0
        {
            return String::new();
        }

        let mut cmd_buffer_ptr: usize = 0;
        if ReadProcessMemory(
            handle.get(),
            cmdline_offset + 8, // skip Length(2) + MaxLength(2) + Pad(4)
            &mut cmd_buffer_ptr as *mut usize as *mut std::ffi::c_void,
            std::mem::size_of::<usize>(),
            &mut bytes_read,
        ) == 0
        {
            return String::new();
        }

        if cmd_length == 0 || cmd_buffer_ptr == 0 {
            return String::new();
        }

        // Read the actual command line string
        let char_count = (cmd_length as usize) / 2;
        let mut cmd_buf = vec![0u16; char_count];
        if ReadProcessMemory(
            handle.get(),
            cmd_buffer_ptr,
            cmd_buf.as_mut_ptr() as *mut std::ffi::c_void,
            cmd_length as usize,
            &mut bytes_read,
        ) == 0
        {
            return String::new();
        }

        String::from_utf16_lossy(&cmd_buf).trim().to_string()
    }
}

fn filetime_to_u64(ft: &windows::Win32::Foundation::FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Convert Windows priority class constant to PriorityClass enum.
pub fn priority_from_u32(pclass: u32) -> PriorityClass {
    match pclass {
        0x00000040 => PriorityClass::Idle,
        0x00004000 => PriorityClass::BelowNormal,
        0x00000020 => PriorityClass::Normal,
        0x00008000 => PriorityClass::AboveNormal,
        0x00000080 => PriorityClass::High,
        0x00000100 => PriorityClass::Realtime,
        _ => PriorityClass::Normal,
    }
}

fn process_cpu_percent(
    prev_total: u64,
    curr_total: u64,
    elapsed_secs: f64,
    core_count: usize,
) -> f64 {
    if elapsed_secs <= 0.0 || !elapsed_secs.is_finite() || core_count == 0 {
        return 0.0;
    }

    let CounterDelta::Delta(delta) = counter_delta(curr_total, prev_total) else {
        return 0.0;
    };

    let system_delta = (elapsed_secs * 10_000_000.0).clamp(0.0, u64::MAX as f64) as u64;
    if system_delta == 0 {
        return 0.0;
    }
    (delta as f64 / system_delta as f64) * 100.0 * core_count as f64
}

#[cfg(test)]
/// Calculate CPU percentage from process time delta (for unit testing).
pub fn cpu_percent_from_times(
    prev_total: u64,
    curr_total: u64,
    elapsed_secs: f64,
    core_count: usize,
) -> f64 {
    process_cpu_percent(prev_total, curr_total, elapsed_secs, core_count)
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
pub fn sort_proc_indices(indices: &mut [usize], procs: &[ProcInfo], sort_by: &str, reverse: bool) {
    indices.sort_by(|&a_idx, &b_idx| {
        let cmp = match (procs.get(a_idx), procs.get(b_idx)) {
            (Some(a), Some(b)) => compare_procs(a, b, sort_by),
            _ => a_idx.cmp(&b_idx),
        };
        if reverse { cmp.reverse() } else { cmp }
    });
}

fn compare_procs(a: &ProcInfo, b: &ProcInfo, sort_by: &str) -> std::cmp::Ordering {
    match sort_by {
        "pid" => a.pid.cmp(&b.pid),
        "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        "command" => a.cmd.to_lowercase().cmp(&b.cmd.to_lowercase()),
        "threads" => a.threads.cmp(&b.threads),
        "user" => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
        "memory" => a.mem.cmp(&b.mem),
        "cpu direct" | "cpu lazy" => a
            .cpu_p
            .partial_cmp(&b.cpu_p)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
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
    fn cpu_percent_from_times_basic() {
        let pct = cpu_percent_from_times(0, 10_000_000, 1.0, 1);
        assert!((pct - 100.0).abs() < 1.0);
    }

    #[test]
    fn cpu_percent_from_times_multicore() {
        let pct = cpu_percent_from_times(0, 10_000_000, 1.0, 4);
        assert!((pct - 400.0).abs() < 1.0);
    }

    #[test]
    fn cpu_percent_zero_elapsed() {
        assert_eq!(cpu_percent_from_times(0, 100, 0.0, 1), 0.0);
    }

    #[test]
    fn cpu_percent_zero_after_counter_reset() {
        assert_eq!(cpu_percent_from_times(200, 100, 1.0, 1), 0.0);
    }

    #[test]
    fn priority_class_from_u32_values() {
        assert_eq!(priority_from_u32(0x00000040), PriorityClass::Idle);
        assert_eq!(priority_from_u32(0x00000020), PriorityClass::Normal);
        assert_eq!(priority_from_u32(0x00000080), PriorityClass::High);
        assert_eq!(priority_from_u32(0x00000100), PriorityClass::Realtime);
        assert_eq!(priority_from_u32(0), PriorityClass::Normal); // Unknown → Normal
    }

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
        sort_proc_indices(&mut indices, &procs, "cpu lazy", false);
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
        sort_proc_indices(&mut indices, &procs, "memory", true); // Reverse = descending
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
        sort_proc_indices(&mut indices, &procs, "name", false);
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

        let entries = build_proc_display_entries(&procs, "cpu lazy", true, "", false);

        let pids: Vec<u32> = entries
            .iter()
            .filter_map(|entry| procs.get(entry.proc_index))
            .map(|proc| proc.pid)
            .collect();
        assert_eq!(pids, vec![2, 3, 1]);
    }

    #[test]
    fn collect_returns_current_process() {
        let mut collector = ProcCollector::new();
        collector.set_core_count(1);
        collector.collect();
        let current_pid = std::process::id();
        assert!(collector.procs.iter().any(|p| p.pid == current_pid));
    }
}
