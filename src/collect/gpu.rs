use crate::domain::gpu::GpuInfo;
use std::ffi::{c_char, c_void};

use super::Collector;

const MAX_HISTORY: usize = 300;
const NVML_SUCCESS: u32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

type NvmlDevice = *mut c_void;

// Function pointer types for NVML functions
type NvmlInitV2 = unsafe extern "C" fn() -> u32;
type NvmlShutdownFn = unsafe extern "C" fn() -> u32;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut u32) -> u32;
type NvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(u32, *mut NvmlDevice) -> u32;
type NvmlDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut c_char, u32) -> u32;
type NvmlDeviceGetUtilizationRates = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> u32;
type NvmlDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> u32;
type NvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> u32;
type NvmlDeviceGetPowerUsage = unsafe extern "C" fn(NvmlDevice, *mut u32) -> u32;
type NvmlDeviceGetClockInfo = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> u32;
type NvmlDeviceGetMaxClockInfo = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> u32;
type NvmlDeviceGetPowerManagementLimit = unsafe extern "C" fn(NvmlDevice, *mut u32) -> u32;

struct NvmlFunctions {
    _handle: windows::Win32::Foundation::HMODULE,
    init: NvmlInitV2,
    shutdown: NvmlShutdownFn,
    device_get_count: NvmlDeviceGetCountV2,
    device_get_handle_by_index: NvmlDeviceGetHandleByIndexV2,
    device_get_name: NvmlDeviceGetName,
    device_get_utilization_rates: NvmlDeviceGetUtilizationRates,
    device_get_temperature: NvmlDeviceGetTemperature,
    device_get_memory_info: NvmlDeviceGetMemoryInfo,
    device_get_power_usage: NvmlDeviceGetPowerUsage,
    device_get_clock_info: NvmlDeviceGetClockInfo,
    device_get_max_clock_info: NvmlDeviceGetMaxClockInfo,
    device_get_power_management_limit: NvmlDeviceGetPowerManagementLimit,
}

impl NvmlFunctions {
    /// Try to load nvml.dll and resolve all function pointers.
    /// Returns None if the library is not found or any function is missing.
    fn load() -> Option<Self> {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::PCSTR;

        let dll_name: Vec<u16> = "nvml.dll\0".encode_utf16().collect();
        let handle = unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())).ok()? };

        unsafe {
            let load_fn = |name: &[u8]| -> Option<unsafe extern "C" fn()> {
                let proc = GetProcAddress(handle, PCSTR(name.as_ptr()));
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "C" fn(),
                >(proc?))
            };

            macro_rules! nvml_fn {
                ($name:expr) => {{ load_fn(concat!($name, "\0").as_bytes())? }};
            }

            Some(Self {
                _handle: handle,
                init: std::mem::transmute::<unsafe extern "C" fn(), NvmlInitV2>(nvml_fn!(
                    "nvmlInit_v2"
                )),
                shutdown: std::mem::transmute::<unsafe extern "C" fn(), NvmlShutdownFn>(nvml_fn!(
                    "nvmlShutdown"
                )),
                device_get_count: std::mem::transmute::<unsafe extern "C" fn(), NvmlDeviceGetCountV2>(
                    nvml_fn!("nvmlDeviceGetCount_v2"),
                ),
                device_get_handle_by_index: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetHandleByIndexV2,
                >(nvml_fn!(
                    "nvmlDeviceGetHandleByIndex_v2"
                )),
                device_get_name: std::mem::transmute::<unsafe extern "C" fn(), NvmlDeviceGetName>(
                    nvml_fn!("nvmlDeviceGetName"),
                ),
                device_get_utilization_rates: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetUtilizationRates,
                >(nvml_fn!(
                    "nvmlDeviceGetUtilizationRates"
                )),
                device_get_temperature: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetTemperature,
                >(nvml_fn!("nvmlDeviceGetTemperature")),
                device_get_memory_info: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetMemoryInfo,
                >(nvml_fn!("nvmlDeviceGetMemoryInfo")),
                device_get_power_usage: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetPowerUsage,
                >(nvml_fn!("nvmlDeviceGetPowerUsage")),
                device_get_clock_info: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetClockInfo,
                >(nvml_fn!("nvmlDeviceGetClockInfo")),
                device_get_max_clock_info: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetMaxClockInfo,
                >(nvml_fn!("nvmlDeviceGetMaxClockInfo")),
                device_get_power_management_limit: std::mem::transmute::<
                    unsafe extern "C" fn(),
                    NvmlDeviceGetPowerManagementLimit,
                >(nvml_fn!(
                    "nvmlDeviceGetPowerManagementLimit"
                )),
            })
        }
    }
}

/// GPU data collector using NVML (dynamically loaded).
pub struct GpuCollector {
    nvml: Option<NvmlFunctions>,
    device_count: u32,
    devices: Vec<NvmlDevice>,
    pub gpus: Vec<GpuInfo>,
    initialized: bool,
}

impl GpuCollector {
    /// Create a new GPU collector, loading NVML if available.
    pub fn new() -> Self {
        let mut collector = Self {
            nvml: NvmlFunctions::load(),
            device_count: 0,
            devices: Vec::new(),
            gpus: Vec::new(),
            initialized: false,
        };
        collector.init();
        collector
    }

    fn init(&mut self) {
        let Some(nvml) = &self.nvml else {
            return;
        };

        let ret = unsafe { (nvml.init)() };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlInit_v2 failed with error {ret}");
            self.nvml = None;
            return;
        }

        let mut count: u32 = 0;
        let ret = unsafe { (nvml.device_get_count)(&mut count) };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlDeviceGetCount_v2 failed with error {ret}");
            self.nvml = None;
            return;
        }

        self.device_count = count;
        self.devices = Vec::with_capacity(count as usize);
        self.gpus = Vec::with_capacity(count as usize);

        for i in 0..count {
            let mut device: NvmlDevice = std::ptr::null_mut();
            let ret = unsafe { (nvml.device_get_handle_by_index)(i, &mut device) };
            if ret != NVML_SUCCESS {
                continue;
            }
            self.devices.push(device);

            let mut info = GpuInfo::default();

            // Get device name
            let mut name_buf = [0u8; 256];
            let ret = unsafe {
                (nvml.device_get_name)(device, name_buf.as_mut_ptr() as *mut c_char, 256)
            };
            if ret == NVML_SUCCESS {
                let name = unsafe { std::ffi::CStr::from_ptr(name_buf.as_ptr() as *const c_char) };
                info.name = name.to_string_lossy().into_owned();
            }

            // Get power management limit
            let mut pwr_limit: u32 = 0;
            let ret = unsafe { (nvml.device_get_power_management_limit)(device, &mut pwr_limit) };
            if ret == NVML_SUCCESS {
                info.pwr_max_usage = pwr_limit as i64;
            }

            // Get max GPU clock speed
            let mut max_clock: u32 = 0;
            let ret = unsafe {
                (nvml.device_get_max_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut max_clock)
            };
            if ret == NVML_SUCCESS {
                info.gpu_max_clock_speed = max_clock;
            }

            self.gpus.push(info);
        }

        self.initialized = true;
    }

    /// Collect current GPU metrics for all detected devices.
    fn collect_impl(&mut self) {
        let Some(nvml) = &self.nvml else {
            return;
        };

        for (i, &device) in self.devices.iter().enumerate() {
            let gpu = &mut self.gpus[i];

            // Utilization rates
            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            let ret = unsafe { (nvml.device_get_utilization_rates)(device, &mut util) };
            if ret == NVML_SUCCESS {
                let pct = util.gpu as i64;
                let totals = &mut gpu.gpu_percent.utilization;
                totals.push_back(pct);
                if totals.len() > MAX_HISTORY {
                    totals.pop_front();
                }
            }

            // Temperature
            let mut temp: u32 = 0;
            let ret =
                unsafe { (nvml.device_get_temperature)(device, NVML_TEMPERATURE_GPU, &mut temp) };
            if ret == NVML_SUCCESS {
                gpu.temp.push_back(temp as i64);
                if gpu.temp.len() > MAX_HISTORY {
                    gpu.temp.pop_front();
                }
            }

            // Memory info
            let mut mem = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            let ret = unsafe { (nvml.device_get_memory_info)(device, &mut mem) };
            if ret == NVML_SUCCESS {
                gpu.mem_total = mem.total;
                gpu.mem_used = mem.used;
                let vram_pct = if mem.total > 0 {
                    (mem.used as f64 / mem.total as f64 * 100.0) as i64
                } else {
                    0
                };
                let vram_hist = &mut gpu.gpu_percent.vram;
                vram_hist.push_back(vram_pct);
                if vram_hist.len() > MAX_HISTORY {
                    vram_hist.pop_front();
                }
                gpu.mem_utilization_percent.push_back(vram_pct);
                if gpu.mem_utilization_percent.len() > MAX_HISTORY {
                    gpu.mem_utilization_percent.pop_front();
                }
            }

            // Power usage (milliwatts)
            let mut power_mw: u32 = 0;
            let ret = unsafe { (nvml.device_get_power_usage)(device, &mut power_mw) };
            if ret == NVML_SUCCESS {
                gpu.pwr_usage = power_mw as i64;
                let pwr_pct = if gpu.pwr_max_usage > 0 {
                    (power_mw as i64 * 100 / gpu.pwr_max_usage).min(100)
                } else {
                    0
                };
                let pwr_hist = &mut gpu.gpu_percent.power;
                pwr_hist.push_back(pwr_pct);
                if pwr_hist.len() > MAX_HISTORY {
                    pwr_hist.pop_front();
                }
            }

            // Clock speed (graphics)
            let mut clock: u32 = 0;
            let ret =
                unsafe { (nvml.device_get_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut clock) };
            if ret == NVML_SUCCESS {
                gpu.gpu_clock_speed = clock;
            }
        }
    }

    /// Returns the number of detected GPU devices.
    pub fn gpu_count(&self) -> usize {
        self.gpus.len()
    }

    /// Shutdown NVML cleanly.
    pub fn shutdown(&mut self) {
        if let Some(nvml) = &self.nvml {
            unsafe {
                (nvml.shutdown)();
            }
        }
        self.nvml = None;
    }
}

impl Default for GpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for GpuCollector {
    fn collect(&mut self) {
        self.collect_impl();
    }
}

impl Drop for GpuCollector {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_collector_new_does_not_panic() {
        // Should silently handle missing nvml.dll
        let collector = GpuCollector::new();
        let _ = collector.gpu_count();
    }

    #[test]
    fn gpu_collector_collect_without_nvml() {
        let mut collector = GpuCollector {
            nvml: None,
            device_count: 0,
            devices: Vec::new(),
            gpus: Vec::new(),
            initialized: false,
        };
        collector.collect(); // should not panic
    }
}
