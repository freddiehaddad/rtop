use crate::collect::win::{OwnedLibrary, percent_u64, string_from_c_buf};
use crate::domain::gpu::GpuInfo;
use std::ffi::{c_char, c_void};

use super::{GpuBackend, clamp_percent, power_percent, push_history};

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

        macro_rules! load_fn {
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
            init: load_fn!("nvmlInit_v2", NvmlInitV2),
            shutdown: load_fn!("nvmlShutdown", NvmlShutdownFn),
            device_get_count: load_fn!("nvmlDeviceGetCount_v2", NvmlDeviceGetCountV2),
            device_get_handle_by_index: load_fn!(
                "nvmlDeviceGetHandleByIndex_v2",
                NvmlDeviceGetHandleByIndexV2
            ),
            device_get_name: load_fn!("nvmlDeviceGetName", NvmlDeviceGetName),
            device_get_utilization_rates: load_fn!(
                "nvmlDeviceGetUtilizationRates",
                NvmlDeviceGetUtilizationRates
            ),
            device_get_temperature: load_fn!("nvmlDeviceGetTemperature", NvmlDeviceGetTemperature),
            device_get_memory_info: load_fn!("nvmlDeviceGetMemoryInfo", NvmlDeviceGetMemoryInfo),
            device_get_power_usage: load_fn!("nvmlDeviceGetPowerUsage", NvmlDeviceGetPowerUsage),
            device_get_clock_info: load_fn!("nvmlDeviceGetClockInfo", NvmlDeviceGetClockInfo),
            device_get_max_clock_info: load_fn!(
                "nvmlDeviceGetMaxClockInfo",
                NvmlDeviceGetMaxClockInfo
            ),
            device_get_power_management_limit: load_fn!(
                "nvmlDeviceGetPowerManagementLimit",
                NvmlDeviceGetPowerManagementLimit
            ),
        })
    }
}

pub(super) struct NvidiaBackend {
    nvml: NvmlFunctions,
    devices: Vec<NvmlDevice>,
    /// Tracks which metrics have already logged a failure (log once, not every cycle).
    logged: LoggedFailures,
}

#[derive(Default)]
struct LoggedFailures {
    utilization: bool,
    temperature: bool,
    memory: bool,
    power: bool,
    clock: bool,
}

impl NvidiaBackend {
    pub(super) fn load() -> Option<Self> {
        let nvml = NvmlFunctions::load()?;

        // SAFETY: nvml.init was loaded from nvml.dll and matches the
        // nvmlInit_v2 signature.
        let ret = unsafe { (nvml.init)() };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlInit_v2 failed with error {ret}");
            return None;
        }

        Some(Self {
            nvml,
            devices: Vec::new(),
            logged: LoggedFailures::default(),
        })
    }
}

impl GpuBackend for NvidiaBackend {
    fn init_devices(&mut self) -> Vec<GpuInfo> {
        let mut count: u32 = 0;
        // SAFETY: count is a valid pointer to a stack-allocated u32.
        let ret = unsafe { (self.nvml.device_get_count)(&mut count) };
        if ret != NVML_SUCCESS {
            tracing::warn!("nvmlDeviceGetCount_v2 failed with error {ret}");
            return Vec::new();
        }

        let count = count.min(MAX_NVML_DEVICES);
        let mut gpus = Vec::with_capacity(count as usize);

        for i in 0..count {
            let mut device: NvmlDevice = std::ptr::null_mut();
            // SAFETY: i is within 0..count as reported by nvmlDeviceGetCount.
            let ret = unsafe { (self.nvml.device_get_handle_by_index)(i, &mut device) };
            if ret != NVML_SUCCESS || device.is_null() {
                continue;
            }
            self.devices.push(device);

            let mut info = GpuInfo::default();

            // Device name
            let mut name_buf = [0u8; 256];
            // SAFETY: device is a valid handle; name_buf matches the length arg.
            let ret = unsafe {
                (self.nvml.device_get_name)(
                    device,
                    name_buf.as_mut_ptr() as *mut c_char,
                    name_buf.len() as u32,
                )
            };
            if ret == NVML_SUCCESS {
                info.name = string_from_c_buf(&name_buf);
            }

            // Power management limit
            let mut pwr_limit: u32 = 0;
            // SAFETY: device is a valid handle; pwr_limit is a valid pointer.
            let ret =
                unsafe { (self.nvml.device_get_power_management_limit)(device, &mut pwr_limit) };
            if ret == NVML_SUCCESS {
                info.pwr_max_usage = pwr_limit as i64;
            }

            // Max GPU clock speed
            let mut max_clock: u32 = 0;
            // SAFETY: device is a valid handle; NVML_CLOCK_GRAPHICS is valid.
            let ret = unsafe {
                (self.nvml.device_get_max_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut max_clock)
            };
            if ret == NVML_SUCCESS {
                info.gpu_max_clock_speed = max_clock;
            }

            gpus.push(info);
        }

        gpus
    }

    fn collect(&mut self, gpus: &mut [GpuInfo]) {
        for (gpu, &device) in gpus.iter_mut().zip(self.devices.iter()) {
            // Utilization
            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            // SAFETY: device is a valid handle obtained during init.
            let ret = unsafe { (self.nvml.device_get_utilization_rates)(device, &mut util) };
            if ret == NVML_SUCCESS {
                push_history(&mut gpu.gpu_percent.utilization, clamp_percent(util.gpu));
            } else if !self.logged.utilization {
                tracing::debug!("NVML utilization failed with error {ret}");
                self.logged.utilization = true;
            }

            // Temperature
            let mut temp: u32 = 0;
            // SAFETY: device is valid; NVML_TEMPERATURE_GPU is valid.
            let ret = unsafe {
                (self.nvml.device_get_temperature)(device, NVML_TEMPERATURE_GPU, &mut temp)
            };
            if ret == NVML_SUCCESS {
                push_history(&mut gpu.temp, temp as i64);
            } else if !self.logged.temperature {
                tracing::debug!("NVML temperature failed with error {ret}");
                self.logged.temperature = true;
            }

            // Memory
            let mut mem = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            // SAFETY: device is valid; mem is a valid repr(C) struct pointer.
            let ret = unsafe { (self.nvml.device_get_memory_info)(device, &mut mem) };
            if ret == NVML_SUCCESS {
                gpu.mem_total = mem.total;
                gpu.mem_used = mem.used.min(mem.total);
                let vram_pct = percent_u64(gpu.mem_used, mem.total).min(100);
                push_history(&mut gpu.gpu_percent.vram, vram_pct);
                push_history(&mut gpu.mem_utilization_percent, vram_pct);
            } else if !self.logged.memory {
                tracing::debug!("NVML memory failed with error {ret}");
                self.logged.memory = true;
            }

            // Power (milliwatts)
            let mut power_mw: u32 = 0;
            // SAFETY: device is valid; power_mw is a valid pointer.
            let ret = unsafe { (self.nvml.device_get_power_usage)(device, &mut power_mw) };
            if ret == NVML_SUCCESS {
                gpu.pwr_usage = power_mw as i64;
                let pwr_pct = power_percent(power_mw as u64, gpu.pwr_max_usage as u64);
                push_history(&mut gpu.gpu_percent.power, pwr_pct);
            } else if !self.logged.power {
                tracing::debug!("NVML power failed with error {ret}");
                self.logged.power = true;
            }

            // Clock speed
            let mut clock: u32 = 0;
            // SAFETY: device is valid; NVML_CLOCK_GRAPHICS is valid.
            let ret = unsafe {
                (self.nvml.device_get_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut clock)
            };
            if ret == NVML_SUCCESS {
                gpu.gpu_clock_speed = clock;
            } else if !self.logged.clock {
                tracing::debug!("NVML clock failed with error {ret}");
                self.logged.clock = true;
            }
        }
    }

    fn shutdown(&mut self) {
        // SAFETY: nvml.shutdown matches the nvmlShutdown signature. Called once
        // during cleanup while the DLL handle is still valid.
        unsafe {
            let _ = (self.nvml.shutdown)();
        }
    }
}
