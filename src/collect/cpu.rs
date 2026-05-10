use crate::collect::cpu_thermal::ThermalCollector;
use crate::domain::cpu::CpuInfo;
use std::collections::VecDeque;

use super::{
    Collector,
    win::{
        OwnedRegKey, PdhCounter, PdhQuery, checked_u32_size, percent_u64, string_from_utf16_buf,
    },
};

/// Maximum number of data points to retain in history deques.
const MAX_HISTORY: usize = 300;
const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION: u32 = 8;

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

struct CpuPdhCounters {
    query: PdhQuery,
    freq: PdhCounter,
    perf: PdhCounter,
}

/// CPU data collector using Windows APIs.
pub struct CpuCollector {
    pub info: CpuInfo,
    pub status: super::CollectStatus,
    prev_idle: Vec<u64>,
    prev_kernel: Vec<u64>,
    prev_user: Vec<u64>,
    load_avg_samples: VecDeque<f64>,
    initialized: bool,
    // Persistent PDH query for CPU frequency (needs two collections for rate counters)
    pdh_counters: Option<CpuPdhCounters>,
    pdh_has_first_sample: bool,
    thermal: ThermalCollector,
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
            status: super::CollectStatus::Ok,
            prev_idle: Vec::new(),
            prev_kernel: Vec::new(),
            prev_user: Vec::new(),
            load_avg_samples: VecDeque::with_capacity(900),
            initialized: false,
            pdh_counters: None,
            pdh_has_first_sample: false,
            thermal: ThermalCollector::default(),
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

        // Detect CPU vendor and load PawnIO module for temperature/power.
        self.thermal = ThermalCollector::detect(self.info.core_count);
    }

    fn collect_impl(&mut self) {
        self.status = super::CollectStatus::Ok;

        if !self.initialized {
            self.init();
        }

        self.collect_cpu_times();
        self.collect_frequency();
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
                let total_delta = kernel_delta.saturating_add(user_delta);

                if total_delta > 0 {
                    let cpu_pct =
                        percent_u64(total_delta.saturating_sub(idle_delta), total_delta).min(100);
                    push_history(&mut self.info.cpu_percent.total, cpu_pct);

                    let user_pct = percent_u64(user_delta, total_delta).min(100);
                    push_history(&mut self.info.cpu_percent.user, user_pct);

                    let system_pct =
                        percent_u64(kernel_delta.saturating_sub(idle_delta), total_delta).min(100);
                    push_history(&mut self.info.cpu_percent.system, system_pct);

                    let idle_pct = percent_u64(idle_delta, total_delta).min(100);
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
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Cpu,
                "core_count is 0; per-core collection skipped",
            );
            self.status
                .downgrade(super::CollectStatus::Failed("no cores detected"));
            return;
        }

        let Some(buf_size) = core_count
            .checked_mul(std::mem::size_of::<ProcessorPerfInfo>())
            .and_then(checked_u32_size)
        else {
            self.status
                .downgrade(super::CollectStatus::Degraded("per-core cpu unavailable"));
            return;
        };

        let mut perf_info = vec![ProcessorPerfInfo::default(); core_count];
        let mut return_len = 0u32;

        // SystemProcessorPerformanceInformation = 8
        // SAFETY: perf_info is a Vec of repr(C) structs sized to core_count.
        // buf_size matches the allocation. return_len receives the actual bytes
        // written and is used to bound iteration over the results.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
                perf_info.as_mut_ptr() as *mut std::ffi::c_void,
                buf_size,
                &mut return_len,
            )
        };

        if status == 0 {
            // status == STATUS_SUCCESS
            let Some(actual_count) = processor_perf_record_count(return_len, core_count) else {
                self.status
                    .downgrade(super::CollectStatus::Degraded("per-core cpu unavailable"));
                return;
            };

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
                let idle_val = perf_counter_to_u64(pi.idle_time);
                let kernel_val = perf_counter_to_u64(pi.kernel_time);
                let user_val = perf_counter_to_u64(pi.user_time);

                // Index i+1 in prev arrays (index 0 is the aggregate)
                let pi_idx = i + 1;

                let idle_delta = idle_val.saturating_sub(self.prev_idle[pi_idx]);
                let kernel_delta = kernel_val.saturating_sub(self.prev_kernel[pi_idx]);
                let user_delta = user_val.saturating_sub(self.prev_user[pi_idx]);
                let total_delta = kernel_delta.saturating_add(user_delta);

                let core_pct =
                    percent_u64(total_delta.saturating_sub(idle_delta), total_delta).min(100);

                push_history(&mut self.info.core_percent[i], core_pct);

                self.prev_idle[pi_idx] = idle_val;
                self.prev_kernel[pi_idx] = kernel_val;
                self.prev_user[pi_idx] = user_val;
            }
        } else {
            self.status
                .downgrade(super::CollectStatus::Degraded("per-core cpu unavailable"));
        }
    }

    fn collect_frequency(&mut self) {
        // Task Manager computes: base_freq * (% Processor Performance) / 100
        // % Processor Performance is a rate counter that needs TWO PdhCollectQueryData
        // calls with a time gap. We keep the query persistent across frames.

        // Initialize the persistent PDH query on first call
        if self.pdh_counters.is_none() {
            let freq_path: Vec<u16> = "\\Processor Information(_Total)\\Processor Frequency\0"
                .encode_utf16()
                .collect();
            let perf_path: Vec<u16> = "\\Processor Information(_Total)\\% Processor Performance\0"
                .encode_utf16()
                .collect();

            let Ok(query) = PdhQuery::open() else {
                self.collect_frequency_fallback();
                return;
            };
            let Ok(freq) = query.add_counter(&freq_path) else {
                self.collect_frequency_fallback();
                return;
            };
            let Ok(perf) = query.add_counter(&perf_path) else {
                self.collect_frequency_fallback();
                return;
            };
            if query.collect().is_err() {
                self.collect_frequency_fallback();
                return;
            }

            self.pdh_counters = Some(CpuPdhCounters { query, freq, perf });
            self.pdh_has_first_sample = false;

            // On first frame, use registry fallback since we don't have data yet
            self.collect_frequency_fallback();
            return;
        }

        // Collect new sample
        let collect_failed = self
            .pdh_counters
            .as_ref()
            .is_none_or(|counters| counters.query.collect().is_err());
        if collect_failed {
            self.collect_frequency_fallback();
            return;
        }

        if !self.pdh_has_first_sample {
            // Second call — now rate counters will have data
            self.pdh_has_first_sample = true;
        }

        let Some(counters) = self.pdh_counters.as_ref() else {
            self.collect_frequency_fallback();
            return;
        };
        let freq_val = counters.freq.formatted_f64();
        let perf_val = counters.perf.formatted_f64();

        if let (Some(freq), Some(perf)) = (freq_val, perf_val)
            && freq.is_finite()
            && perf.is_finite()
            && freq > 0.0
            && perf > 0.0
        {
            let actual_mhz = (freq * perf / 100.0).clamp(0.0, u32::MAX as f64) as u32;
            if actual_mhz >= 1000 {
                self.info.cpu_hz = format!("{:.2} GHz", actual_mhz as f64 / 1000.0);
            } else {
                self.info.cpu_hz = format!("{} MHz", actual_mhz);
            }
            return;
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
        // The key handle is closed by OwnedRegKey after use.
        unsafe {
            let mut raw_key = Default::default();
            let subkey = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, Some(0), KEY_READ, &mut raw_key).is_ok() {
                let Some(key) = OwnedRegKey::new(raw_key) else {
                    return;
                };
                let mut mhz: u32 = 0;
                let mut size = std::mem::size_of::<u32>() as u32;
                let mut value_type = REG_VALUE_TYPE::default();
                let val_name = w!("~MHz");
                if RegQueryValueExW(
                    key.get(),
                    val_name,
                    None,
                    Some(&mut value_type),
                    Some(&mut mhz as *mut u32 as *mut u8),
                    Some(&mut size),
                )
                .is_ok()
                    && value_type == REG_DWORD
                    && size as usize == std::mem::size_of::<u32>()
                {
                    if mhz >= 1000 {
                        self.info.cpu_hz = format!("{:.2} GHz", mhz as f64 / 1000.0);
                    } else {
                        self.info.cpu_hz = format!("{} MHz", mhz);
                    }
                }
            }
        }
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
        if !self.thermal.is_active() {
            return;
        }

        let sample = self.thermal.sample();

        self.info.cpu_watts = sample.watts;
        self.info.cpu_max_watts = sample.max_watts;

        // Storage layout: index 0 = package, index 1+ = per-core.
        let total = 1 + sample.core_temps.len();
        while self.info.temp.len() < total {
            self.info.temp.push(VecDeque::new());
        }

        let pkg = sample.package_temp.unwrap_or(0);
        push_history(&mut self.info.temp[0], pkg);

        for (slot, &temp) in sample.core_temps.iter().enumerate() {
            push_history(&mut self.info.temp[slot + 1], temp);
        }
    }
}

impl Collector for CpuCollector {
    type Snapshot = crate::runner::CpuSnapshot;

    fn collect(&mut self) {
        self.collect_impl();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::CpuSnapshot {
            info: self.info.clone(),
            status: self.status.clone(),
        }
    }
}

fn get_cpu_name() -> String {
    use windows::Win32::System::Registry::*;
    use windows::core::*;

    // SAFETY: RegOpenKeyExW and RegQueryValueExW receive valid null-terminated
    // wide-string paths. The buffer is a stack-allocated u16 array with its
    // byte size passed correctly. The key handle is closed by OwnedRegKey.
    unsafe {
        let mut raw_key = Default::default();
        let subkey = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, Some(0), KEY_READ, &mut raw_key).is_ok() {
            let Some(key) = OwnedRegKey::new(raw_key) else {
                return "Unknown CPU".to_string();
            };
            let mut buf = [0u16; 256];
            let mut size = (buf.len() * 2) as u32;
            let mut value_type = REG_VALUE_TYPE::default();
            let val_name = w!("ProcessorNameString");
            if RegQueryValueExW(
                key.get(),
                val_name,
                None,
                Some(&mut value_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut size),
            )
            .is_ok()
                && value_type == REG_SZ
            {
                let byte_len = size as usize;
                if byte_len <= buf.len() * std::mem::size_of::<u16>()
                    && byte_len.is_multiple_of(std::mem::size_of::<u16>())
                {
                    let units = byte_len / std::mem::size_of::<u16>();
                    return string_from_utf16_buf(&buf[..units]);
                }
            }
        }
    }
    "Unknown CPU".to_string()
}

pub(crate) fn get_core_count() -> usize {
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

fn perf_counter_to_u64(value: i64) -> u64 {
    value.try_into().unwrap_or(0)
}

fn processor_perf_record_count(return_len: u32, capacity: usize) -> Option<usize> {
    let record_size = std::mem::size_of::<ProcessorPerfInfo>();
    let return_len = return_len as usize;
    if return_len == 0 || !return_len.is_multiple_of(record_size) {
        return None;
    }
    let count = return_len / record_size;
    (count <= capacity).then_some(count)
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
    let total = kernel_delta.saturating_add(user_delta);
    if total == 0 {
        return 0;
    }
    percent_u64(total.saturating_sub(idle_delta), total).min(100)
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
    fn calculate_cpu_percent_saturates_invalid_idle_delta() {
        assert_eq!(calculate_cpu_percent(200, 100, 0), 0);
    }

    #[test]
    fn calculate_cpu_percent_avoids_overflow() {
        assert_eq!(calculate_cpu_percent(0, u64::MAX, u64::MAX), 100);
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
    fn processor_perf_record_count_validates_return_len() {
        let record = std::mem::size_of::<ProcessorPerfInfo>() as u32;
        assert_eq!(processor_perf_record_count(record * 4, 8), Some(4));
        assert_eq!(processor_perf_record_count(record * 9, 8), None);
        assert_eq!(processor_perf_record_count(record + 1, 8), None);
        assert_eq!(processor_perf_record_count(0, 8), None);
    }

    #[test]
    fn collect_returns_valid_cpu_info() {
        let mut collector = CpuCollector::new();
        collector.init();
        assert!(collector.info.core_count > 0);
        assert!(!collector.info.cpu_name.is_empty());
    }
}
