use crate::domain::cpu::CpuInfo;
use std::collections::VecDeque;

use super::Collector;

/// Maximum number of data points to retain in history deques.
const MAX_HISTORY: usize = 300;

/// How we collect temperature data.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TempSource {
    None,
    LhmHttp,
}

/// CPU data collector using Windows APIs.
pub struct CpuCollector {
    pub info: CpuInfo,
    prev_idle: Vec<u64>,
    prev_kernel: Vec<u64>,
    prev_user: Vec<u64>,
    load_avg_samples: VecDeque<f64>,
    initialized: bool,
    // Persistent PDH query for CPU frequency (needs two collections for rate counters)
    pdh_query: isize,
    pdh_freq_counter: isize,
    pdh_perf_counter: isize,
    pdh_initialized: bool,
    pdh_has_first_sample: bool,
    temp_source: TempSource,
}

impl Default for CpuCollector {
    /// Creates a new `CpuCollector`. Note: `init()` must be called separately
    /// to populate CPU name, core count, and temperature source.
    fn default() -> Self {
        Self::new()
    }
}

impl CpuCollector {
    /// Create a new CPU collector with default state.
    pub fn new() -> Self {
        Self {
            info: CpuInfo::default(),
            prev_idle: Vec::new(),
            prev_kernel: Vec::new(),
            prev_user: Vec::new(),
            load_avg_samples: VecDeque::with_capacity(900),
            initialized: false,
            pdh_query: 0,
            pdh_freq_counter: 0,
            pdh_perf_counter: 0,
            pdh_initialized: false,
            pdh_has_first_sample: false,
            temp_source: TempSource::None,
        }
    }

    /// Initialize CPU information (model name, core count).
    pub fn init(&mut self) {
        self.info.cpu_name = get_cpu_name();
        self.info.core_count = get_core_count();
        self.info.core_percent = vec![VecDeque::new(); self.info.core_count];
        self.prev_idle = vec![0; self.info.core_count + 1];
        self.prev_kernel = vec![0; self.info.core_count + 1];
        self.prev_user = vec![0; self.info.core_count + 1];
        self.initialized = true;

        // Detect temperature source: try LHM HTTP first
        if lhm_http_probe() {
            self.temp_source = TempSource::LhmHttp;
        }
    }

    fn collect_impl(&mut self) {
        if !self.initialized {
            self.init();
        }

        self.collect_cpu_times();
        self.collect_frequency();
        self.collect_uptime();
        self.update_load_avg();
        self.collect_temperatures();
    }

    fn collect_cpu_times(&mut self) {
        use windows::Win32::Foundation::FILETIME;

        // GetSystemTimes for aggregate totals
        // SAFETY: FFI declaration for kernel32 GetSystemTimes; signature matches
        // the Windows API ABI with properly typed FILETIME output pointers.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetSystemTimes(
                idle: *mut FILETIME,
                kernel: *mut FILETIME,
                user: *mut FILETIME,
            ) -> i32;
        }

        // NtQuerySystemInformation for per-core data

        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct ProcessorPerfInfo {
            idle_time: i64,
            kernel_time: i64,
            user_time: i64,
            dpc_time: i64,
            interrupt_time: i64,
            interrupt_count: u32,
        }

        // SAFETY: FFI declaration for ntdll NtQuerySystemInformation; signature
        // matches the NT API with correctly sized buffer and return-length pointer.
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtQuerySystemInformation(
                system_information_class: u32,
                system_information: *mut std::ffi::c_void,
                system_information_length: u32,
                return_length: *mut u32,
            ) -> i32;
        }

        // --- Aggregate CPU times ---
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();

        // SAFETY: GetSystemTimes writes to valid, properly-aligned FILETIME
        // pointers allocated on the stack. The return value is checked before
        // using the output.
        unsafe {
            if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                let idle_val = filetime_to_u64(&idle);
                let kernel_val = filetime_to_u64(&kernel);
                let user_val = filetime_to_u64(&user);

                let idle_delta = idle_val.saturating_sub(self.prev_idle[0]);
                let kernel_delta = kernel_val.saturating_sub(self.prev_kernel[0]);
                let user_delta = user_val.saturating_sub(self.prev_user[0]);
                let total_delta = kernel_delta + user_delta;

                if total_delta > 0 {
                    let cpu_pct = ((total_delta - idle_delta) * 100)
                        .checked_div(total_delta)
                        .unwrap_or(0) as i64;
                    push_history(&mut self.info.cpu_percent.total, cpu_pct);

                    let user_pct = (user_delta * 100).checked_div(total_delta).unwrap_or(0) as i64;
                    push_history(&mut self.info.cpu_percent.user, user_pct);

                    let system_pct = ((kernel_delta - idle_delta) * 100)
                        .checked_div(total_delta)
                        .unwrap_or(0) as i64;
                    push_history(&mut self.info.cpu_percent.system, system_pct.max(0));

                    let idle_pct = (idle_delta * 100).checked_div(total_delta).unwrap_or(0) as i64;
                    push_history(&mut self.info.cpu_percent.idle, idle_pct);
                }

                self.prev_idle[0] = idle_val;
                self.prev_kernel[0] = kernel_val;
                self.prev_user[0] = user_val;
            }
        }

        // --- Per-core CPU times via NtQuerySystemInformation ---
        let core_count = self.info.core_count;
        if core_count == 0 {
            return;
        }

        let mut perf_info = vec![ProcessorPerfInfo::default(); core_count];
        let buf_size = (core_count * std::mem::size_of::<ProcessorPerfInfo>()) as u32;
        let mut return_len = 0u32;

        // SystemProcessorPerformanceInformation = 8
        // SAFETY: perf_info is a Vec of repr(C) structs sized to core_count.
        // buf_size matches the allocation. return_len receives the actual bytes
        // written and is used to bound iteration over the results.
        let status = unsafe {
            NtQuerySystemInformation(
                8,
                perf_info.as_mut_ptr() as *mut std::ffi::c_void,
                buf_size,
                &mut return_len,
            )
        };

        if status == 0 {
            // status == STATUS_SUCCESS
            let actual_count =
                (return_len as usize / std::mem::size_of::<ProcessorPerfInfo>()).min(core_count);

            // Ensure vectors are sized correctly
            while self.prev_idle.len() < actual_count + 1 {
                self.prev_idle.push(0);
            }
            while self.prev_kernel.len() < actual_count + 1 {
                self.prev_kernel.push(0);
            }
            while self.prev_user.len() < actual_count + 1 {
                self.prev_user.push(0);
            }
            while self.info.core_percent.len() < actual_count {
                self.info.core_percent.push(VecDeque::new());
            }

            for (i, pi) in perf_info.iter().enumerate().take(actual_count) {
                let idle_val = pi.idle_time as u64;
                let kernel_val = pi.kernel_time as u64;
                let user_val = pi.user_time as u64;

                // Index i+1 in prev arrays (index 0 is the aggregate)
                let pi_idx = i + 1;

                let idle_delta = idle_val.saturating_sub(self.prev_idle[pi_idx]);
                let kernel_delta = kernel_val.saturating_sub(self.prev_kernel[pi_idx]);
                let user_delta = user_val.saturating_sub(self.prev_user[pi_idx]);
                let total_delta = kernel_delta + user_delta;

                let core_pct = ((total_delta - idle_delta) * 100)
                    .checked_div(total_delta)
                    .unwrap_or(0) as i64;

                push_history(&mut self.info.core_percent[i], core_pct);

                self.prev_idle[pi_idx] = idle_val;
                self.prev_kernel[pi_idx] = kernel_val;
                self.prev_user[pi_idx] = user_val;
            }
        }
    }

    fn collect_frequency(&mut self) {
        // Task Manager computes: base_freq * (% Processor Performance) / 100
        // % Processor Performance is a rate counter that needs TWO PdhCollectQueryData
        // calls with a time gap. We keep the query persistent across frames.

        // SAFETY: FFI declarations for pdh.dll Performance Data Helper
        // functions; signatures match the Windows PDH API.
        #[link(name = "pdh")]
        unsafe extern "system" {
            fn PdhOpenQueryW(ds: *const u16, ud: usize, q: *mut isize) -> i32;
            fn PdhAddCounterW(q: isize, p: *const u16, ud: usize, c: *mut isize) -> i32;
            fn PdhCollectQueryData(q: isize) -> i32;
            fn PdhGetFormattedCounterValue(c: isize, f: u32, ct: *mut u32, v: *mut PdhVal) -> i32;
            fn PdhCloseQuery(q: isize) -> i32;
        }

        #[repr(C)]
        #[derive(Default)]
        struct PdhVal {
            status: u32,
            value: f64,
        }

        const PDH_FMT_DOUBLE: u32 = 0x00000200;

        // Initialize the persistent PDH query on first call
        if !self.pdh_initialized {
            let freq_path: Vec<u16> = "\\Processor Information(_Total)\\Processor Frequency\0"
                .encode_utf16()
                .collect();
            let perf_path: Vec<u16> = "\\Processor Information(_Total)\\% Processor Performance\0"
                .encode_utf16()
                .collect();

            // SAFETY: PDH functions receive valid pointers to stack-allocated
            // handles and null-terminated UTF-16 counter path strings. Return
            // values are checked; the query is closed on failure.
            unsafe {
                let mut q: isize = 0;
                if PdhOpenQueryW(std::ptr::null(), 0, &mut q) != 0 {
                    self.collect_frequency_fallback();
                    return;
                }
                self.pdh_query = q;

                if PdhAddCounterW(q, freq_path.as_ptr(), 0, &mut self.pdh_freq_counter) != 0
                    || PdhAddCounterW(q, perf_path.as_ptr(), 0, &mut self.pdh_perf_counter) != 0
                {
                    PdhCloseQuery(q);
                    self.pdh_query = 0;
                    self.collect_frequency_fallback();
                    return;
                }

                // First collection establishes baseline for rate counters
                let _ = PdhCollectQueryData(q);
                self.pdh_initialized = true;
                self.pdh_has_first_sample = false;
            }

            // On first frame, use registry fallback since we don't have data yet
            self.collect_frequency_fallback();
            return;
        }

        // Collect new sample
        // SAFETY: self.pdh_query and counter handles were successfully
        // initialized by a prior PdhOpenQueryW/PdhAddCounterW call.
        // PdhVal is repr(C) and properly aligned for the API output.
        unsafe {
            if PdhCollectQueryData(self.pdh_query) != 0 {
                self.collect_frequency_fallback();
                return;
            }

            if !self.pdh_has_first_sample {
                // Second call — now rate counters will have data
                self.pdh_has_first_sample = true;
            }

            let mut freq_val = PdhVal::default();
            let mut perf_val = PdhVal::default();
            let mut ct: u32 = 0;

            let freq_ok = PdhGetFormattedCounterValue(
                self.pdh_freq_counter,
                PDH_FMT_DOUBLE,
                &mut ct,
                &mut freq_val,
            ) == 0
                && freq_val.status == 0;

            let perf_ok = PdhGetFormattedCounterValue(
                self.pdh_perf_counter,
                PDH_FMT_DOUBLE,
                &mut ct,
                &mut perf_val,
            ) == 0
                && perf_val.status == 0;

            if freq_ok && perf_ok && freq_val.value > 0.0 && perf_val.value > 0.0 {
                let actual_mhz = (freq_val.value * perf_val.value / 100.0) as u32;
                if actual_mhz >= 1000 {
                    self.info.cpu_hz = format!("{:.2} GHz", actual_mhz as f64 / 1000.0);
                } else {
                    self.info.cpu_hz = format!("{} MHz", actual_mhz);
                }
                return;
            }
        }

        self.collect_frequency_fallback();
    }

    fn collect_frequency_fallback(&mut self) {
        // Fallback: read base frequency from registry
        use windows::Win32::System::Registry::*;
        use windows::core::*;

        // SAFETY: RegOpenKeyExW and RegQueryValueExW receive valid
        // null-terminated wide-string key/value names. The output buffer
        // is a stack-allocated u32 with its size passed correctly.
        // The key handle is closed after use.
        unsafe {
            let mut key = Default::default();
            let subkey = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, Some(0), KEY_READ, &mut key).is_ok() {
                let mut mhz: u32 = 0;
                let mut size = std::mem::size_of::<u32>() as u32;
                let val_name = w!("~MHz");
                if RegQueryValueExW(
                    key,
                    val_name,
                    None,
                    None,
                    Some(&mut mhz as *mut u32 as *mut u8),
                    Some(&mut size),
                )
                .is_ok()
                {
                    if mhz >= 1000 {
                        self.info.cpu_hz = format!("{:.2} GHz", mhz as f64 / 1000.0);
                    } else {
                        self.info.cpu_hz = format!("{} MHz", mhz);
                    }
                }
                let _ = RegCloseKey(key);
            }
        }
    }

    fn collect_uptime(&mut self) {
        // SAFETY: GetTickCount64 requires no arguments and always succeeds.
        self.info.uptime_seconds =
            unsafe { windows::Win32::System::SystemInformation::GetTickCount64() / 1000 };
    }

    fn update_load_avg(&mut self) {
        let total = &self.info.cpu_percent.total;
        if let Some(&last) = total.back() {
            let sample = last as f64 / 100.0;
            self.load_avg_samples.push_back(sample);
            if self.load_avg_samples.len() > 900 {
                self.load_avg_samples.pop_front();
            }

            // Compute rolling averages
            let len = self.load_avg_samples.len();
            let samples = self.load_avg_samples.make_contiguous();
            let avg_fn = |window: usize| -> f64 {
                let start = len.saturating_sub(window);
                let slice = &samples[start..];
                if slice.is_empty() {
                    0.0
                } else {
                    slice.iter().sum::<f64>() / slice.len() as f64
                }
            };

            self.info.load_avg = [
                avg_fn(60),  // ~1 min (at 1 sample/sec)
                avg_fn(300), // ~5 min
                avg_fn(900), // ~15 min
            ];
        }
    }

    fn collect_temperatures(&mut self) {
        if self.temp_source != TempSource::LhmHttp {
            return;
        }

        let Some(json) = lhm_http_fetch() else {
            return;
        };

        let mut package_temp: Option<i64> = None;
        let mut core_temps: Vec<i64> = Vec::new();

        // Walk the tree looking for "Temperatures" under CPU hardware nodes.
        // Generic approach: identify package-level temp by known keywords,
        // treat everything else as per-core temps in discovery order.
        fn walk(
            node: &serde_json::Value,
            in_temps: bool,
            in_cpu: bool,
            pkg: &mut Option<i64>,
            cores: &mut Vec<i64>,
        ) {
            let text = node.get("Text").and_then(|v| v.as_str()).unwrap_or("");
            let hw_id = node
                .get("HardwareId")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Detect CPU hardware nodes by HardwareId or name
            let is_cpu_hw = in_cpu
                || hw_id.contains("cpu")
                || hw_id.contains("intelcpu")
                || hw_id.contains("amdcpu")
                || text.to_lowercase().contains("cpu")
                || text.to_lowercase().contains("core i")
                || text.to_lowercase().contains("core ultra")
                || text.to_lowercase().contains("ryzen")
                || text.to_lowercase().contains("xeon")
                || text.to_lowercase().contains("threadripper");

            let is_temps = text == "Temperatures";

            if in_temps && is_cpu_hw {
                if let Some(val_str) = node.get("Value").and_then(|v| v.as_str()) {
                    if let Some(temp) = parse_temp_value(val_str) {
                        // Skip non-temperature entries
                        if text.contains("Distance") || text.contains("TjMax") {
                            // Skip distance-to-max entries
                        }
                        // Package/aggregate temps
                        else if text == "CPU Package"
                            || text == "CPU"
                            || text == "Core Max"
                            || text == "Core Average"
                            || text == "Tctl"
                            || text == "Tdie"
                            || text == "CPU (Tctl)"
                            || text == "CPU (Tdie)"
                        {
                            // Prefer CPU Package > Tdie > Tctl > others
                            if pkg.is_none()
                                || text == "CPU Package"
                                || (text == "Tdie" && pkg.is_none())
                            {
                                *pkg = Some(temp);
                            }
                        }
                        // Everything else under CPU temps = per-core
                        else if !text.contains("System")
                            && !text.contains("VRM")
                            && !text.contains("PCH")
                            && !text.contains("Socket")
                            && !text.contains("PCIe")
                            && !text.contains("M2")
                        {
                            cores.push(temp);
                        }
                    }
                }
            }

            if let Some(children) = node.get("Children").and_then(|v| v.as_array()) {
                for child in children {
                    walk(child, is_temps && is_cpu_hw, is_cpu_hw, pkg, cores);
                }
            }
        }

        walk(&json, false, false, &mut package_temp, &mut core_temps);

        // Total entries: 1 (package) + per-core count
        let total = 1 + core_temps.len();

        // Ensure self.info.temp has enough VecDeques
        while self.info.temp.len() < total {
            self.info.temp.push(VecDeque::new());
        }

        // Index 0 = package temp
        let pkg = package_temp.unwrap_or(0);
        push_history(&mut self.info.temp[0], pkg);

        // Index 1+ = per-core temps in discovery order
        for (slot, &temp) in core_temps.iter().enumerate() {
            push_history(&mut self.info.temp[slot + 1], temp);
        }
    }
}

impl Collector for CpuCollector {
    fn collect(&mut self) {
        self.collect_impl();
    }
}

/// Probe whether LHM HTTP API is reachable.
fn lhm_http_probe() -> bool {
    lhm_http_fetch().is_some()
}

/// Fetch and parse JSON from LHM HTTP API. Returns None on any failure.
fn lhm_http_fetch() -> Option<serde_json::Value> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream =
        TcpStream::connect_timeout(&"127.0.0.1:8085".parse().ok()?, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;

    let request = "GET /data.json HTTP/1.1\r\nHost: localhost:8085\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;

    let text = String::from_utf8_lossy(&response);
    // Find the start of the JSON body (after the blank line separating headers)
    let body_start = text
        .find("\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| text.find("\n\n").map(|i| i + 2))?;
    let body = &text[body_start..];

    serde_json::from_str(body).ok()
}

/// Parse a temperature value string like "65.0 °C" → 65
fn parse_temp_value(s: &str) -> Option<i64> {
    let num_part = s.split([' ', '\u{00b0}']).next()?;
    num_part.parse::<f64>().ok().map(|v| v as i64)
}

#[cfg(test)]
/// Parse core temperature sensor names to a zero-indexed core number.
fn parse_core_index(text: &str, p_core_count: usize) -> Option<usize> {
    if let Some(rest) = text.strip_prefix("CPU Core #") {
        return rest.parse::<usize>().ok().map(|n| n.saturating_sub(1));
    }
    if let Some(rest) = text.strip_prefix("P-Core #") {
        return rest.parse::<usize>().ok().map(|n| n.saturating_sub(1));
    }
    if let Some(rest) = text.strip_prefix("E-Core #") {
        return rest
            .parse::<usize>()
            .ok()
            .map(|n| p_core_count + n.saturating_sub(1));
    }
    if let Some(rest) = text.strip_prefix("Core #") {
        return rest.parse::<usize>().ok().map(|n| n.saturating_sub(1));
    }
    None
}

fn get_cpu_name() -> String {
    use windows::Win32::System::Registry::*;
    use windows::core::*;

    // SAFETY: RegOpenKeyExW and RegQueryValueExW receive valid null-terminated
    // wide-string paths. The buffer is a stack-allocated u16 array with its
    // byte size passed correctly. The key handle is closed on all paths.
    unsafe {
        let mut key = Default::default();
        let subkey = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, Some(0), KEY_READ, &mut key).is_ok() {
            let mut buf = [0u16; 256];
            let mut size = (buf.len() * 2) as u32;
            let val_name = w!("ProcessorNameString");
            if RegQueryValueExW(
                key,
                val_name,
                None,
                None,
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut size),
            )
            .is_ok()
            {
                let _ = RegCloseKey(key);
                let len = (size as usize / 2).saturating_sub(1);
                return String::from_utf16_lossy(&buf[..len]).trim().to_string();
            }
            let _ = RegCloseKey(key);
        }
    }
    "Unknown CPU".to_string()
}

fn get_core_count() -> usize {
    use windows::Win32::System::SystemInformation::*;

    let mut info = SYSTEM_INFO::default();
    // SAFETY: GetSystemInfo writes to a valid, properly-aligned SYSTEM_INFO
    // struct allocated on the stack and always succeeds.
    unsafe {
        GetSystemInfo(&mut info);
    }
    info.dwNumberOfProcessors as usize
}

fn filetime_to_u64(ft: &windows::Win32::Foundation::FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

fn push_history(deque: &mut VecDeque<i64>, value: i64) {
    deque.push_back(value);
    while deque.len() > MAX_HISTORY {
        deque.pop_front();
    }
}

#[cfg(test)]
/// Calculate CPU percentage from time deltas (for unit testing).
pub fn calculate_cpu_percent(idle_delta: u64, kernel_delta: u64, user_delta: u64) -> i64 {
    let total = kernel_delta + user_delta;
    if total == 0 {
        return 0;
    }
    ((total - idle_delta) * 100 / total) as i64
}

#[cfg(test)]
/// Format frequency in GHz or MHz.
pub fn format_frequency(mhz: u32) -> String {
    if mhz >= 1000 {
        format!("{:.2} GHz", mhz as f64 / 1000.0)
    } else {
        format!("{} MHz", mhz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_cpu_percent_delta() {
        assert_eq!(calculate_cpu_percent(50, 80, 20), 50);
        assert_eq!(calculate_cpu_percent(0, 50, 50), 100);
        assert_eq!(calculate_cpu_percent(100, 100, 0), 0);
    }

    #[test]
    fn calculate_cpu_percent_zero_total() {
        assert_eq!(calculate_cpu_percent(0, 0, 0), 0);
    }

    #[test]
    fn format_frequency_ghz() {
        assert_eq!(format_frequency(3600), "3.60 GHz");
        assert_eq!(format_frequency(5800), "5.80 GHz");
    }

    #[test]
    fn format_frequency_mhz() {
        assert_eq!(format_frequency(800), "800 MHz");
    }

    #[test]
    fn push_history_caps_at_max() {
        let mut deque = VecDeque::new();
        for i in 0..500 {
            push_history(&mut deque, i);
        }
        assert_eq!(deque.len(), MAX_HISTORY);
        assert_eq!(*deque.back().unwrap(), 499);
    }

    #[test]
    fn collect_returns_valid_cpu_info() {
        let mut collector = CpuCollector::new();
        collector.init();
        assert!(collector.info.core_count > 0);
        assert!(!collector.info.cpu_name.is_empty());
    }

    #[test]
    fn parse_temp_value_normal() {
        assert_eq!(parse_temp_value("65.0 °C"), Some(65));
        assert_eq!(parse_temp_value("100.5 °C"), Some(100));
        assert_eq!(parse_temp_value("0.0 °C"), Some(0));
    }

    #[test]
    fn parse_temp_value_no_space() {
        assert_eq!(parse_temp_value("65.0°C"), Some(65));
    }

    #[test]
    fn parse_temp_value_invalid() {
        assert_eq!(parse_temp_value(""), None);
        assert_eq!(parse_temp_value("N/A"), None);
    }

    #[test]
    fn parse_core_index_valid() {
        assert_eq!(parse_core_index("CPU Core #1", 0), Some(0));
        assert_eq!(parse_core_index("CPU Core #8", 0), Some(7));
        assert_eq!(parse_core_index("CPU Core #16", 0), Some(15));
        assert_eq!(parse_core_index("P-Core #1", 0), Some(0));
        assert_eq!(parse_core_index("P-Core #8", 0), Some(7));
        assert_eq!(parse_core_index("E-Core #1", 8), Some(8));
        assert_eq!(parse_core_index("E-Core #16", 8), Some(23));
        assert_eq!(parse_core_index("Core #1", 0), Some(0));
    }

    #[test]
    fn parse_core_index_non_core() {
        assert_eq!(parse_core_index("CPU Package", 0), None);
        assert_eq!(parse_core_index("GPU Core", 0), None);
        assert_eq!(parse_core_index("", 0), None);
        assert_eq!(parse_core_index("P-Core #1 Distance to TjMax", 0), None);
    }
}
