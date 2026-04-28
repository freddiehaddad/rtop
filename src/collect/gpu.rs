use crate::domain::gpu::GpuInfo;
use std::ffi::{c_char, c_void};

use super::{
    Collector,
    win::{OwnedLibrary, percent_u64, string_from_c_buf},
};

const MAX_HISTORY: usize = 300;
const NVML_SUCCESS: u32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const MAX_NVML_DEVICES: u32 = 32;

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
    _library: OwnedLibrary,
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
        // SAFETY: LoadLibraryW receives a valid null-terminated UTF-16 DLL name.
        // The returned handle is checked via ok()? before use.
        let library = OwnedLibrary::new(
            unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())) }.ok()?,
        )?;
        let handle = library.get();

        macro_rules! load_nvml_fn {
            ($name:literal, $ty:ty) => {{
                // SAFETY: handle is an owned, loaded nvml.dll module and the
                // symbol name is a static null-terminated byte string.
                let proc = unsafe { GetProcAddress(handle, PCSTR(concat!($name, "\0").as_ptr())) }?;
                // SAFETY: GetProcAddress returned a non-null address from the
                // loaded nvml.dll. The requested symbol name and target type are
                // paired at this call site with the documented NVML signature.
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc) }
            }};
        }

        Some(Self {
            _library: library,
            init: load_nvml_fn!("nvmlInit_v2", NvmlInitV2),
            shutdown: load_nvml_fn!("nvmlShutdown", NvmlShutdownFn),
            device_get_count: load_nvml_fn!("nvmlDeviceGetCount_v2", NvmlDeviceGetCountV2),
            device_get_handle_by_index: load_nvml_fn!(
                "nvmlDeviceGetHandleByIndex_v2",
                NvmlDeviceGetHandleByIndexV2
            ),
            device_get_name: load_nvml_fn!("nvmlDeviceGetName", NvmlDeviceGetName),
            device_get_utilization_rates: load_nvml_fn!(
                "nvmlDeviceGetUtilizationRates",
                NvmlDeviceGetUtilizationRates
            ),
            device_get_temperature: load_nvml_fn!(
                "nvmlDeviceGetTemperature",
                NvmlDeviceGetTemperature
            ),
            device_get_memory_info: load_nvml_fn!(
                "nvmlDeviceGetMemoryInfo",
                NvmlDeviceGetMemoryInfo
            ),
            device_get_power_usage: load_nvml_fn!(
                "nvmlDeviceGetPowerUsage",
                NvmlDeviceGetPowerUsage
            ),
            device_get_clock_info: load_nvml_fn!("nvmlDeviceGetClockInfo", NvmlDeviceGetClockInfo),
            device_get_max_clock_info: load_nvml_fn!(
                "nvmlDeviceGetMaxClockInfo",
                NvmlDeviceGetMaxClockInfo
            ),
            device_get_power_management_limit: load_nvml_fn!(
                "nvmlDeviceGetPowerManagementLimit",
                NvmlDeviceGetPowerManagementLimit
            ),
        })
    }
}

/// GPU data collector using NVML (dynamically loaded).
pub struct GpuCollector {
    nvml: Option<NvmlFunctions>,
    device_count: u32,
    devices: Vec<NvmlDevice>,
    pub gpus: Vec<GpuInfo>,
    pub status: super::CollectStatus,
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
            status: super::CollectStatus::Ok,
            initialized: false,
        };
        collector.init();
        collector
    }

    fn init(&mut self) {
        let Some(nvml) = &self.nvml else {
            return;
        };

        // SAFETY: nvml.init was loaded from nvml.dll and matches the
        // nvmlInit_v2 signature. It requires no arguments.
        let ret = unsafe { (nvml.init)() };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlInit_v2 failed with error {ret}");
            self.nvml = None;
            return;
        }

        let mut count: u32 = 0;
        // SAFETY: count is a valid pointer to a stack-allocated u32.
        let ret = unsafe { (nvml.device_get_count)(&mut count) };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlDeviceGetCount_v2 failed with error {ret}");
            // SAFETY: nvml.init succeeded, so shut NVML down before unloading
            // the library via OwnedLibrary. There is no recovery path if
            // shutdown itself fails during initialization cleanup.
            unsafe {
                let _ = (nvml.shutdown)();
            }
            self.nvml = None;
            return;
        }

        let count = count.min(MAX_NVML_DEVICES);
        self.device_count = count;
        self.devices = Vec::with_capacity(count as usize);
        self.gpus = Vec::with_capacity(count as usize);

        for i in 0..count {
            let mut device: NvmlDevice = std::ptr::null_mut();
            // SAFETY: i is within 0..count as reported by nvmlDeviceGetCount.
            // device is a valid pointer to a stack-allocated NvmlDevice.
            let ret = unsafe { (nvml.device_get_handle_by_index)(i, &mut device) };
            if ret != NVML_SUCCESS || device.is_null() {
                continue;
            }
            self.devices.push(device);

            let mut info = GpuInfo::default();

            // Get device name
            let mut name_buf = [0u8; 256];
            // SAFETY: device is a valid handle from nvmlDeviceGetHandleByIndex.
            // name_buf is a stack-allocated 256-byte array, matching the length
            // argument passed to the function.
            let ret = unsafe {
                (nvml.device_get_name)(
                    device,
                    name_buf.as_mut_ptr() as *mut c_char,
                    name_buf.len() as u32,
                )
            };
            if ret == NVML_SUCCESS {
                info.name = string_from_c_buf(&name_buf);
            }

            // Get power management limit
            let mut pwr_limit: u32 = 0;
            // SAFETY: device is a valid NVML handle; pwr_limit is a valid
            // pointer to a stack-allocated u32.
            let ret = unsafe { (nvml.device_get_power_management_limit)(device, &mut pwr_limit) };
            if ret == NVML_SUCCESS {
                info.pwr_max_usage = pwr_limit as i64;
            }

            // Get max GPU clock speed
            let mut max_clock: u32 = 0;
            // SAFETY: device is a valid NVML handle; NVML_CLOCK_GRAPHICS is a
            // valid clock type constant; max_clock is a valid u32 pointer.
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
        self.status = super::CollectStatus::Ok;

        let Some(nvml) = &self.nvml else {
            self.status = super::CollectStatus::Failed("no NVML");
            return;
        };

        for (i, &device) in self.devices.iter().enumerate() {
            let gpu = &mut self.gpus[i];

            // Utilization rates
            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            // SAFETY: device is a valid NVML handle obtained during init.
            // util is a valid pointer to a stack-allocated repr(C) struct.
            let ret = unsafe { (nvml.device_get_utilization_rates)(device, &mut util) };
            if ret == NVML_SUCCESS {
                let pct = percent_0_to_100(util.gpu);
                let totals = &mut gpu.gpu_percent.utilization;
                totals.push_back(pct);
                if totals.len() > MAX_HISTORY {
                    totals.pop_front();
                }
            }

            // Temperature
            let mut temp: u32 = 0;
            // SAFETY: device is a valid NVML handle; NVML_TEMPERATURE_GPU is
            // a valid sensor type; temp is a valid u32 pointer.
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
            // SAFETY: device is a valid NVML handle; mem is a valid pointer
            // to a stack-allocated repr(C) NvmlMemory struct.
            let ret = unsafe { (nvml.device_get_memory_info)(device, &mut mem) };
            if ret == NVML_SUCCESS {
                gpu.mem_total = mem.total;
                gpu.mem_used = mem.used.min(mem.total);
                let vram_pct = percent_u64(gpu.mem_used, mem.total).min(100);
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
            // SAFETY: device is a valid NVML handle; power_mw is a valid
            // pointer to a stack-allocated u32.
            let ret = unsafe { (nvml.device_get_power_usage)(device, &mut power_mw) };
            if ret == NVML_SUCCESS {
                gpu.pwr_usage = power_mw as i64;
                let pwr_pct = power_percent(power_mw, gpu.pwr_max_usage);
                let pwr_hist = &mut gpu.gpu_percent.power;
                pwr_hist.push_back(pwr_pct);
                if pwr_hist.len() > MAX_HISTORY {
                    pwr_hist.pop_front();
                }
            }

            // Clock speed (graphics)
            let mut clock: u32 = 0;
            // SAFETY: device is a valid NVML handle; NVML_CLOCK_GRAPHICS is a
            // valid clock type constant; clock is a valid u32 pointer.
            let ret =
                unsafe { (nvml.device_get_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut clock) };
            if ret == NVML_SUCCESS {
                gpu.gpu_clock_speed = clock;
            }
        }
    }

    /// Shutdown NVML cleanly.
    pub fn shutdown(&mut self) {
        if let Some(nvml) = &self.nvml {
            // SAFETY: nvml.shutdown was loaded from nvml.dll and matches the
            // nvmlShutdown signature. Called once during cleanup while the DLL
            // handle is still valid. Shutdown failures cannot be recovered
            // during cleanup, so the status is intentionally ignored.
            unsafe {
                let _ = (nvml.shutdown)();
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

fn percent_0_to_100(value: u32) -> i64 {
    value.min(100) as i64
}

fn power_percent(power_mw: u32, max_power_mw: i64) -> i64 {
    if max_power_mw <= 0 {
        return 0;
    }
    percent_u64(power_mw as u64, max_power_mw as u64).min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_collector_new_does_not_panic() {
        // Should silently handle missing nvml.dll
        let collector = GpuCollector::new();
        let _ = collector.gpus.len();
    }

    #[test]
    fn gpu_collector_collect_without_nvml() {
        let mut collector = GpuCollector {
            nvml: None,
            device_count: 0,
            devices: Vec::new(),
            gpus: Vec::new(),
            status: crate::collect::CollectStatus::Ok,
            initialized: false,
        };
        collector.collect(); // should not panic
        assert_eq!(
            collector.status,
            crate::collect::CollectStatus::Failed("no NVML")
        );
    }

    #[test]
    fn percent_0_to_100_clamps_utilization() {
        assert_eq!(percent_0_to_100(42), 42);
        assert_eq!(percent_0_to_100(150), 100);
    }

    #[test]
    fn power_percent_requires_positive_limit() {
        assert_eq!(power_percent(50, 0), 0);
        assert_eq!(power_percent(50, -1), 0);
        assert_eq!(power_percent(50, 100), 50);
        assert_eq!(power_percent(200, 100), 100);
    }
}
