use crate::collect::win::{OwnedLibrary, percent_u64};
use crate::domain::gpu::GpuInfo;
use std::ffi::c_void;

use super::{GpuBackend, clamp_percent, power_percent, push_history};

const ADL_OK: i32 = 0;
const ADL_MAX_PATH: usize = 256;

// ---------------------------------------------------------------------------
// PMLog sensor IDs (from ADL SDK adl_defines.h: ADL_PMLOG_SENSORS enum)
// ---------------------------------------------------------------------------

const ADL_PMLOG_MAX_SENSORS: usize = 256;

/// GPU core clock in MHz.
const SENSOR_CLK_GFXCLK: usize = 1;
/// GPU edge temperature in °C.
const SENSOR_TEMPERATURE_EDGE: usize = 8;
/// GPU utilization in % (0–100).
const SENSOR_ACTIVITY_GFX: usize = 19;
/// Total ASIC power in watts (pre-RDNA3).
const SENSOR_ASIC_POWER: usize = 23;
/// Total board power in watts (RDNA3+).
const SENSOR_BOARD_POWER: usize = 73;

// ---------------------------------------------------------------------------
// ADL structure definitions (repr(C) matching ADL SDK headers)
// ---------------------------------------------------------------------------

#[repr(C)]
struct AdapterInfo {
    i_size: i32,
    i_adapter_index: i32,
    str_udid: [u8; ADL_MAX_PATH],
    i_bus_number: i32,
    i_device_number: i32,
    i_function_number: i32,
    i_vendor_id: i32,
    str_adapter_name: [u8; ADL_MAX_PATH],
    str_display_name: [u8; ADL_MAX_PATH],
    i_present: i32,
    i_exist: i32,
    str_driver_path: [u8; ADL_MAX_PATH],
    str_driver_path_ext: [u8; ADL_MAX_PATH],
    str_pnp_string: [u8; ADL_MAX_PATH],
    i_os_display_index: i32,
}

impl AdapterInfo {
    fn zeroed() -> Self {
        // SAFETY: AdapterInfo is repr(C) with only integer and byte-array fields.
        // All-zeros is a valid representation.
        unsafe { std::mem::zeroed() }
    }
}

/// Per-sensor data returned by `ADL2_New_QueryPMLogData_Get`.
/// `supported` is non-zero when the sensor is active; `value` holds the
/// reading in sensor-specific units (MHz, °C, %, or watts).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AdlSingleSensorData {
    supported: i32,
    value: i32,
}

/// Output struct for `ADL2_New_QueryPMLogData_Get`.
/// Sensors are indexed directly by their `ADL_PMLOG_SENSORS` enum value.
#[repr(C)]
struct AdlPMLogDataOutput {
    size: i32,
    sensors: [AdlSingleSensorData; ADL_PMLOG_MAX_SENSORS],
}

impl AdlPMLogDataOutput {
    fn zeroed() -> Self {
        // SAFETY: repr(C) with only integer fields. All-zeros is valid.
        unsafe { std::mem::zeroed() }
    }

    /// Read a sensor value if supported. Returns `None` if the sensor is
    /// not present or the index is out of range.
    fn get(&self, sensor_id: usize) -> Option<i32> {
        let s = self.sensors.get(sensor_id)?;
        (s.supported != 0).then_some(s.value)
    }
}

#[repr(C)]
struct AdlMemoryInfoX4 {
    i_memory_size: i64,
    str_memory_type: [u8; ADL_MAX_PATH],
    i_memory_bandwidth: i64,
    i_hyperlink_memory_size: i64,
    i_invisible_memory_size: i64,
    i_visible_memory_size: i64,
    i_vram_vendor_rev_id: i64,
}

impl AdlMemoryInfoX4 {
    fn zeroed() -> Self {
        // SAFETY: repr(C) with only integer and byte-array fields.
        unsafe { std::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// ADL function pointer types
// ---------------------------------------------------------------------------

/// ADL memory allocation callback. ADL requires the caller to provide malloc.
type AdlMallocCallback = unsafe extern "C" fn(i32) -> *mut c_void;

type Adl2MainControlCreate = unsafe extern "C" fn(AdlMallocCallback, i32, *mut *mut c_void) -> i32;
type Adl2MainControlDestroy = unsafe extern "C" fn(*mut c_void) -> i32;
type Adl2AdapterNumberOfAdaptersGet = unsafe extern "C" fn(*mut c_void, *mut i32) -> i32;
type Adl2AdapterAdapterInfoGet = unsafe extern "C" fn(*mut c_void, *mut AdapterInfo, i32) -> i32;
type Adl2AdapterActiveGet = unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> i32;
type Adl2NewQueryPMLogDataGet =
    unsafe extern "C" fn(*mut c_void, i32, *mut AdlPMLogDataOutput) -> i32;
type Adl2AdapterMemoryInfoX4Get =
    unsafe extern "C" fn(*mut c_void, i32, *mut AdlMemoryInfoX4) -> i32;
type Adl2AdapterDedicatedVRAMUsageGet = unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> i32;

struct AdlFunctions {
    _library: OwnedLibrary,
    main_control_create: Adl2MainControlCreate,
    main_control_destroy: Adl2MainControlDestroy,
    adapter_number_of_adapters_get: Adl2AdapterNumberOfAdaptersGet,
    adapter_adapter_info_get: Adl2AdapterAdapterInfoGet,
    adapter_active_get: Adl2AdapterActiveGet,
    query_pmlog_data_get: Adl2NewQueryPMLogDataGet,
    adapter_memory_info_x4_get: Adl2AdapterMemoryInfoX4Get,
    dedicated_vram_usage_get: Adl2AdapterDedicatedVRAMUsageGet,
}

/// ADL malloc callback — allocates memory using the global allocator.
unsafe extern "C" fn adl_malloc(size: i32) -> *mut c_void {
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
    // SAFETY: layout has non-zero size (ADL only calls this with positive sizes).
    unsafe { std::alloc::alloc(layout) as *mut c_void }
}

impl AdlFunctions {
    fn load() -> Option<Self> {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::PCSTR;

        let dll_name: Vec<u16> = "atiadlxx.dll\0".encode_utf16().collect();
        // SAFETY: LoadLibraryW receives a valid null-terminated UTF-16 DLL name.
        let library = OwnedLibrary::new(
            unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())) }.ok()?,
        )?;
        let handle = library.get();

        macro_rules! load_fn {
            ($name:literal, $ty:ty) => {{
                // SAFETY: handle is a loaded atiadlxx.dll module; symbol name
                // is a static null-terminated string.
                let proc = unsafe { GetProcAddress(handle, PCSTR(concat!($name, "\0").as_ptr())) }?;
                // SAFETY: GetProcAddress returned a non-null address matching
                // the documented ADL function signature.
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc) }
            }};
        }

        Some(Self {
            _library: library,
            main_control_create: load_fn!("ADL2_Main_Control_Create", Adl2MainControlCreate),
            main_control_destroy: load_fn!("ADL2_Main_Control_Destroy", Adl2MainControlDestroy),
            adapter_number_of_adapters_get: load_fn!(
                "ADL2_Adapter_NumberOfAdapters_Get",
                Adl2AdapterNumberOfAdaptersGet
            ),
            adapter_adapter_info_get: load_fn!(
                "ADL2_Adapter_AdapterInfo_Get",
                Adl2AdapterAdapterInfoGet
            ),
            adapter_active_get: load_fn!("ADL2_Adapter_Active_Get", Adl2AdapterActiveGet),
            query_pmlog_data_get: load_fn!("ADL2_New_QueryPMLogData_Get", Adl2NewQueryPMLogDataGet),
            adapter_memory_info_x4_get: load_fn!(
                "ADL2_Adapter_MemoryInfoX4_Get",
                Adl2AdapterMemoryInfoX4Get
            ),
            dedicated_vram_usage_get: load_fn!(
                "ADL2_Adapter_DedicatedVRAMUsage_Get",
                Adl2AdapterDedicatedVRAMUsageGet
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// AMD backend
// ---------------------------------------------------------------------------

pub(super) struct AmdBackend {
    adl: AdlFunctions,
    context: *mut c_void,
    adapter_indices: Vec<i32>,
}

impl AmdBackend {
    pub(super) fn load() -> Option<Self> {
        let adl = AdlFunctions::load()?;

        let mut context: *mut c_void = std::ptr::null_mut();
        // SAFETY: adl.main_control_create was loaded from atiadlxx.dll.
        // adl_malloc is a valid callback; 1 = enumerate connected adapters only.
        let ret = unsafe { (adl.main_control_create)(adl_malloc, 1, &mut context) };
        if ret != ADL_OK {
            tracing::warn!("ADL2_Main_Control_Create failed with error {ret}");
            return None;
        }

        Some(Self {
            adl,
            context,
            adapter_indices: Vec::new(),
        })
    }
}

/// Extract a C string from a fixed-size byte buffer.
fn string_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

impl GpuBackend for AmdBackend {
    fn init_devices(&mut self) -> Vec<GpuInfo> {
        let mut num_adapters: i32 = 0;
        // SAFETY: context is valid; num_adapters is a valid pointer.
        let ret =
            unsafe { (self.adl.adapter_number_of_adapters_get)(self.context, &mut num_adapters) };
        if ret != ADL_OK || num_adapters <= 0 {
            return Vec::new();
        }

        let count = num_adapters as usize;
        let mut adapter_infos: Vec<AdapterInfo> =
            (0..count).map(|_| AdapterInfo::zeroed()).collect();
        let buf_size = (count * std::mem::size_of::<AdapterInfo>()) as i32;
        // SAFETY: adapter_infos is a valid buffer of count AdapterInfo structs.
        let ret = unsafe {
            (self.adl.adapter_adapter_info_get)(self.context, adapter_infos.as_mut_ptr(), buf_size)
        };
        if ret != ADL_OK {
            return Vec::new();
        }

        let mut gpus = Vec::new();
        let mut seen_bus = std::collections::HashSet::new();

        for info in &adapter_infos {
            // Skip inactive adapters.
            let mut active: i32 = 0;
            // SAFETY: context is valid; adapter index from ADL; active is valid pointer.
            let ret = unsafe {
                (self.adl.adapter_active_get)(self.context, info.i_adapter_index, &mut active)
            };
            if ret != ADL_OK || active == 0 {
                continue;
            }

            // Deduplicate by PCI bus number (ADL reports multiple entries per GPU).
            if !seen_bus.insert(info.i_bus_number) {
                continue;
            }

            let idx = info.i_adapter_index;
            self.adapter_indices.push(idx);

            let mut gpu = GpuInfo {
                name: string_from_buf(&info.str_adapter_name),
                ..GpuInfo::default()
            };

            // Query initial PMLog snapshot for power limit reference.
            let mut pmlog = AdlPMLogDataOutput::zeroed();
            // SAFETY: context and idx are valid; pmlog is a valid pointer.
            let ret = unsafe { (self.adl.query_pmlog_data_get)(self.context, idx, &mut pmlog) };
            if ret == ADL_OK {
                // Use board power as the max reference (best available from
                // a single PMLog snapshot at init — actual TDP is not exposed
                // via PMLog, so we rely on the live reading at startup and
                // let the meter scale naturally).
                let power_w = pmlog
                    .get(SENSOR_BOARD_POWER)
                    .or_else(|| pmlog.get(SENSOR_ASIC_POWER));
                if let Some(w) = power_w {
                    // Sensor reports watts; domain stores milliwatts.
                    // Use the initial reading as a baseline — the UI meter
                    // will rescale if power exceeds this on later samples.
                    gpu.pwr_max_usage = w.max(1) as i64 * 1000;
                }
            }

            // Query total VRAM.
            let mut mem = AdlMemoryInfoX4::zeroed();
            // SAFETY: context and idx are valid; mem is a valid pointer.
            let ret = unsafe { (self.adl.adapter_memory_info_x4_get)(self.context, idx, &mut mem) };
            if ret == ADL_OK && mem.i_memory_size > 0 {
                gpu.mem_total = (mem.i_memory_size * 1024 * 1024) as u64;
            }

            gpus.push(gpu);
        }

        gpus
    }

    fn collect(&mut self, gpus: &mut [GpuInfo]) {
        for (gpu, &idx) in gpus.iter_mut().zip(self.adapter_indices.iter()) {
            // PMLog one-shot query — provides utilization, clocks, temp, power.
            let mut pmlog = AdlPMLogDataOutput::zeroed();
            // SAFETY: context and idx are valid; pmlog is a valid pointer.
            let ret = unsafe { (self.adl.query_pmlog_data_get)(self.context, idx, &mut pmlog) };
            if ret == ADL_OK {
                // Utilization (direct %).
                if let Some(pct) = pmlog.get(SENSOR_ACTIVITY_GFX) {
                    push_history(
                        &mut gpu.gpu_percent.utilization,
                        clamp_percent(pct.max(0) as u32),
                    );
                }

                // Clock speed (direct MHz).
                if let Some(mhz) = pmlog.get(SENSOR_CLK_GFXCLK) {
                    gpu.gpu_clock_speed = mhz.max(0) as u32;
                }

                // Temperature (direct °C).
                if let Some(temp) = pmlog.get(SENSOR_TEMPERATURE_EDGE) {
                    push_history(&mut gpu.temp, temp as i64);
                }

                // Power — prefer board power (RDNA3+), fall back to ASIC power.
                // Sensor values are in watts; domain stores milliwatts.
                let power_w = pmlog
                    .get(SENSOR_BOARD_POWER)
                    .or_else(|| pmlog.get(SENSOR_ASIC_POWER));
                if let Some(w) = power_w {
                    let power_mw = w.max(0) as u64 * 1000;
                    gpu.pwr_usage = power_mw as i64;
                    let pwr_pct = power_percent(power_mw, gpu.pwr_max_usage as u64);
                    push_history(&mut gpu.gpu_percent.power, pwr_pct);
                }
            } else {
                tracing::debug!("ADL PMLog query failed with error {ret}");
            }

            // VRAM usage (direct MB from ADL).
            let mut vram_used_mb: i32 = 0;
            // SAFETY: context and idx are valid; vram_used_mb is a valid pointer.
            let ret = unsafe {
                (self.adl.dedicated_vram_usage_get)(self.context, idx, &mut vram_used_mb)
            };
            if ret == ADL_OK && vram_used_mb > 0 {
                gpu.mem_used = vram_used_mb as u64 * 1024 * 1024;
                let vram_pct = percent_u64(gpu.mem_used, gpu.mem_total).min(100);
                push_history(&mut gpu.gpu_percent.vram, vram_pct);
                push_history(&mut gpu.mem_utilization_percent, vram_pct);
            } else if ret != ADL_OK {
                tracing::debug!("ADL VRAM usage query failed with error {ret}");
            }
        }
    }

    fn shutdown(&mut self) {
        if !self.context.is_null() {
            // SAFETY: context was created by ADL2_Main_Control_Create.
            unsafe {
                let _ = (self.adl.main_control_destroy)(self.context);
            }
            self.context = std::ptr::null_mut();
        }
    }
}
