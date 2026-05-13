use crate::collect::CollectStatus;
use crate::collect::win::OwnedLibrary;
use std::ffi::c_void;

use super::{clamp_percent, push_history};

const CTL_RESULT_SUCCESS: u32 = 0;
const CTL_IMPL_VERSION: u32 = (1 << 16) | 1;
const CTL_INIT_FLAG_USE_LEVEL_ZERO: u32 = 1;
const CTL_MAX_DEVICE_NAME: usize = 256;

// ---------------------------------------------------------------------------
// IGCL opaque handle types (all are pointers)
// ---------------------------------------------------------------------------

type CtlApiHandle = *mut c_void;
type CtlDeviceHandle = *mut c_void;
type CtlTempHandle = *mut c_void;
type CtlMemHandle = *mut c_void;
type CtlEngineHandle = *mut c_void;
type CtlFreqHandle = *mut c_void;
type CtlPwrHandle = *mut c_void;

// ---------------------------------------------------------------------------
// IGCL structures (repr(C) matching igcl_api.h)
// ---------------------------------------------------------------------------

#[repr(C)]
struct CtlInitArgs {
    size: u32,
    version: u32,
    flags: u32,
    reserved: u32,
    app_uid: [u8; 16],
    app_version: u32,
    p_reserved: *mut c_void,
}

#[repr(C)]
struct CtlDeviceAdapterProperties {
    size: u32,
    device_id_size: u32,
    p_device_id: *mut c_void,
    vendor_id: u32,
    device_id: u32,
    rev_id: u32,
    num_eus_per_sub_slice: u32,
    num_sub_slices_per_slice: u32,
    num_slices: u32,
    name: [u8; CTL_MAX_DEVICE_NAME],
    graphics_adapter_properties: u32,
    frequency: u32,
    pci_vendor_id: u32,
    pci_device_id: u32,
    pci_sub_sys_id: u32,
    pci_rev_id: u32,
    adapter_type: u32,
}

#[repr(C)]
#[derive(Default)]
struct CtlTempState {
    temperature: f64,
}

#[repr(C)]
#[derive(Default)]
struct CtlMemState {
    size: u64,
    free: u64,
}

#[repr(C)]
#[derive(Default)]
struct CtlEngineActivity {
    active_time: u64,
    timestamp: u64,
}

#[repr(C)]
#[derive(Default)]
struct CtlFreqState {
    current_frequency: f64,
    tdp_frequency: f64,
    efficient_frequency: f64,
    actual_frequency: f64,
    throttle_reasons: u32,
}

#[repr(C)]
#[derive(Default)]
struct CtlPowerProperties {
    on_subdevice: u32,
    subdevice_id: u32,
    can_control: u32,
    is_energy_threshold_supported: u32,
    default_limit: i32,
    min_limit: i32,
    max_limit: i32,
}

const CTL_PSU_COUNT: usize = 5;
const CTL_FAN_COUNT: usize = 5;

/// Telemetry value union — largest member is `f64` (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlDataValue {
    datadouble: f64,
}

/// Single telemetry item with support flag and typed value.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlTelemetryItem {
    supported: u32,
    units: u32,
    data_type: u32,
    value: CtlDataValue,
}

impl CtlTelemetryItem {
    fn get(&self) -> Option<f64> {
        (self.supported != 0).then_some(self.value.datadouble)
    }
}

/// Per-PSU info entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlPsuInfo {
    supported: u32,
    psu_type: u32,
    energy_counter: CtlTelemetryItem,
    voltage: CtlTelemetryItem,
}

/// Power telemetry struct returned by `ctlPowerTelemetryGet`.
/// Fields match IGCL igcl_api.h `ctl_power_telemetry_t` (version 1).
#[repr(C)]
struct CtlPowerTelemetry {
    size: u32,
    version: u8,
    // 3 bytes padding to align next field to 4-byte boundary
    _pad: [u8; 3],
    timestamp: CtlTelemetryItem,
    gpu_energy_counter: CtlTelemetryItem,
    gpu_voltage: CtlTelemetryItem,
    gpu_current_clock_frequency: CtlTelemetryItem,
    gpu_current_temperature: CtlTelemetryItem,
    global_activity_counter: CtlTelemetryItem,
    render_compute_activity_counter: CtlTelemetryItem,
    media_activity_counter: CtlTelemetryItem,
    gpu_power_limited: u32,
    gpu_temperature_limited: u32,
    gpu_current_limited: u32,
    gpu_voltage_limited: u32,
    gpu_utilization_limited: u32,
    vram_energy_counter: CtlTelemetryItem,
    vram_voltage: CtlTelemetryItem,
    vram_current_clock_frequency: CtlTelemetryItem,
    vram_current_effective_frequency: CtlTelemetryItem,
    vram_read_bandwidth_counter: CtlTelemetryItem,
    vram_write_bandwidth_counter: CtlTelemetryItem,
    vram_current_temperature: CtlTelemetryItem,
    vram_power_limited: u32,
    vram_temperature_limited: u32,
    vram_current_limited: u32,
    vram_voltage_limited: u32,
    vram_utilization_limited: u32,
    total_card_energy_counter: CtlTelemetryItem,
    psu: [CtlPsuInfo; CTL_PSU_COUNT],
    fan_speed: [CtlTelemetryItem; CTL_FAN_COUNT],
    gpu_vr_temp: CtlTelemetryItem,
    vram_vr_temp: CtlTelemetryItem,
    sa_vr_temp: CtlTelemetryItem,
    gpu_effective_clock: CtlTelemetryItem,
    gpu_over_voltage_percent: CtlTelemetryItem,
    gpu_power_percent: CtlTelemetryItem,
    gpu_temperature_percent: CtlTelemetryItem,
    vram_read_bandwidth: CtlTelemetryItem,
    vram_write_bandwidth: CtlTelemetryItem,
}

impl CtlPowerTelemetry {
    fn new() -> Self {
        // SAFETY: repr(C) struct with numeric and array fields. All-zeros is
        // valid; size and version are set after construction.
        let mut t: Self = unsafe { std::mem::zeroed() };
        t.size = std::mem::size_of::<Self>() as u32;
        t.version = 1;
        t
    }
}

// ---------------------------------------------------------------------------
// IGCL function pointer types
// ---------------------------------------------------------------------------

type CtlInitFn = unsafe extern "C" fn(*mut CtlInitArgs, *mut CtlApiHandle) -> u32;
type CtlCloseFn = unsafe extern "C" fn(CtlApiHandle) -> u32;
type CtlEnumDevicesFn = unsafe extern "C" fn(CtlApiHandle, *mut u32, *mut CtlDeviceHandle) -> u32;
type CtlGetDevicePropsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut CtlDeviceAdapterProperties) -> u32;
type CtlEnumTempSensorsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlTempHandle) -> u32;
type CtlTempGetStateFn = unsafe extern "C" fn(CtlTempHandle, *mut CtlTempState) -> u32;
type CtlEnumMemModulesFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlMemHandle) -> u32;
type CtlMemGetStateFn = unsafe extern "C" fn(CtlMemHandle, *mut CtlMemState) -> u32;
type CtlEnumEngineGroupsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlEngineHandle) -> u32;
type CtlEngineGetActivityFn = unsafe extern "C" fn(CtlEngineHandle, *mut CtlEngineActivity) -> u32;
type CtlEnumFreqDomainsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlFreqHandle) -> u32;
type CtlFreqGetStateFn = unsafe extern "C" fn(CtlFreqHandle, *mut CtlFreqState) -> u32;
type CtlEnumPowerDomainsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlPwrHandle) -> u32;
type CtlPowerGetPropsFn = unsafe extern "C" fn(CtlPwrHandle, *mut CtlPowerProperties) -> u32;
type CtlPowerTelemetryGetFn = unsafe extern "C" fn(CtlDeviceHandle, *mut CtlPowerTelemetry) -> u32;

struct IgclFunctions {
    _library: OwnedLibrary,
    init: CtlInitFn,
    close: CtlCloseFn,
    enum_devices: CtlEnumDevicesFn,
    get_device_props: CtlGetDevicePropsFn,
    enum_temp_sensors: CtlEnumTempSensorsFn,
    temp_get_state: CtlTempGetStateFn,
    enum_mem_modules: CtlEnumMemModulesFn,
    mem_get_state: CtlMemGetStateFn,
    enum_engine_groups: CtlEnumEngineGroupsFn,
    engine_get_activity: CtlEngineGetActivityFn,
    enum_freq_domains: CtlEnumFreqDomainsFn,
    freq_get_state: CtlFreqGetStateFn,
    enum_power_domains: CtlEnumPowerDomainsFn,
    power_get_props: CtlPowerGetPropsFn,
    power_telemetry_get: CtlPowerTelemetryGetFn,
}

impl IgclFunctions {
    fn load() -> Option<Self> {
        use windows::Win32::System::LibraryLoader::GetProcAddress;
        use windows::core::PCSTR;

        // Try ControlLib.dll first (Intel Arc Control), fall back to igcl.dll
        let library =
            Self::try_load_dll("ControlLib.dll\0").or_else(|| Self::try_load_dll("igcl.dll\0"))?;
        let handle = library.get();

        macro_rules! load_fn {
            ($name:literal, $ty:ty) => {{
                // SAFETY: handle is a loaded ControlLib.dll/igcl.dll module;
                // symbol name is a static null-terminated string.
                let proc = unsafe { GetProcAddress(handle, PCSTR(concat!($name, "\0").as_ptr())) }?;
                // SAFETY: GetProcAddress returned a non-null address matching
                // the documented IGCL function signature.
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc) }
            }};
        }

        Some(Self {
            _library: library,
            init: load_fn!("ctlInit", CtlInitFn),
            close: load_fn!("ctlClose", CtlCloseFn),
            enum_devices: load_fn!("ctlEnumerateDevices", CtlEnumDevicesFn),
            get_device_props: load_fn!("ctlGetDeviceProperties", CtlGetDevicePropsFn),
            enum_temp_sensors: load_fn!("ctlEnumTemperatureSensors", CtlEnumTempSensorsFn),
            temp_get_state: load_fn!("ctlTemperatureGetState", CtlTempGetStateFn),
            enum_mem_modules: load_fn!("ctlEnumMemoryModules", CtlEnumMemModulesFn),
            mem_get_state: load_fn!("ctlMemoryGetState", CtlMemGetStateFn),
            enum_engine_groups: load_fn!("ctlEnumEngineGroups", CtlEnumEngineGroupsFn),
            engine_get_activity: load_fn!("ctlEngineGetActivity", CtlEngineGetActivityFn),
            enum_freq_domains: load_fn!("ctlEnumFrequencyDomains", CtlEnumFreqDomainsFn),
            freq_get_state: load_fn!("ctlFrequencyGetState", CtlFreqGetStateFn),
            enum_power_domains: load_fn!("ctlEnumPowerDomains", CtlEnumPowerDomainsFn),
            power_get_props: load_fn!("ctlPowerGetProperties", CtlPowerGetPropsFn),
            power_telemetry_get: load_fn!("ctlPowerTelemetryGet", CtlPowerTelemetryGetFn),
        })
    }

    fn try_load_dll(name: &str) -> Option<OwnedLibrary> {
        use windows::Win32::System::LibraryLoader::LoadLibraryW;

        let dll_name: Vec<u16> = name.encode_utf16().collect();
        // SAFETY: dll_name is a valid null-terminated UTF-16 string built
        // from `name` (callers pass a string that already ends with `\0`).
        OwnedLibrary::new(unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())) }.ok()?)
    }
}

// ---------------------------------------------------------------------------
// Intel vendor session and per-device collector
// ---------------------------------------------------------------------------

/// `Send + Sync` newtype around `CtlApiHandle` (`*mut c_void`).
///
/// The IGCL `api_handle` is a process-singleton in Intel's official
/// integration pattern (their own wrapper at
/// `intel/drivers.gpu.control-library/Source/cApiWrapper.cpp` uses
/// a static `hinstLib` and calls `ctlInit` only when it's `NULL`).
/// Sharing one handle across per-device threads via `Arc` matches
/// that contract; the IGCL functions themselves take a per-device
/// handle as their leading argument and the `api_handle` is only
/// passed to `ctlEnumerateDevices` (called once during discovery,
/// never again from a worker thread) and to `ctlClose` (called once
/// from `Drop` on the last `Arc`).
#[repr(transparent)]
struct IgclApiHandleSafe(CtlApiHandle);

// SAFETY: see type-level doc — the handle is treated as a
// process-singleton by Intel's own integration pattern; rtop never
// dereferences it from Rust.
unsafe impl Send for IgclApiHandleSafe {}
// SAFETY: see Send impl above.
unsafe impl Sync for IgclApiHandleSafe {}

/// `Send` newtype around `CtlDeviceHandle` (`*mut c_void`).
/// Each per-device state owns its own device handle and never
/// shares it with another thread.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct IgclDevice(CtlDeviceHandle);
// SAFETY: opaque IGCL handle; only passed back to IGCL functions
// from a single owning thread.
unsafe impl Send for IgclDevice {}

/// `Send` newtype around `CtlTempHandle`. Per-device-owned;
/// passed back only to `ctlTemperatureGetState` and friends.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct IgclTempHandle(CtlTempHandle);
// SAFETY: opaque IGCL temperature-sensor handle; per-device-owned.
unsafe impl Send for IgclTempHandle {}

/// `Send` newtype around `CtlMemHandle`. Per-device-owned.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct IgclMemHandle(CtlMemHandle);
// SAFETY: opaque IGCL memory-module handle; per-device-owned.
unsafe impl Send for IgclMemHandle {}

/// `Send` newtype around `CtlEngineHandle`. Per-device-owned.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct IgclEngineHandle(CtlEngineHandle);
// SAFETY: opaque IGCL engine-group handle; per-device-owned.
unsafe impl Send for IgclEngineHandle {}

/// `Send` newtype around `CtlFreqHandle`. Per-device-owned.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct IgclFreqHandle(CtlFreqHandle);
// SAFETY: opaque IGCL frequency-domain handle; per-device-owned.
unsafe impl Send for IgclFreqHandle {}

/// IGCL vendor session: loaded library, resolved function-pointer
/// table, and the `api_handle` from `ctlInit`. Owned once by
/// [`super::GpuCollector`] when at least one Intel device is
/// detected; constructed by [`discover`] and dropped when the
/// collector drops. The `Drop` impl calls `ctlClose` exactly once
/// per successful `ctlInit`.
pub(super) struct IgclSession {
    igcl: IgclFunctions,
    api_handle: IgclApiHandleSafe,
}

impl IgclSession {
    fn load() -> Option<Self> {
        let igcl = IgclFunctions::load()?;

        let mut init_args = CtlInitArgs {
            size: std::mem::size_of::<CtlInitArgs>() as u32,
            version: CTL_IMPL_VERSION,
            flags: CTL_INIT_FLAG_USE_LEVEL_ZERO,
            reserved: 0,
            app_uid: [0; 16],
            app_version: 0,
            p_reserved: std::ptr::null_mut(),
        };
        let mut api_handle: CtlApiHandle = std::ptr::null_mut();

        // SAFETY: init_args is a valid repr(C) struct with size
        // and version set; api_handle is a valid pointer to a
        // stack-allocated null pointer.
        let ret = unsafe { (igcl.init)(&mut init_args, &mut api_handle) };
        if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                "ctlInit failed",
            );
            return None;
        }

        Some(Self {
            igcl,
            api_handle: IgclApiHandleSafe(api_handle),
        })
    }
}

impl Drop for IgclSession {
    fn drop(&mut self) {
        if !self.api_handle.0.is_null() {
            // SAFETY: api_handle was created by ctlInit on the same
            // igcl function table; close was resolved from the same
            // library and is called exactly once per successful init.
            unsafe {
                let _ = (self.igcl.close)(self.api_handle.0);
            }
            self.api_handle.0 = std::ptr::null_mut();
        }
    }
}

/// Slim per-device Intel state. One per detected device; held
/// inside [`super::GpuCollector`]'s `Vec<DeviceEntry>`. Carries the
/// per-device IGCL handle, the cached telemetry sub-handles
/// (temperature/memory/engine/frequency) populated during
/// discovery, and the four prev-counters needed for delta-based
/// power and engine-utilization derivation.
pub(super) struct DeviceState {
    pub(super) handle: IgclDevice,
    pub(super) temp_sensor: Option<IgclTempHandle>,
    pub(super) mem_module: Option<IgclMemHandle>,
    pub(super) engine_group: Option<IgclEngineHandle>,
    pub(super) freq_domain: Option<IgclFreqHandle>,
    pub(super) prev_active: u64,
    pub(super) prev_timestamp: u64,
    pub(super) prev_energy: f64,
    pub(super) prev_energy_ts: f64,
}

/// One collection cycle for a single Intel device.
///
/// Reads vendor function pointers from `session`, the per-device
/// handles and prev-counters from `dev`, mutates the rendered
/// `info` in place, and downgrades `status` on partial failures.
/// Updates the prev-counters on `dev` for the next cycle's delta
/// calculations.
pub(super) fn collect(
    session: &IgclSession,
    dev: &mut DeviceState,
    info: &mut crate::domain::gpu::GpuInfo,
    status: &mut CollectStatus,
) {
    let igcl = &session.igcl;

    // Temperature
    if let Some(temp_h) = dev.temp_sensor {
        let mut state = CtlTempState::default();
        // SAFETY: temp_h cached from discovery; state is valid.
        let ret = unsafe { (igcl.temp_get_state)(temp_h.0, &mut state) };
        if ret == CTL_RESULT_SUCCESS {
            push_history(&mut info.temp, state.temperature as i64);
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                "IGCL temperature query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl temperature"));
        }
    }

    // Memory
    if let Some(mem_h) = dev.mem_module {
        let mut state = CtlMemState::default();
        // SAFETY: mem_h cached from discovery; state is valid.
        let ret = unsafe { (igcl.mem_get_state)(mem_h.0, &mut state) };
        if ret == CTL_RESULT_SUCCESS && state.size > 0 {
            info.mem_total = state.size;
            info.mem_used = state.size.saturating_sub(state.free);
            let vram_pct =
                crate::collect::counters::percent_u64(info.mem_used, state.size).min(100);
            push_history(&mut info.gpu_percent.vram, vram_pct);
            push_history(&mut info.mem_utilization_percent, vram_pct);
        } else if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                "IGCL memory query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl memory"));
        }
    }

    // Engine utilization (compute delta from active/timestamp pairs)
    if let Some(eng_h) = dev.engine_group {
        let mut activity = CtlEngineActivity::default();
        // SAFETY: eng_h cached from discovery; activity is valid.
        let ret = unsafe { (igcl.engine_get_activity)(eng_h.0, &mut activity) };
        if ret == CTL_RESULT_SUCCESS && activity.timestamp > 0 {
            if dev.prev_timestamp > 0 {
                let dt = activity.timestamp.saturating_sub(dev.prev_timestamp);
                let da = activity.active_time.saturating_sub(dev.prev_active);
                let pct = (da * 100).checked_div(dt).unwrap_or(0) as u32;
                push_history(&mut info.gpu_percent.utilization, clamp_percent(pct));
            }
            dev.prev_active = activity.active_time;
            dev.prev_timestamp = activity.timestamp;
        } else if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                "IGCL engine activity query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl engine"));
        }
    }

    // Frequency
    if let Some(freq_h) = dev.freq_domain {
        let mut state = CtlFreqState::default();
        // SAFETY: freq_h cached from discovery; state is valid.
        let ret = unsafe { (igcl.freq_get_state)(freq_h.0, &mut state) };
        if ret == CTL_RESULT_SUCCESS && state.actual_frequency > 0.0 {
            info.gpu_clock_speed = state.actual_frequency as u32;
        } else if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                "IGCL frequency query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl frequency"));
        }
    }

    // Power — derived from energy-counter differentiation (ΔJ / Δs).
    let mut telemetry = CtlPowerTelemetry::new();
    // SAFETY: dev.handle.0 is a valid device handle; telemetry is
    // a valid versioned struct with size and version set.
    let ret = unsafe { (igcl.power_telemetry_get)(dev.handle.0, &mut telemetry) };
    if ret == CTL_RESULT_SUCCESS {
        let energy = telemetry
            .total_card_energy_counter
            .get()
            .or_else(|| telemetry.gpu_energy_counter.get());
        let timestamp = telemetry.timestamp.get();

        if let (Some(energy_j), Some(ts_s)) = (energy, timestamp) {
            let dt = ts_s - dev.prev_energy_ts;
            let de = energy_j - dev.prev_energy;
            if dt > 0.0 && de >= 0.0 && dev.prev_energy_ts > 0.0 {
                let watts = de / dt;
                let power_mw = (watts * 1000.0) as u64;
                info.pwr_usage = power_mw as i64;
                let pwr_pct = super::power_percent(power_mw, info.pwr_max_usage as u64);
                push_history(&mut info.gpu_percent.power, pwr_pct);
            }
            dev.prev_energy = energy_j;
            dev.prev_energy_ts = ts_s;
        }
    } else {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuIgcl,
            code = %crate::log::Hex(ret),
            "IGCL power telemetry query failed",
        );
        status.downgrade(CollectStatus::Degraded("igcl power"));
    }
}

/// Discovery result for the Intel vendor.
///
/// Returned by [`discover`] when at least one device was detected;
/// owned by [`super::GpuCollector`] which holds the session for the
/// lifetime of every device entry derived from this bundle.
pub(super) struct IgclBundle {
    pub(super) session: IgclSession,
    pub(super) entries: Vec<(DeviceState, crate::domain::gpu::GpuInfo)>,
}

/// Discover every Intel GPU. Returns `None` when IGCL is
/// unavailable or no devices are detected (vendor session drops on
/// the spot — no library / api_handle resources outlive an empty
/// discovery).
pub(super) fn discover() -> Option<IgclBundle> {
    let session = IgclSession::load()?;

    // Enumerate devices (two-call pattern).
    let mut count: u32 = 0;
    // SAFETY: api_handle from ctlInit; count is valid pointer; passing
    // a null device pointer with a valid count is the documented
    // IGCL "query count" call shape.
    let ret = unsafe {
        (session.igcl.enum_devices)(session.api_handle.0, &mut count, std::ptr::null_mut())
    };
    if ret != CTL_RESULT_SUCCESS || count == 0 {
        return None;
    }

    let mut device_handles: Vec<CtlDeviceHandle> = vec![std::ptr::null_mut(); count as usize];
    // SAFETY: api_handle valid; device_handles is correctly sized for the
    // count returned by the previous call.
    let ret = unsafe {
        (session.igcl.enum_devices)(
            session.api_handle.0,
            &mut count,
            device_handles.as_mut_ptr(),
        )
    };
    if ret != CTL_RESULT_SUCCESS {
        return None;
    }
    device_handles.truncate(count as usize);

    let mut entries: Vec<(DeviceState, crate::domain::gpu::GpuInfo)> = Vec::new();
    for (vendor_relative_index, dev) in device_handles.into_iter().enumerate() {
        // SAFETY: dev is a valid handle; props is zeroed repr(C) struct.
        let mut props: CtlDeviceAdapterProperties = unsafe { std::mem::zeroed() };
        props.size = std::mem::size_of::<CtlDeviceAdapterProperties>() as u32;
        // SAFETY: dev is valid; props is a valid versioned struct.
        let ret = unsafe { (session.igcl.get_device_props)(dev, &mut props) };
        if ret != CTL_RESULT_SUCCESS {
            continue;
        }

        let name = name_from_buf(&props.name);
        if name.is_empty() {
            continue;
        }

        // Cache first sub-handle of each telemetry type
        let temp_sensors = unsafe { enum_handles(dev, session.igcl.enum_temp_sensors) };
        let mem_modules = unsafe { enum_handles(dev, session.igcl.enum_mem_modules) };
        let engine_groups = unsafe { enum_handles(dev, session.igcl.enum_engine_groups) };
        let freq_domains = unsafe { enum_handles(dev, session.igcl.enum_freq_domains) };

        // Query max power limit
        let power_domains = unsafe { enum_handles(dev, session.igcl.enum_power_domains) };
        let mut max_power_mw: i64 = 0;
        if let Some(&pwr) = power_domains.first() {
            let mut power_props = CtlPowerProperties::default();
            // SAFETY: pwr is a valid power handle.
            let ret = unsafe { (session.igcl.power_get_props)(pwr, &mut power_props) };
            if ret == CTL_RESULT_SUCCESS && power_props.max_limit > 0 {
                max_power_mw = power_props.max_limit as i64;
            }
        }

        // Query initial memory total
        let mut mem_total: u64 = 0;
        if let Some(&mem_h) = mem_modules.first() {
            let mut mem_state = CtlMemState::default();
            // SAFETY: mem_h is a valid memory handle.
            let ret = unsafe { (session.igcl.mem_get_state)(mem_h, &mut mem_state) };
            if ret == CTL_RESULT_SUCCESS {
                mem_total = mem_state.size;
            }
        }

        // Query max clock from frequency domain
        let mut max_clock: u32 = 0;
        if let Some(&freq_h) = freq_domains.first() {
            let mut freq_state = CtlFreqState::default();
            // SAFETY: freq_h is a valid frequency handle.
            let ret = unsafe { (session.igcl.freq_get_state)(freq_h, &mut freq_state) };
            if ret == CTL_RESULT_SUCCESS && freq_state.tdp_frequency > 0.0 {
                max_clock = freq_state.tdp_frequency as u32;
            }
        }

        // IGCL does not expose a per-card UUID at this struct
        // level (`pci_device_id` is shared across every Arc A770
        // ever shipped). The vendor-relative discovery index is
        // stable for any configuration that doesn't physically
        // rearrange devices.
        let stable_id = format!("INTEL:adapter:{vendor_relative_index}");

        let info = crate::domain::gpu::GpuInfo {
            stable_id,
            name,
            mem_total,
            pwr_max_usage: max_power_mw,
            gpu_max_clock_speed: max_clock,
            ..crate::domain::gpu::GpuInfo::default()
        };

        entries.push((
            DeviceState {
                handle: IgclDevice(dev),
                temp_sensor: temp_sensors.into_iter().next().map(IgclTempHandle),
                mem_module: mem_modules.into_iter().next().map(IgclMemHandle),
                engine_group: engine_groups.into_iter().next().map(IgclEngineHandle),
                freq_domain: freq_domains.into_iter().next().map(IgclFreqHandle),
                prev_active: 0,
                prev_timestamp: 0,
                prev_energy: 0.0,
                prev_energy_ts: 0.0,
            },
            info,
        ));
    }

    if entries.is_empty() {
        return None;
    }

    tracing::info!(
        subsystem = %crate::log::Subsystem::GpuIgcl,
        devices = entries.len(),
        "vendor initialized",
    );

    Some(IgclBundle { session, entries })
}

/// Extract a name string from a fixed-size byte buffer.
fn name_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

/// Enumerate sub-handles using the IGCL two-call pattern (get count, then fill).
unsafe fn enum_handles<H: Copy>(
    device: CtlDeviceHandle,
    enum_fn: unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut H) -> u32,
) -> Vec<H> {
    let mut count: u32 = 0;
    // SAFETY: device is a valid IGCL device handle (caller's responsibility
    // per the `unsafe fn` contract); a null target pointer with a valid
    // &mut u32 count is the documented IGCL "query count" call shape.
    let ret = unsafe { enum_fn(device, &mut count, std::ptr::null_mut()) };
    if ret != CTL_RESULT_SUCCESS || count == 0 {
        return Vec::new();
    }
    // SAFETY: MaybeUninit<H> where H is a pointer type — zeroed is valid for pointers.
    let mut handles: Vec<H> = vec![unsafe { std::mem::zeroed() }; count as usize];
    // SAFETY: device is a valid IGCL device handle; handles is sized to
    // `count` (the value the previous call returned), so the IGCL fill
    // pass writes within the allocated buffer.
    let ret = unsafe { enum_fn(device, &mut count, handles.as_mut_ptr()) };
    if ret != CTL_RESULT_SUCCESS {
        return Vec::new();
    }
    handles.truncate(count as usize);
    handles
}
