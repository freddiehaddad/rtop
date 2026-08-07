use crate::domain::process::{PriorityClass, ProcInfo, ProcState};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    mem::size_of,
};
use thiserror::Error;

use super::{
    Collector,
    counters::{CounterDelta, counter_delta},
    win::OwnedHandle,
};
use windows::Win32::Foundation::{HANDLE, UNICODE_STRING};

const TOKEN_QUERY_ACCESS: u32 = 0x0008;

/// `PROCESSINFOCLASS::ProcessCommandLineInformation`. Available on Windows 8.1
/// and later; queries on Windows 8 also accept it. Returns a
/// `UNICODE_STRING` followed by the command-line bytes; `Buffer` points
/// just past the struct, inside the same allocation.
const PROCESS_COMMAND_LINE_INFORMATION_CLASS: u32 = 60;

/// `STATUS_INFO_LENGTH_MISMATCH` — reported by `NtQueryInformationProcess`
/// when the supplied buffer is too small. The required size is written
/// to `ReturnLength` regardless of return status.
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

/// Sanity cap on the buffer the kernel may request for a remote command
/// line. 64 KiB is well above any realistic command-line length and
/// matches the historical `RTL_USER_PROCESS_PARAMETERS.CommandLine`
/// upper bound (`u16::MAX` bytes).
const MAX_CMDLINE_BUFFER_BYTES: u32 = 64 * 1024;

/// `SYSTEM_INFORMATION_CLASS::SystemProcessInformation`. Returns a
/// chain of `SYSTEM_PROCESS_INFORMATION` records linked by
/// `NextEntryOffset`, each immediately followed by its
/// `NumberOfThreads` `SYSTEM_THREAD_INFORMATION` entries.
const SYSTEM_PROCESS_INFORMATION_CLASS: u32 = 5;

/// `KTHREAD_STATE::Waiting`. A suspended thread is always in this
/// state; `WaitReason` distinguishes why.
const THREAD_STATE_WAIT: u32 = 5;

/// `KWAIT_REASON::Suspended`. Paired with `THREAD_STATE_WAIT` this is
/// the signature of a thread parked by `SuspendThread` or by the
/// Windows process lifetime manager.
const WAIT_REASON_SUSPENDED: u32 = 5;

/// Starting size for the system-wide process snapshot. Large enough
/// for a typical desktop (roughly 400 processes and 5000 threads) to
/// succeed on the first call.
const SYSTEM_PROCESS_BUFFER_BYTES: usize = 512 * 1024;

/// Ceiling on the snapshot buffer. The kernel reports the size it
/// wants, but the process table can grow between the sizing call and
/// the fetch, so the loop retries; this bounds that retry.
const MAX_SYSTEM_PROCESS_BUFFER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum CmdlineReadError {
    #[error("NtQueryInformationProcess (ProcessCommandLineInformation) failed")]
    Query,
    #[error("UNICODE_STRING returned by NtQueryInformationProcess is invalid")]
    InvalidUnicodeString,
    #[error("kernel reported a buffer size larger than the cmdline cap")]
    BufferTooLarge,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationProcess(
        process: HANDLE,
        info_class: u32,
        info: *mut c_void,
        info_length: u32,
        return_length: *mut u32,
    ) -> i32;

    fn NtQuerySystemInformation(
        info_class: u32,
        info: *mut c_void,
        info_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

/// Process data collector using Windows APIs.
pub struct ProcCollector {
    /// Raw collected process data — never sorted/filtered in place.
    pub procs: Vec<ProcInfo>,
    pub status: super::CollectStatus,
    /// Per-PID kernel + user CPU ticks from the previous cycle.
    /// Used to compute the CPU% delta for the current cycle's window.
    prev_times: HashMap<u32, u64>,
    last_collect: std::time::Instant,
    core_count: usize,
}

impl ProcCollector {
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
        // One system-wide query, before walking the Toolhelp snapshot,
        // so each process can be labelled without a second syscall.
        let suspended = collect_suspended_pids();

        // SAFETY: CreateToolhelp32Snapshot returns a valid handle (checked via
        // Err). PROCESSENTRY32W has dwSize set correctly. Process32FirstW and
        // Process32NextW iterate the snapshot using the OS-managed list. The
        // snapshot handle is closed by OwnedHandle after iteration.
        unsafe {
            let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(h) => match OwnedHandle::new(h) {
                    Some(handle) => handle,
                    None => {
                        tracing::warn!(
                            subsystem = %crate::log::Subsystem::Process,
                            "CreateToolhelp32Snapshot returned invalid handle",
                        );
                        self.status
                            .downgrade(super::CollectStatus::Failed("snapshot failed"));
                        return;
                    }
                },
                Err(_) => {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Process,
                        "CreateToolhelp32Snapshot failed",
                    );
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

                    let details = get_process_details(pid, &self.prev_times, elapsed, core_count);

                    new_procs.push(ProcInfo {
                        pid,
                        name: name.clone(),
                        cmd: if details.cmd.is_empty() {
                            name
                        } else {
                            details.cmd
                        },
                        threads,
                        user: details.user,
                        mem: details.mem,
                        cpu_p: details.cpu_p,
                        state: if suspended.contains(&pid) {
                            ProcState::Suspended
                        } else {
                            ProcState::Running
                        },
                        priority: details.priority,
                        ppid,
                        cpu_time: details.cpu_time,
                        io_read: details.io_read,
                        io_write: details.io_write,
                    });

                    if Process32NextW(snapshot.get(), &mut entry).is_err() {
                        break;
                    }
                }
            }
        }

        // Update prev_times for next-cycle delta calculation.
        self.prev_times.clear();
        for p in &new_procs {
            self.prev_times.insert(p.pid, p.cpu_time);
        }

        self.procs = new_procs;
    }
}

impl Collector for ProcCollector {
    type Snapshot = crate::runner::ProcSnapshot;

    fn collect(&mut self) {
        self.collect_impl();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::ProcSnapshot {
            procs: self.procs.clone(),
            status: self.status.clone(),
        }
    }
}

/// Fetch the system-wide process/thread snapshot into an 8-byte
/// aligned buffer, growing until the kernel stops reporting
/// `STATUS_INFO_LENGTH_MISMATCH`. Returns the buffer and the number of
/// bytes the kernel actually wrote.
fn query_system_processes() -> Option<(Vec<u64>, usize)> {
    let mut capacity = SYSTEM_PROCESS_BUFFER_BYTES;

    loop {
        // `Vec<u64>` rather than `Vec<u8>`: SYSTEM_PROCESS_INFORMATION
        // contains pointers and `usize`, so the buffer must be
        // 8-byte aligned before records can be read out of it.
        let mut buf = vec![0u64; capacity.div_ceil(size_of::<u64>())];
        let byte_len = buf.len() * size_of::<u64>();
        let Ok(byte_len_u32) = u32::try_from(byte_len) else {
            return None;
        };
        let mut return_len = 0u32;

        // SAFETY: buf is an owned allocation of exactly `byte_len`
        // bytes, aligned to 8 by its u64 element type. `byte_len_u32`
        // describes that same allocation, and `return_len` is a live
        // local the kernel writes the used or required size into.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_INFORMATION_CLASS,
                buf.as_mut_ptr().cast::<c_void>(),
                byte_len_u32,
                &mut return_len,
            )
        };

        if status == STATUS_INFO_LENGTH_MISMATCH {
            // The kernel reports the size it wanted, but the process
            // table can grow again before the next call, so take the
            // larger of its answer and a doubling and retry.
            let wanted = return_len as usize;
            let next = wanted.max(capacity.saturating_mul(2));
            if next > MAX_SYSTEM_PROCESS_BUFFER_BYTES {
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Process,
                    wanted,
                    "system process snapshot exceeds the buffer cap",
                );
                return None;
            }
            capacity = next;
            continue;
        }

        if status != 0 {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Process,
                code = %crate::log::Hex(status),
                "NtQuerySystemInformation(SystemProcessInformation) failed",
            );
            return None;
        }

        let used = (return_len as usize).min(byte_len);
        return Some((buf, used));
    }
}

/// PIDs whose every thread sits in a suspended wait.
///
/// One system-wide call covers every process, which is far cheaper
/// than opening each process and querying its threads individually. A
/// process counts as suspended only when it has at least one thread
/// and all of them are parked, matching how Process Explorer and
/// Process Hacker report the state.
fn collect_suspended_pids() -> HashSet<u32> {
    use windows::Win32::System::WindowsProgramming::{
        SYSTEM_PROCESS_INFORMATION, SYSTEM_THREAD_INFORMATION,
    };

    let mut suspended = HashSet::new();
    let Some((buf, used)) = query_system_processes() else {
        return suspended;
    };

    let base = buf.as_ptr().cast::<u8>();
    let mut offset = 0usize;

    loop {
        // Every field read below lives inside the record, so the
        // record itself must fit in the bytes the kernel wrote.
        if offset.saturating_add(size_of::<SYSTEM_PROCESS_INFORMATION>()) > used {
            break;
        }

        // SAFETY: the bounds check above proves the record lies inside
        // the initialised region of `buf`, and the buffer's u64
        // element type satisfies the struct's 8-byte alignment.
        // Copying the record ends the borrow immediately, so the walk
        // below does not hold a reference into the buffer.
        let record = unsafe { *base.add(offset).cast::<SYSTEM_PROCESS_INFORMATION>() };

        let threads_offset = offset + size_of::<SYSTEM_PROCESS_INFORMATION>();
        let threads_len = (record.NumberOfThreads as usize)
            .saturating_mul(size_of::<SYSTEM_THREAD_INFORMATION>());

        if record.NumberOfThreads > 0 && threads_offset.saturating_add(threads_len) <= used {
            let pid = record.UniqueProcessId.0 as u32;
            // SAFETY: the thread array is contiguous with its record,
            // and the bounds check above proves all `NumberOfThreads`
            // entries lie inside the initialised region.
            let threads = unsafe {
                std::slice::from_raw_parts(
                    base.add(threads_offset).cast::<SYSTEM_THREAD_INFORMATION>(),
                    record.NumberOfThreads as usize,
                )
            };
            let all_parked = threads.iter().all(|t| {
                t.ThreadState == THREAD_STATE_WAIT && t.WaitReason == WAIT_REASON_SUSPENDED
            });
            if all_parked {
                suspended.insert(pid);
            }
        }

        // A zero `NextEntryOffset` marks the final record; any other
        // value advances `offset`, so the walk always terminates.
        let step = record.NextEntryOffset as usize;
        if step == 0 {
            break;
        }
        offset = offset.saturating_add(step);
    }

    suspended
}

/// Per-process values read from a single `OpenProcess` handle.
///
/// Grouped into a struct rather than returned as a tuple: the set has
/// grown past the point where positional returns are readable, and
/// every field is filled from the same handle.
struct ProcessDetails {
    cpu_p: f64,
    cpu_time: u64,
    mem: u64,
    priority: PriorityClass,
    user: String,
    cmd: String,
    /// Cumulative bytes read over the process lifetime, spanning file,
    /// network and device I/O.
    io_read: u64,
    /// Cumulative bytes written over the process lifetime, spanning
    /// file, network and device I/O.
    io_write: u64,
}

impl Default for ProcessDetails {
    fn default() -> Self {
        Self {
            cpu_p: 0.0,
            cpu_time: 0,
            mem: 0,
            priority: PriorityClass::Normal,
            user: String::new(),
            cmd: String::new(),
            io_read: 0,
            io_write: 0,
        }
    }
}

fn get_process_details(
    pid: u32,
    prev_times: &HashMap<u32, u64>,
    elapsed: f64,
    core_count: usize,
) -> ProcessDetails {
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Threading::*;

    let mut details = ProcessDetails::default();

    // SAFETY: OpenProcess is called with limited query rights. The returned
    // handle is checked before use. FILETIME, PROCESS_MEMORY_COUNTERS and
    // IO_COUNTERS are stack-allocated with correct sizes. All API return
    // values are checked. The handle is closed by OwnedHandle on all paths.
    unsafe {
        if let Some(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .ok()
            .and_then(OwnedHandle::new)
        {
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
                details.cpu_time = kt.saturating_add(ut);

                if let Some(&prev_total) = prev_times.get(&pid) {
                    details.cpu_p =
                        process_cpu_percent(prev_total, details.cpu_time, elapsed, core_count);
                }
            }

            use windows::Win32::System::ProcessStatus::*;
            let mut mem_counters = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            if GetProcessMemoryInfo(handle.get(), &mut mem_counters, mem_counters.cb).is_ok() {
                details.mem = mem_counters.WorkingSetSize as u64;
            }

            // Cumulative I/O totals. Protected processes deny this even
            // with PROCESS_QUERY_LIMITED_INFORMATION, so a failure leaves
            // the counters at zero rather than dropping the process.
            let mut io = IO_COUNTERS::default();
            if GetProcessIoCounters(handle.get(), &mut io).is_ok() {
                details.io_read = io.ReadTransferCount;
                details.io_write = io.WriteTransferCount;
            }

            let pclass = GetPriorityClass(handle.get());
            details.priority = priority_from_u32(pclass);

            details.user = get_process_user(handle.get());

            // Command line — reuse the same handle (the API is documented
            // to work with PROCESS_QUERY_LIMITED_INFORMATION on Win 10+).
            details.cmd = get_process_cmdline(handle.get());
        }
    }

    details
}

/// Read the full command line of a process via
/// `NtQueryInformationProcess(ProcessCommandLineInformation)`. Returns
/// an empty string if the query fails (access denied, protected
/// process, kernel rejection, …) so the caller falls back to the
/// process name.
fn get_process_cmdline(handle: HANDLE) -> String {
    try_get_process_cmdline(handle).unwrap_or_default()
}

fn try_get_process_cmdline(handle: HANDLE) -> Result<String, CmdlineReadError> {
    // Probe call: a null info pointer with size 0 returns
    // STATUS_INFO_LENGTH_MISMATCH along with the required size.
    let mut needed: u32 = 0;
    // SAFETY: passing a null info pointer with size 0 is the documented
    // probe shape for variable-size NtQueryInformationProcess outputs;
    // the kernel writes the required size to `needed` regardless of
    // return status.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION_CLASS,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if status != STATUS_INFO_LENGTH_MISMATCH || (needed as usize) < size_of::<UNICODE_STRING>() {
        return Err(CmdlineReadError::Query);
    }
    if needed > MAX_CMDLINE_BUFFER_BYTES {
        return Err(CmdlineReadError::BufferTooLarge);
    }

    // `Vec<u64>`-backed storage so that the kernel-supplied
    // `UNICODE_STRING.Buffer` pointer (which the kernel writes inside
    // our allocation) lands on a u16-aligned address.
    let words = (needed as usize).div_ceil(size_of::<u64>());
    let mut storage: Vec<u64> = vec![0; words];
    let buf_ptr = storage.as_mut_ptr().cast::<u8>();
    let buf_len = words * size_of::<u64>();

    let mut written: u32 = 0;
    // SAFETY: buf_ptr addresses `buf_len` >= `needed` bytes of writable,
    // u64-aligned storage owned by `storage`. `written` is a stack-
    // allocated u32. The status is checked before any field of the
    // returned UNICODE_STRING is read.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION_CLASS,
            buf_ptr.cast::<c_void>(),
            needed,
            &mut written,
        )
    };
    if status != 0 || (written as usize) < size_of::<UNICODE_STRING>() {
        return Err(CmdlineReadError::Query);
    }

    // SAFETY: the kernel wrote at least `size_of::<UNICODE_STRING>()`
    // bytes into `storage`. `UNICODE_STRING` is a POD layout (two u16,
    // padding, raw pointer); any bit pattern is well-defined for read.
    let header: UNICODE_STRING =
        unsafe { std::ptr::read_unaligned(buf_ptr.cast::<UNICODE_STRING>()) };
    let length_bytes = header.Length as usize;
    if length_bytes == 0 {
        return Ok(String::new());
    }
    if !length_bytes.is_multiple_of(size_of::<u16>()) {
        return Err(CmdlineReadError::InvalidUnicodeString);
    }

    // Validate that `header.Buffer` points inside our allocation and
    // that the claimed range is reachable.
    let alloc_start = buf_ptr as usize;
    let alloc_end = alloc_start + buf_len;
    let buffer_addr = header.Buffer.0 as usize;
    if buffer_addr < alloc_start
        || buffer_addr
            .checked_add(length_bytes)
            .is_none_or(|end| end > alloc_end)
    {
        return Err(CmdlineReadError::InvalidUnicodeString);
    }

    let units = length_bytes / size_of::<u16>();
    // SAFETY: the bounds check above proved
    // `buffer_addr .. buffer_addr + length_bytes` lies inside
    // `storage`. The address is u16-aligned because `storage` is
    // u64-aligned (8 % 2 == 0) and the kernel writes
    // `header.Buffer` immediately after the UNICODE_STRING struct,
    // whose size is a multiple of 2 on every supported architecture.
    let utf16 = unsafe { std::slice::from_raw_parts(header.Buffer.0.cast_const(), units) };
    Ok(sanitize_command_line(&String::from_utf16_lossy(utf16)))
}

fn get_process_user(handle: windows::Win32::Foundation::HANDLE) -> String {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::*;

    // SAFETY: The process handle is valid (passed from caller). Token and SID
    // lookups use API-reported buffer sizes before casting or conversion. The
    // token handle is closed by OwnedHandle on every return path.
    unsafe {
        let mut raw_token = HANDLE::default();
        if OpenProcessToken(handle, TOKEN_QUERY_ACCESS, &mut raw_token) == 0 {
            return String::new();
        }
        let Some(token) = OwnedHandle::new(raw_token) else {
            return String::new();
        };

        let mut size = 0u32;
        let _ = GetTokenInformation(token.get(), TokenUser, None, 0, &mut size);

        if (size as usize) < size_of::<TOKEN_USER>() {
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
        if (size as usize) > buffer.len() || (size as usize) < size_of::<TOKEN_USER>() {
            return String::new();
        }

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let sid = token_user.User.Sid;
        if sid.is_invalid() {
            return String::new();
        }

        let mut name_len = 0u32;
        let mut domain_len = 0u32;
        let mut sid_type = SID_NAME_USE::default();
        let _ = LookupAccountSidW(
            None,
            sid,
            None,
            &mut name_len,
            None,
            &mut domain_len,
            &mut sid_type,
        );
        if name_len == 0 {
            return String::new();
        }

        let mut name_buf = vec![0u16; name_len as usize];
        let mut domain_buf = vec![0u16; domain_len.max(1) as usize];
        let mut name_capacity = name_buf.len() as u32;
        let mut domain_capacity = domain_buf.len() as u32;

        if LookupAccountSidW(
            None,
            sid,
            Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
            &mut name_capacity,
            Some(windows::core::PWSTR(domain_buf.as_mut_ptr())),
            &mut domain_capacity,
            &mut sid_type,
        )
        .is_ok()
        {
            let name_len = (name_capacity as usize).min(name_buf.len());
            return String::from_utf16_lossy(&name_buf[..name_len]);
        }
    }
    String::new()
}

/// Replace every Unicode control character (`U+0000`–`U+001F`, `U+007F` DEL,
/// and `U+0080`–`U+009F` C1 controls) in a raw command-line string with a
/// single ASCII space, then trim leading and trailing whitespace.
///
/// Foreign processes' `RTL_USER_PROCESS_PARAMETERS.CommandLine` buffers can
/// contain embedded `NUL` units (commonly between argv elements for
/// COM-launched services such as `WmiPrvSE.exe`). Those characters are
/// zero-width on terminals and would corrupt downstream display alignment,
/// regex matching, sort order, and the detail panel; `ESC` would also
/// trample our own ANSI rendering. Replace them all defensively before the
/// rest of the program ever sees them.
fn sanitize_command_line(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned.trim().to_string()
}

fn filetime_to_u64(ft: &windows::Win32::Foundation::FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

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
    let pct = (delta as f64 / system_delta as f64) * 100.0;
    if !pct.is_finite() {
        return 0.0;
    }
    pct.clamp(0.0, 100.0 * core_count as f64)
}

#[cfg(test)]
pub fn cpu_percent_from_times(
    prev_total: u64,
    curr_total: u64,
    elapsed_secs: f64,
    core_count: usize,
) -> f64 {
    process_cpu_percent(prev_total, curr_total, elapsed_secs, core_count)
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
        assert!((pct - 100.0).abs() < 1.0);
    }

    #[test]
    fn cpu_percent_from_times_all_cores() {
        let pct = cpu_percent_from_times(0, 40_000_000, 1.0, 4);
        assert!((pct - 400.0).abs() < 1.0);
    }

    #[test]
    fn cpu_percent_clamps_to_core_capacity() {
        let pct = cpu_percent_from_times(0, 80_000_000, 1.0, 4);
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
    fn sanitize_command_line_replaces_nul_with_space() {
        assert_eq!(
            sanitize_command_line("wmiprvse.exe\0-secured\0-Embedding"),
            "wmiprvse.exe -secured -Embedding"
        );
    }

    #[test]
    fn sanitize_command_line_replaces_other_c0_controls() {
        // ESC, BEL, SOH, US — would otherwise corrupt ANSI rendering or alignment.
        assert_eq!(sanitize_command_line("a\x01b\x07c\x1bd\x1fe"), "a b c d e");
    }

    #[test]
    fn sanitize_command_line_replaces_tab_cr_lf() {
        assert_eq!(sanitize_command_line("a\tb\rc\nd"), "a b c d");
    }

    #[test]
    fn sanitize_command_line_replaces_del() {
        assert_eq!(sanitize_command_line("a\x7fb"), "a b");
    }

    #[test]
    fn sanitize_command_line_trims_ends() {
        assert_eq!(
            sanitize_command_line("\0\0  cmd.exe -arg  \0\0"),
            "cmd.exe -arg"
        );
    }

    #[test]
    fn sanitize_command_line_leaves_normal_ascii_unchanged() {
        let s = "C:\\Program Files\\app.exe --flag value";
        assert_eq!(sanitize_command_line(s), s);
    }

    #[test]
    fn sanitize_command_line_preserves_unicode_text() {
        let s = "C:\\項目\\app.exe --名前 値";
        assert_eq!(sanitize_command_line(s), s);
    }

    #[test]
    fn sanitize_command_line_does_not_collapse_runs_of_spaces() {
        // Two NULs become two spaces; intentional spacing is preserved.
        assert_eq!(sanitize_command_line("a\0\0b"), "a  b");
    }

    #[test]
    fn sanitize_command_line_output_is_control_char_free() {
        // The renderer relies on `proc.cmd` being free of control characters
        // (NUL/ESC/CR/LF/TAB/DEL/C1) so that `format!("{:<W$}", cmd)` pads to
        // the same number of terminal columns it pads bytes — the original
        // alignment bug was caused by NULs being zero-width on terminals
        // while consuming a slot in the format-width budget. This is the
        // contract test for that prerequisite.
        let raw = "exe\0arg1\x01\x07\x1b\x1ftab\there\rcr\nlf\x7fdel\u{0085}c1";
        let cleaned = sanitize_command_line(raw);
        assert!(
            cleaned.chars().all(|c| !c.is_control()),
            "sanitized command line still contains control chars: {cleaned:?}"
        );
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
    fn collect_returns_current_process() {
        let mut collector = ProcCollector::new();
        collector.set_core_count(1);
        collector.collect();
        let current_pid = std::process::id();
        assert!(collector.procs.iter().any(|p| p.pid == current_pid));
    }

    #[test]
    fn detects_a_process_whose_threads_are_all_suspended() {
        // CREATE_SUSPENDED starts the child with its only thread
        // parked, which is exactly the state the collector looks for.
        // Using a real child rather than a fixture exercises the whole
        // path: the NtQuerySystemInformation call, the record walk and
        // the all-threads-parked rule.
        use std::os::windows::process::CommandExt;
        const CREATE_SUSPENDED: u32 = 0x0000_0004;

        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "exit"])
            .creation_flags(CREATE_SUSPENDED)
            .spawn()
            .expect("spawning a suspended child must succeed");
        let pid = child.id();

        let suspended = collect_suspended_pids();
        let found = suspended.contains(&pid);

        // Terminate before asserting so a failure cannot leak the
        // suspended child, which would never exit on its own.
        let _ = child.kill();
        let _ = child.wait();

        assert!(
            found,
            "a CREATE_SUSPENDED child (pid {pid}) must be reported as suspended",
        );
    }

    #[test]
    fn running_processes_are_not_reported_as_suspended() {
        // The current process is demonstrably running, so it must not
        // appear in the set. Guards against a walk that mislabels
        // every process, which the test above alone would not catch.
        let suspended = collect_suspended_pids();
        assert!(
            !suspended.contains(&std::process::id()),
            "the running test process must not be reported as suspended",
        );
    }

    #[test]
    fn system_process_snapshot_parses_into_plausible_records() {
        // The System process (pid 4) always exists and always carries
        // many threads, so finding it proves NextEntryOffset walking
        // and the UniqueProcessId field are both being read correctly.
        use windows::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;

        let (buf, used) = query_system_processes().expect("system snapshot must succeed");
        let base = buf.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        let mut records = 0usize;
        let mut saw_system = false;

        loop {
            if offset + size_of::<SYSTEM_PROCESS_INFORMATION>() > used {
                break;
            }
            // SAFETY: the bounds check above proves the record lies
            // inside the initialised region of the aligned buffer.
            let record = unsafe { *base.add(offset).cast::<SYSTEM_PROCESS_INFORMATION>() };
            records += 1;
            if record.UniqueProcessId.0 as u32 == 4 {
                saw_system = true;
                assert!(
                    record.NumberOfThreads > 0,
                    "the System process must report threads",
                );
            }
            let step = record.NextEntryOffset as usize;
            if step == 0 {
                break;
            }
            offset += step;
        }

        assert!(records > 1, "the snapshot must contain many processes");
        assert!(saw_system, "the snapshot must contain the System process");
    }

    #[test]
    fn collect_reports_io_counters_for_the_current_process() {
        // Do a known write first, so the assertion rests on I/O this
        // test performed rather than on incidental image loading.
        // Processes that deny PROCESS_QUERY_LIMITED_INFORMATION keep
        // zeroed counters, so this deliberately checks our own PID.
        let path = std::env::temp_dir().join(format!("rtop-io-probe-{}", std::process::id()));
        std::fs::write(&path, vec![0u8; 64 * 1024]).expect("temp file write must succeed");
        let observed = std::fs::read(&path).expect("temp file read must succeed");
        assert_eq!(observed.len(), 64 * 1024);
        let _ = std::fs::remove_file(&path);

        let mut collector = ProcCollector::new();
        collector.set_core_count(1);
        collector.collect();

        let me = collector
            .procs
            .iter()
            .find(|p| p.pid == std::process::id())
            .expect("the current process is always in its own snapshot");

        assert!(
            me.io_write > 0,
            "io_write must reflect the 64 KiB this test wrote",
        );
        assert!(
            me.io_read > 0,
            "io_read must reflect the 64 KiB this test read back",
        );
    }
}
