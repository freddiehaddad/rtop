use crate::domain::process::{PriorityClass, ProcInfo, ProcState};
use std::{
    collections::HashMap,
    ffi::c_void,
    mem::{MaybeUninit, size_of},
    slice,
};
use thiserror::Error;

use super::{
    Collector,
    win::{CounterDelta, OwnedHandle, checked_u32_size, counter_delta, exact_byte_count},
};
use windows::Win32::Foundation::HANDLE;

const TOKEN_QUERY_ACCESS: u32 = 0x0008;
const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;
const MAX_REMOTE_COMMAND_LINE_BYTES: usize = u16::MAX as usize - 1;

#[cfg(target_pointer_width = "64")]
const PEB_PROCESS_PARAMETERS_OFFSET_X64: usize = 0x20;
#[cfg(target_pointer_width = "64")]
const RTL_USER_PROCESS_PARAMETERS_COMMAND_LINE_OFFSET_X64: usize = 0x70;

#[repr(C)]
struct ProcessBasicInformation {
    reserved1: usize,
    peb_base_address: usize,
    reserved2: [usize; 2],
    unique_process_id: usize,
    reserved3: usize,
}

#[cfg(target_pointer_width = "64")]
#[repr(C)]
#[derive(Clone, Copy)]
struct RemoteUnicodeString64 {
    length: u16,
    maximum_length: u16,
    padding: u32,
    buffer: u64,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<RemoteUnicodeString64>() == 16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum CmdlineReadError {
    #[cfg(not(target_pointer_width = "64"))]
    #[error("unsupported architecture for remote command-line read")]
    UnsupportedArchitecture,
    #[error("OpenProcess failed")]
    OpenProcess,
    #[error("NtQueryInformationProcess (ProcessBasicInformation) failed")]
    QueryBasicInfo,
    #[error("invalid pointer encountered while traversing PEB")]
    InvalidPointer,
    #[error("integer overflow while computing remote read size")]
    IntegerOverflow,
    #[error("ReadProcessMemory returned fewer bytes than requested")]
    ShortRead,
    #[error("UNICODE_STRING in remote process is invalid")]
    InvalidUnicodeString,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadProcessMemory(
        process: HANDLE,
        base: usize,
        buffer: *mut c_void,
        size: usize,
        bytes_read: *mut usize,
    ) -> i32;
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

fn query_process_basic_info(handle: HANDLE) -> Result<ProcessBasicInformation, CmdlineReadError> {
    let mut pbi = MaybeUninit::<ProcessBasicInformation>::uninit();
    let mut ret_len: u32 = 0;
    let info_len = checked_u32_size(size_of::<ProcessBasicInformation>())
        .ok_or(CmdlineReadError::IntegerOverflow)?;

    // SAFETY: pbi points to writable storage for ProcessBasicInformation and
    // info_len matches that storage size. The return status and byte count are
    // checked before the structure is assumed initialized.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_BASIC_INFORMATION_CLASS,
            pbi.as_mut_ptr().cast::<c_void>(),
            info_len,
            &mut ret_len,
        )
    };
    if status != 0 || ret_len as usize != size_of::<ProcessBasicInformation>() {
        return Err(CmdlineReadError::QueryBasicInfo);
    }

    // SAFETY: NtQueryInformationProcess succeeded and reported that it wrote
    // exactly a full ProcessBasicInformation record.
    let pbi = unsafe { pbi.assume_init() };
    if pbi.peb_base_address == 0 {
        return Err(CmdlineReadError::InvalidPointer);
    }
    Ok(pbi)
}

fn read_process_memory_exact(
    handle: HANDLE,
    base: usize,
    buffer: &mut [u8],
) -> Result<(), CmdlineReadError> {
    if base == 0 || buffer.is_empty() {
        return Err(CmdlineReadError::InvalidPointer);
    }

    let mut bytes_read = 0usize;
    // SAFETY: buffer is a valid local writable byte slice. The remote address
    // belongs to the target process handle; ReadProcessMemory validates that
    // address and reports how many bytes it copied.
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            base,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len(),
            &mut bytes_read,
        )
    };
    if ok == 0 || !exact_byte_count(bytes_read, buffer.len()) {
        return Err(CmdlineReadError::ShortRead);
    }

    Ok(())
}

unsafe fn read_remote_copy<T: Copy>(handle: HANDLE, base: usize) -> Result<T, CmdlineReadError> {
    let mut value = MaybeUninit::<T>::uninit();
    // SAFETY: the byte slice covers only the uninitialized local storage for T.
    // The caller guarantees T is an integer-only FFI layout where any bit
    // pattern is valid before assume_init below.
    let bytes =
        unsafe { slice::from_raw_parts_mut(value.as_mut_ptr().cast::<u8>(), size_of::<T>()) };
    read_process_memory_exact(handle, base, bytes)?;
    // SAFETY: read_process_memory_exact wrote exactly size_of::<T>() bytes into
    // value, and the caller constrained T to an all-bit-pattern-valid layout.
    Ok(unsafe { value.assume_init() })
}

fn read_remote_utf16(
    handle: HANDLE,
    base: usize,
    units: usize,
) -> Result<Vec<u16>, CmdlineReadError> {
    if units == 0 {
        return Err(CmdlineReadError::InvalidUnicodeString);
    }

    let mut buf = vec![0u16; units];
    // SAFETY: the byte slice covers the initialized local UTF-16 buffer. The
    // byte length is units * size_of::<u16>(), which cannot overflow because it
    // was validated from a u16 byte count.
    let bytes = unsafe {
        slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), units * size_of::<u16>())
    };
    read_process_memory_exact(handle, base, bytes)?;
    Ok(buf)
}

fn command_line_utf16_units(
    length: u16,
    maximum_length: u16,
    buffer: usize,
) -> Result<usize, CmdlineReadError> {
    let length = usize::from(length);
    let maximum_length = usize::from(maximum_length);
    if buffer == 0
        || length == 0
        || length % size_of::<u16>() != 0
        || length > maximum_length
        || length > MAX_REMOTE_COMMAND_LINE_BYTES
    {
        return Err(CmdlineReadError::InvalidUnicodeString);
    }
    Ok(length / size_of::<u16>())
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

/// Read the full command line of a process via NtQueryInformationProcess.
/// Returns empty string if access is denied or the process is protected.
fn get_process_cmdline(pid: u32) -> String {
    try_get_process_cmdline(pid).unwrap_or_default()
}

#[cfg(target_pointer_width = "64")]
fn try_get_process_cmdline(pid: u32) -> Result<String, CmdlineReadError> {
    use windows::Win32::System::Threading::*;

    // SAFETY: OpenProcess is called with query/read rights for a PID provided by
    // process enumeration. The returned handle is checked before use and owned
    // by OwnedHandle.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
        .ok()
        .and_then(OwnedHandle::new)
        .ok_or(CmdlineReadError::OpenProcess)?;

    let pbi = query_process_basic_info(handle.get())?;
    let params_ptr_addr = pbi
        .peb_base_address
        .checked_add(PEB_PROCESS_PARAMETERS_OFFSET_X64)
        .ok_or(CmdlineReadError::IntegerOverflow)?;
    // SAFETY: usize is an integer pointer-sized value; every bit pattern is
    // valid. read_remote_copy verifies the exact byte count before returning.
    let params_ptr: usize = unsafe { read_remote_copy(handle.get(), params_ptr_addr)? };
    if params_ptr == 0 {
        return Err(CmdlineReadError::InvalidPointer);
    }

    let cmdline_addr = params_ptr
        .checked_add(RTL_USER_PROCESS_PARAMETERS_COMMAND_LINE_OFFSET_X64)
        .ok_or(CmdlineReadError::IntegerOverflow)?;
    // SAFETY: RemoteUnicodeString64 is a repr(C) integer-only mirror of the x64
    // UNICODE_STRING layout embedded in RTL_USER_PROCESS_PARAMETERS.
    let cmdline: RemoteUnicodeString64 = unsafe { read_remote_copy(handle.get(), cmdline_addr)? };

    let char_count = command_line_utf16_units(
        cmdline.length,
        cmdline.maximum_length,
        cmdline.buffer as usize,
    )?;
    let cmd_buf = read_remote_utf16(handle.get(), cmdline.buffer as usize, char_count)?;
    Ok(sanitize_command_line(&String::from_utf16_lossy(&cmd_buf)))
}

#[cfg(not(target_pointer_width = "64"))]
fn try_get_process_cmdline(_pid: u32) -> Result<String, CmdlineReadError> {
    Err(CmdlineReadError::UnsupportedArchitecture)
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
    let pct = (delta as f64 / system_delta as f64) * 100.0;
    if !pct.is_finite() {
        return 0.0;
    }
    pct.clamp(0.0, 100.0 * core_count as f64)
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
    fn command_line_utf16_units_accepts_valid_even_length() {
        assert_eq!(command_line_utf16_units(8, 10, 0x1000), Ok(4));
    }

    #[test]
    fn command_line_utf16_units_rejects_invalid_metadata() {
        assert_eq!(
            command_line_utf16_units(0, 10, 0x1000),
            Err(CmdlineReadError::InvalidUnicodeString)
        );
        assert_eq!(
            command_line_utf16_units(7, 10, 0x1000),
            Err(CmdlineReadError::InvalidUnicodeString)
        );
        assert_eq!(
            command_line_utf16_units(12, 10, 0x1000),
            Err(CmdlineReadError::InvalidUnicodeString)
        );
        assert_eq!(
            command_line_utf16_units(8, 10, 0),
            Err(CmdlineReadError::InvalidUnicodeString)
        );
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
}
