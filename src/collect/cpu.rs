use crate::domain::cpu::{BatteryInfo, CpuInfo};
use std::collections::VecDeque;
use std::mem;

/// Maximum number of data points to retain in history deques.
const MAX_HISTORY: usize = 300;

/// CPU data collector using Windows APIs.
pub struct CpuCollector {
    pub info: CpuInfo,
    prev_idle: Vec<u64>,
    prev_kernel: Vec<u64>,
    prev_user: Vec<u64>,
    load_avg_samples: VecDeque<f64>,
    initialized: bool,
}

impl CpuCollector {
    pub fn new() -> Self {
        Self {
            info: CpuInfo::default(),
            prev_idle: Vec::new(),
            prev_kernel: Vec::new(),
            prev_user: Vec::new(),
            load_avg_samples: VecDeque::with_capacity(900),
            initialized: false,
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
    }

    /// Collect current CPU data.
    pub fn collect(&mut self) -> &CpuInfo {
        if !self.initialized {
            self.init();
        }

        self.collect_cpu_times();
        self.collect_frequency();
        self.collect_uptime();
        self.collect_battery();
        self.update_load_avg();

        &self.info
    }

    fn collect_cpu_times(&mut self) {
        use windows::Win32::Foundation::FILETIME;

        // GetSystemTimes for aggregate totals
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
                    let cpu_pct = ((total_delta - idle_delta) * 100 / total_delta) as i64;
                    push_history(
                        self.info.cpu_percent.get_mut("total").unwrap(),
                        cpu_pct,
                    );

                    let user_pct = (user_delta * 100 / total_delta) as i64;
                    push_history(
                        self.info.cpu_percent.get_mut("user").unwrap(),
                        user_pct,
                    );

                    let system_pct = ((kernel_delta - idle_delta) * 100 / total_delta) as i64;
                    push_history(
                        self.info.cpu_percent.get_mut("system").unwrap(),
                        system_pct.max(0),
                    );

                    let idle_pct = (idle_delta * 100 / total_delta) as i64;
                    push_history(
                        self.info.cpu_percent.get_mut("idle").unwrap(),
                        idle_pct,
                    );
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
            let actual_count = (return_len as usize / std::mem::size_of::<ProcessorPerfInfo>())
                .min(core_count);

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

            for i in 0..actual_count {
                let pi = &perf_info[i];
                let idle_val = pi.idle_time as u64;
                let kernel_val = pi.kernel_time as u64;
                let user_val = pi.user_time as u64;

                // Index i+1 in prev arrays (index 0 is the aggregate)
                let pi_idx = i + 1;

                let idle_delta = idle_val.saturating_sub(self.prev_idle[pi_idx]);
                let kernel_delta = kernel_val.saturating_sub(self.prev_kernel[pi_idx]);
                let user_delta = user_val.saturating_sub(self.prev_user[pi_idx]);
                let total_delta = kernel_delta + user_delta;

                let core_pct = if total_delta > 0 {
                    ((total_delta - idle_delta) * 100 / total_delta) as i64
                } else {
                    0
                };

                push_history(&mut self.info.core_percent[i], core_pct);

                self.prev_idle[pi_idx] = idle_val;
                self.prev_kernel[pi_idx] = kernel_val;
                self.prev_user[pi_idx] = user_val;
            }
        }
    }

    fn collect_frequency(&mut self) {
        // Read from registry
        use windows::Win32::System::Registry::*;
        use windows::core::*;

        unsafe {
            let mut key = Default::default();
            let subkey = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, Some(0), KEY_READ, &mut key).is_ok() {
                let mut mhz: u32 = 0;
                let mut size = mem::size_of::<u32>() as u32;
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
        self.info.uptime_seconds = unsafe {
            windows::Win32::System::SystemInformation::GetTickCount64() / 1000
        };
    }

    fn collect_battery(&mut self) {
        use windows::Win32::System::Power::*;

        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe {
            if GetSystemPowerStatus(&mut status).is_ok() {
                self.info.has_battery = status.BatteryFlag != 128; // 128 = no battery
                self.info.battery = BatteryInfo {
                    percent: if status.BatteryLifePercent <= 100 {
                        status.BatteryLifePercent as i32
                    } else {
                        -1
                    },
                    watts: 0.0,
                    seconds_remaining: if status.BatteryLifeTime != u32::MAX {
                        status.BatteryLifeTime as i64
                    } else {
                        -1
                    },
                    status: match status.ACLineStatus {
                        0 => {
                            if status.BatteryFlag & 8 != 0 {
                                "Charging".to_string()
                            } else {
                                "Discharging".to_string()
                            }
                        }
                        1 => "Full".to_string(),
                        _ => "Unknown".to_string(),
                    },
                    ac_connected: status.ACLineStatus == 1,
                };
            }
        }
    }

    fn update_load_avg(&mut self) {
        if let Some(total) = self.info.cpu_percent.get("total") {
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
    }
}

fn get_cpu_name() -> String {
    use windows::Win32::System::Registry::*;
    use windows::core::*;

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

/// Calculate CPU percentage from time deltas (for unit testing).
pub fn calculate_cpu_percent(idle_delta: u64, kernel_delta: u64, user_delta: u64) -> i64 {
    let total = kernel_delta + user_delta;
    if total == 0 {
        return 0;
    }
    ((total - idle_delta) * 100 / total) as i64
}

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
    #[ignore] // Requires real Windows system
    fn collect_returns_valid_cpu_info() {
        let mut collector = CpuCollector::new();
        collector.init();
        assert!(collector.info.core_count > 0);
        assert!(!collector.info.cpu_name.is_empty());
    }
}
