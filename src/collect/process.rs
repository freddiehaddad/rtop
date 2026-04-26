use crate::domain::process::{PriorityClass, ProcInfo, ProcState};
use std::collections::HashMap;

/// Process data collector using Windows APIs.
pub struct ProcCollector {
    /// Raw collected process data — never sorted/filtered in place.
    pub procs: Vec<ProcInfo>,
    /// Derived display list — sorted, filtered, tree-prefixed.
    /// Rebuilt from `procs` whenever PROC_LIST dirty flag is set.
    pub display_procs: Vec<ProcInfo>,
    prev_times: HashMap<u32, (u64, u64)>, // pid → (kernel_time, user_time)
    last_collect: std::time::Instant,
}

impl ProcCollector {
    pub fn new() -> Self {
        Self {
            procs: Vec::new(),
            display_procs: Vec::new(),
            prev_times: HashMap::new(),
            last_collect: std::time::Instant::now(),
        }
    }

    /// Rebuild `display_procs` from raw `procs` by sorting, filtering, and
    /// optionally building tree prefixes.
    pub fn rebuild_display(&mut self, sort_by: &str, reversed: bool, filter: &str, tree_mode: bool) {
        self.display_procs = self.procs.clone();
        sort_procs(&mut self.display_procs, sort_by, reversed);
        if !filter.is_empty() {
            self.display_procs.retain(|p| matches_filter(p, filter));
        }
        if tree_mode {
            let children = build_tree(&self.display_procs);
            generate_tree_prefixes(&mut self.display_procs, &children);
        }
    }

    /// Collect process list.
    pub fn collect(&mut self, core_count: usize) -> &Vec<ProcInfo> {
        use windows::Win32::System::Diagnostics::ToolHelp::*;
        use windows::Win32::Foundation::*;

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_collect).as_secs_f64().max(0.001);
        self.last_collect = now;

        let mut new_procs = Vec::new();

        unsafe {
            let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(h) => h,
                Err(_) => return &self.procs,
            };

            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            if Process32FirstW(snapshot, &mut entry).is_ok() {
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
                        prefix: String::new(),
                        depth: 0,
                        tree_index: 0,
                    });

                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }

            let _ = CloseHandle(snapshot);
        }

        // Update prev_times for next delta
        self.prev_times.clear();
        for p in &new_procs {
            self.prev_times.insert(p.pid, (0, p.cpu_time));
        }

        self.procs = new_procs;
        &self.procs
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

    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        );

        if let Ok(handle) = handle {
            // CPU times
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user_time = FILETIME::default();

            if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user_time)
                .is_ok()
            {
                let kt = filetime_to_u64(&kernel);
                let ut = filetime_to_u64(&user_time);
                cpu_time = kt + ut;

                if let Some(&(_, prev_total)) = prev_times.get(&pid) {
                    let delta = cpu_time.saturating_sub(prev_total);
                    let system_delta = (elapsed * 10_000_000.0) as u64;
                    if system_delta > 0 {
                        cpu_p = (delta as f64 / system_delta as f64) * 100.0
                            * core_count as f64;
                    }
                }
            }

            // Memory
            use windows::Win32::System::ProcessStatus::*;
            let mut mem_counters = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            if GetProcessMemoryInfo(
                handle,
                &mut mem_counters,
                mem_counters.cb,
            )
            .is_ok()
            {
                mem = mem_counters.WorkingSetSize as u64;
            }

            // Priority
            let pclass = GetPriorityClass(handle);
            priority = priority_from_u32(pclass);

            // Username
            user = get_process_user(handle);

            // Command line
            cmd = get_process_cmdline(pid);

            let _ = CloseHandle(handle);
        }
    }

    (cpu_p, cpu_time, mem, priority, user, cmd)
}

fn get_process_user(handle: windows::Win32::Foundation::HANDLE) -> String {
    use windows::Win32::Security::*;
    use windows::Win32::Foundation::*;

    unsafe {
        let mut token = HANDLE::default();
        // Use raw FFI for OpenProcessToken
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
        }

        if OpenProcessToken(handle, 0x0008 /* TOKEN_QUERY */, &mut token) == 0 {
            return String::new();
        }

        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);

        if size == 0 {
            let _ = CloseHandle(token);
            return String::new();
        }

        let mut buffer = vec![0u8; size as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            size,
            &mut size,
        )
        .is_err()
        {
            let _ = CloseHandle(token);
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
            let _ = CloseHandle(token);
            return String::from_utf16_lossy(&name_buf[..name_len as usize]);
        }

        let _ = CloseHandle(token);
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

    unsafe {
        // Need PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
        let handle = match OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        ) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };

        // Get PEB address
        let mut pbi = std::mem::zeroed::<ProcessBasicInformation>();
        let mut ret_len: u32 = 0;
        let status = NtQueryInformationProcess(
            handle,
            0, // ProcessBasicInformation
            &mut pbi as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut ret_len,
        );
        if status != 0 || pbi.peb_base_address == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        // Read ProcessParameters pointer from PEB (offset 0x20 on 64-bit)
        let params_ptr_addr = pbi.peb_base_address + 0x20;
        let mut params_ptr: usize = 0;
        let mut bytes_read: usize = 0;

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
            handle,
            params_ptr_addr,
            &mut params_ptr as *mut usize as *mut std::ffi::c_void,
            std::mem::size_of::<usize>(),
            &mut bytes_read,
        ) == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        // Read CommandLine UNICODE_STRING from RTL_USER_PROCESS_PARAMETERS
        // Offset 0x70 on 64-bit: UNICODE_STRING { Length: u16, MaxLength: u16, Pad: u32, Buffer: *mut u16 }
        let cmdline_offset = params_ptr + 0x70;
        let mut cmd_length: u16 = 0;
        if ReadProcessMemory(
            handle,
            cmdline_offset,
            &mut cmd_length as *mut u16 as *mut std::ffi::c_void,
            2,
            &mut bytes_read,
        ) == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        let mut cmd_buffer_ptr: usize = 0;
        if ReadProcessMemory(
            handle,
            cmdline_offset + 8, // skip Length(2) + MaxLength(2) + Pad(4)
            &mut cmd_buffer_ptr as *mut usize as *mut std::ffi::c_void,
            std::mem::size_of::<usize>(),
            &mut bytes_read,
        ) == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        if cmd_length == 0 || cmd_buffer_ptr == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        // Read the actual command line string
        let char_count = (cmd_length as usize) / 2;
        let mut cmd_buf = vec![0u16; char_count];
        if ReadProcessMemory(
            handle,
            cmd_buffer_ptr,
            cmd_buf.as_mut_ptr() as *mut std::ffi::c_void,
            cmd_length as usize,
            &mut bytes_read,
        ) == 0 {
            let _ = CloseHandle(handle);
            return String::new();
        }

        let _ = CloseHandle(handle);
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

#[cfg(test)]
/// Calculate CPU percentage from process time delta (for unit testing).
pub fn cpu_percent_from_times(
    prev_total: u64,
    curr_total: u64,
    elapsed_secs: f64,
    core_count: usize,
) -> f64 {
    let delta = curr_total.saturating_sub(prev_total);
    let system_delta = (elapsed_secs * 10_000_000.0) as u64;
    if system_delta == 0 {
        return 0.0;
    }
    (delta as f64 / system_delta as f64) * 100.0 * core_count as f64
}

/// Build a process tree from PPID relationships.
pub fn build_tree(procs: &[ProcInfo]) -> HashMap<u32, Vec<usize>> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, p) in procs.iter().enumerate() {
        children.entry(p.ppid).or_default().push(i);
    }
    children
}

/// Generate tree prefix strings for display.
pub fn generate_tree_prefixes(procs: &mut [ProcInfo], children: &HashMap<u32, Vec<usize>>) {
    // Find root processes (ppid not in process list)
    let pids: std::collections::HashSet<u32> = procs.iter().map(|p| p.pid).collect();
    let roots: Vec<usize> = procs
        .iter()
        .enumerate()
        .filter(|(_, p)| !pids.contains(&p.ppid))
        .map(|(i, _)| i)
        .collect();

    let mut tree_index = 0;
    for &root_idx in &roots {
        assign_prefix(procs, children, root_idx, "", true, &mut tree_index);
    }
}

fn assign_prefix(
    procs: &mut [ProcInfo],
    children: &HashMap<u32, Vec<usize>>,
    idx: usize,
    header: &str,
    is_last: bool,
    tree_index: &mut usize,
) {
    let pid = procs[idx].pid;
    procs[idx].prefix = header.to_string();
    procs[idx].tree_index = *tree_index;
    *tree_index += 1;

    if let Some(child_indices) = children.get(&pid) {
        let len = child_indices.len();
        for (i, &child_idx) in child_indices.iter().enumerate() {
            let last = i == len - 1;
            let connector = if last { "└─ " } else { "├─ " };
            let new_header = if is_last {
                format!("{}   ", header)
            } else {
                format!("{}│  ", header)
            };
            procs[child_idx].depth = procs[idx].depth + 1;
            let prefix = format!("{}{}", new_header, connector);
            procs[child_idx].prefix = prefix;
            assign_prefix(procs, children, child_idx, &new_header, last, tree_index);
        }
    }
}

/// Sort processes by the given column.
pub fn sort_procs(procs: &mut [ProcInfo], sort_by: &str, reverse: bool) {
    procs.sort_by(|a, b| {
        let cmp = match sort_by {
            "pid" => a.pid.cmp(&b.pid),
            "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            "command" => a.cmd.to_lowercase().cmp(&b.cmd.to_lowercase()),
            "threads" => a.threads.cmp(&b.threads),
            "user" => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
            "memory" => a.mem.cmp(&b.mem),
            "cpu direct" | "cpu lazy" => a.cpu_p.partial_cmp(&b.cpu_p).unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        };
        if reverse { cmp.reverse() } else { cmp }
    });
}

/// Test if a process matches a filter string.
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
            ProcInfo { pid: 1, ppid: 0, ..Default::default() },
            ProcInfo { pid: 2, ppid: 1, ..Default::default() },
            ProcInfo { pid: 3, ppid: 1, ..Default::default() },
            ProcInfo { pid: 4, ppid: 2, ..Default::default() },
        ];
        let tree = build_tree(&procs);
        assert_eq!(tree.get(&1).unwrap().len(), 2); // pid 1 has 2 children
        assert_eq!(tree.get(&2).unwrap().len(), 1); // pid 2 has 1 child
    }

    #[test]
    fn sort_by_cpu() {
        let mut procs = vec![
            ProcInfo { pid: 1, cpu_p: 10.0, ..Default::default() },
            ProcInfo { pid: 2, cpu_p: 50.0, ..Default::default() },
            ProcInfo { pid: 3, cpu_p: 5.0, ..Default::default() },
        ];
        sort_procs(&mut procs, "cpu lazy", false);
        assert_eq!(procs[0].pid, 3);
        assert_eq!(procs[2].pid, 2);
    }

    #[test]
    fn sort_by_memory() {
        let mut procs = vec![
            ProcInfo { pid: 1, mem: 1000, ..Default::default() },
            ProcInfo { pid: 2, mem: 5000, ..Default::default() },
            ProcInfo { pid: 3, mem: 100, ..Default::default() },
        ];
        sort_procs(&mut procs, "memory", true); // Reverse = descending
        assert_eq!(procs[0].pid, 2);
        assert_eq!(procs[2].pid, 3);
    }

    #[test]
    fn sort_by_name() {
        let mut procs = vec![
            ProcInfo { pid: 1, name: "chrome.exe".into(), ..Default::default() },
            ProcInfo { pid: 2, name: "Acrobat.exe".into(), ..Default::default() },
            ProcInfo { pid: 3, name: "zsh.exe".into(), ..Default::default() },
        ];
        sort_procs(&mut procs, "name", false);
        assert_eq!(procs[0].name, "Acrobat.exe");
        assert_eq!(procs[2].name, "zsh.exe");
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
        let mut procs = vec![
            ProcInfo { pid: 1, ppid: 0, depth: 0, ..Default::default() },
            ProcInfo { pid: 2, ppid: 1, depth: 0, ..Default::default() },
            ProcInfo { pid: 3, ppid: 1, depth: 0, ..Default::default() },
        ];
        let tree = build_tree(&procs);
        generate_tree_prefixes(&mut procs, &tree);
        // Root has no prefix
        assert_eq!(procs[0].depth, 0);
        // Children have depth 1
        assert_eq!(procs[1].depth, 1);
        assert_eq!(procs[2].depth, 1);
    }

    #[test]
    #[ignore]
    fn collect_returns_current_process() {
        let mut collector = ProcCollector::new();
        collector.collect(1);
        let current_pid = std::process::id();
        assert!(collector.procs.iter().any(|p| p.pid == current_pid));
    }
}
