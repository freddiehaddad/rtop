use crate::collect::CollectStatus;
use crate::collect::win::OwnedLibrary;
use std::ffi::c_void;

use super::{clamp_percent, push_history};

// ---------------------------------------------------------------------------
// IGCL constants (mirror Intel `igcl_api.h`)
// ---------------------------------------------------------------------------

const CTL_RESULT_SUCCESS: u32 = 0;
const CTL_IMPL_MAJOR_VERSION: u32 = 1;
const CTL_IMPL_MINOR_VERSION: u32 = 1;
const CTL_IMPL_VERSION: u32 = (CTL_IMPL_MAJOR_VERSION << 16) | (CTL_IMPL_MINOR_VERSION & 0xFFFF);
const CTL_INIT_FLAG_USE_LEVEL_ZERO: u32 = 1;
const CTL_MAX_DEVICE_NAME_LEN: usize = 100;
const CTL_MAX_RESERVED_SIZE: usize = 108;
const CTL_PSU_COUNT: usize = 5;
const CTL_FAN_COUNT: usize = 5;

// Power telemetry version 1 unlocks the extended fields (totalCardEnergyCounter
// and the per-VR temperature/effective-clock/percent items). Intel's own
// Sample_TelemetryAPP.cpp uses this version. Every other Ctl* struct in IGCL
// uses Version 0.
const CTL_POWER_TELEMETRY_VERSION: u8 = 1;
const CTL_STRUCT_VERSION_0: u8 = 0;

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
// IGCL structures (repr(C) matching `igcl_api.h` on MSVC x64).
//
// Field order, types, and padding must match the C ABI exactly so that the
// `Size = sizeof(Self)` IGCL versioning contract is satisfied. The
// `#[cfg(test)] abi_tests` module at the bottom of this file pins every
// struct size to the MSVC ABI value verified against the canonical header.
// ---------------------------------------------------------------------------

/// `ctl_application_id_t` — 16 bytes, alignment 4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlApplicationId {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// `ctl_init_args_t` — 36 bytes, alignment 4. Passed in/out to `ctlInit`.
#[repr(C)]
struct CtlInitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: CtlApplicationId,
}

impl CtlInitArgs {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            version: CTL_STRUCT_VERSION_0,
            app_version: CTL_IMPL_VERSION,
            flags: CTL_INIT_FLAG_USE_LEVEL_ZERO,
            supported_version: 0,
            application_uid: CtlApplicationId::default(),
        }
    }
}

/// `ctl_firmware_version_t` — 24 bytes, alignment 8. Embedded in
/// `ctl_device_adapter_properties_t`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlFirmwareVersion {
    major_version: u64,
    minor_version: u64,
    build_number: u64,
}

/// `ctl_adapter_bdf_t` — 3 bytes, alignment 1. Embedded in
/// `ctl_device_adapter_properties_t`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlAdapterBdf {
    bus: u8,
    device: u8,
    function: u8,
}

/// `ctl_device_adapter_properties_t` — 320 bytes, alignment 8.
///
/// `Version = 0` accesses every field through `num_xe_cores` (Intel's spec
/// note "Supported only for Version > N" refers to driver-side data
/// availability, not struct layout). Setting `Version` higher than 0 does
/// not change the layout; it only signals which read-out fields the caller
/// is prepared to interpret.
#[repr(C)]
struct CtlDeviceAdapterProperties {
    size: u32,
    version: u8,
    p_device_id: *mut c_void,
    device_id_size: u32,
    device_type: u32,
    supported_subfunction_flags: u32,
    driver_version: u64,
    firmware_version: CtlFirmwareVersion,
    pci_vendor_id: u32,
    pci_device_id: u32,
    rev_id: u32,
    num_eus_per_sub_slice: u32,
    num_sub_slices_per_slice: u32,
    num_slices: u32,
    name: [u8; CTL_MAX_DEVICE_NAME_LEN],
    graphics_adapter_properties: u32,
    frequency: u32,
    pci_subsys_id: u16,
    pci_subsys_vendor_id: u16,
    adapter_bdf: CtlAdapterBdf,
    num_xe_cores: u32,
    reserved: [u8; CTL_MAX_RESERVED_SIZE],
}

impl CtlDeviceAdapterProperties {
    fn new() -> Self {
        // SAFETY: every field is integer, pointer, or fixed-size POD array;
        // zero is a valid bit pattern for all of them. `size` and `version`
        // are set immediately after construction below.
        let mut p: Self = unsafe { std::mem::zeroed() };
        p.size = std::mem::size_of::<Self>() as u32;
        p.version = CTL_STRUCT_VERSION_0;
        p
    }
}

/// `ctl_mem_state_t` — 24 bytes, alignment 8.
#[repr(C)]
struct CtlMemState {
    size: u32,
    version: u8,
    free: u64,
    total: u64,
}

impl CtlMemState {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            version: CTL_STRUCT_VERSION_0,
            free: 0,
            total: 0,
        }
    }
}

/// `ctl_engine_stats_t` — 24 bytes, alignment 8.
#[repr(C)]
struct CtlEngineStats {
    size: u32,
    version: u8,
    active_time: u64,
    timestamp: u64,
}

impl CtlEngineStats {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            version: CTL_STRUCT_VERSION_0,
            active_time: 0,
            timestamp: 0,
        }
    }
}

/// `ctl_freq_state_t` — 56 bytes, alignment 8.
///
/// `tdp` is the maximum frequency supported under current TDP conditions
/// (used during discovery to populate `gpu_max_clock_speed`). `actual` is
/// the resolved instantaneous frequency (sampled per cycle).
#[repr(C)]
struct CtlFreqState {
    size: u32,
    version: u8,
    current_voltage: f64,
    request: f64,
    tdp: f64,
    efficient: f64,
    actual: f64,
    throttle_reasons: u32,
}

impl CtlFreqState {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            version: CTL_STRUCT_VERSION_0,
            current_voltage: 0.0,
            request: 0.0,
            tdp: 0.0,
            efficient: 0.0,
            actual: 0.0,
            throttle_reasons: 0,
        }
    }
}

/// `ctl_power_properties_t` — 20 bytes, alignment 4.
///
/// Limits are reported in milliwatts; a value of `-1` per Intel's spec
/// signals that the limit is not known.
#[repr(C)]
struct CtlPowerProperties {
    size: u32,
    version: u8,
    can_control: u8,
    default_limit: i32,
    min_limit: i32,
    max_limit: i32,
}

impl CtlPowerProperties {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            version: CTL_STRUCT_VERSION_0,
            can_control: 0,
            default_limit: 0,
            min_limit: 0,
            max_limit: 0,
        }
    }
}

/// Telemetry value union (`ctl_data_value_t`) — 8 bytes, alignment 8.
///
/// The widest member of the C union is `double`/`uint64_t`. rtop only
/// consumes the `double` projection (the only type any of the telemetry
/// items in `ctl_power_telemetry_t` are documented to use).
#[repr(C)]
#[derive(Clone, Copy)]
union CtlDataValue {
    datadouble: f64,
    datau64: u64,
}

impl Default for CtlDataValue {
    fn default() -> Self {
        Self { datau64: 0 }
    }
}

/// `ctl_oc_telemetry_item_t` — 24 bytes, alignment 8.
///
/// `b_supported` is a C `bool` (1 byte). When false, the entire item is
/// stale and must be ignored per Intel's spec.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlTelemetryItem {
    b_supported: u8,
    units: u32,
    data_type: u32,
    value: CtlDataValue,
}

impl CtlTelemetryItem {
    fn get(&self) -> Option<f64> {
        // SAFETY: when `b_supported` is non-zero, Intel guarantees the
        // union holds a value tagged by `data_type` / `units`. rtop only
        // calls `get()` on items documented to use the `double` projection
        // (timeStamp, gpuEnergyCounter, totalCardEnergyCounter).
        (self.b_supported != 0).then_some(unsafe { self.value.datadouble })
    }
}

/// `ctl_psu_info_t` — 56 bytes, alignment 8.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlPsuInfo {
    b_supported: u8,
    psu_type: u32,
    energy_counter: CtlTelemetryItem,
    voltage: CtlTelemetryItem,
}

/// `ctl_power_telemetry_t` — 1024 bytes, alignment 8.
///
/// rtop requests `Version = 1` to opt into the extended fields (anything
/// past `mediaActivityCounter`), matching Intel's `Sample_TelemetryAPP.cpp`.
/// rtop currently reads only `time_stamp`, `gpu_energy_counter`, and
/// `total_card_energy_counter`; the remaining fields exist to make the
/// struct layout match the C ABI exactly.
#[repr(C)]
struct CtlPowerTelemetry {
    size: u32,
    version: u8,
    time_stamp: CtlTelemetryItem,
    gpu_energy_counter: CtlTelemetryItem,
    gpu_voltage: CtlTelemetryItem,
    gpu_current_clock_frequency: CtlTelemetryItem,
    gpu_current_temperature: CtlTelemetryItem,
    global_activity_counter: CtlTelemetryItem,
    render_compute_activity_counter: CtlTelemetryItem,
    media_activity_counter: CtlTelemetryItem,
    gpu_power_limited: u8,
    gpu_temperature_limited: u8,
    gpu_current_limited: u8,
    gpu_voltage_limited: u8,
    gpu_utilization_limited: u8,
    vram_energy_counter: CtlTelemetryItem,
    vram_voltage: CtlTelemetryItem,
    vram_current_clock_frequency: CtlTelemetryItem,
    vram_current_effective_frequency: CtlTelemetryItem,
    vram_read_bandwidth_counter: CtlTelemetryItem,
    vram_write_bandwidth_counter: CtlTelemetryItem,
    vram_current_temperature: CtlTelemetryItem,
    vram_power_limited: u8,
    vram_temperature_limited: u8,
    vram_current_limited: u8,
    vram_voltage_limited: u8,
    vram_utilization_limited: u8,
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
        // SAFETY: every field is a `u8`, an integer array, or a repr(C)
        // struct of integers/unions whose zero bit pattern is valid. `size`
        // and `version` are set below before the struct is handed to IGCL.
        let mut t: Self = unsafe { std::mem::zeroed() };
        t.size = std::mem::size_of::<Self>() as u32;
        t.version = CTL_POWER_TELEMETRY_VERSION;
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
type CtlTempGetStateFn = unsafe extern "C" fn(CtlTempHandle, *mut f64) -> u32;
type CtlEnumMemModulesFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlMemHandle) -> u32;
type CtlMemGetStateFn = unsafe extern "C" fn(CtlMemHandle, *mut CtlMemState) -> u32;
type CtlEnumEngineGroupsFn =
    unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut CtlEngineHandle) -> u32;
type CtlEngineGetActivityFn = unsafe extern "C" fn(CtlEngineHandle, *mut CtlEngineStats) -> u32;
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
// IGCL error code -> symbolic name (for diagnostic log readability)
// ---------------------------------------------------------------------------

/// Map a `ctl_result_t` value to the symbolic name from `igcl_api.h`.
///
/// `None` means the code is outside the documented range. Used purely for
/// log readability so field reports like `code=0x4000000F` are paired with
/// `name=CTL_RESULT_ERROR_INVALID_SIZE` instead of forcing the reader to
/// cross-reference the header by hand.
fn ctl_result_name(code: u32) -> Option<&'static str> {
    let s = match code {
        0x00000000 => "CTL_RESULT_SUCCESS",
        0x00000001 => "CTL_RESULT_SUCCESS_STILL_OPEN_BY_ANOTHER_CALLER",
        0x0000FFFF => "CTL_RESULT_ERROR_SUCCESS_END",
        0x40000000 => "CTL_RESULT_ERROR_GENERIC_START",
        0x40000001 => "CTL_RESULT_ERROR_NOT_INITIALIZED",
        0x40000002 => "CTL_RESULT_ERROR_ALREADY_INITIALIZED",
        0x40000003 => "CTL_RESULT_ERROR_DEVICE_LOST",
        0x40000004 => "CTL_RESULT_ERROR_OUT_OF_HOST_MEMORY",
        0x40000005 => "CTL_RESULT_ERROR_OUT_OF_DEVICE_MEMORY",
        0x40000006 => "CTL_RESULT_ERROR_INSUFFICIENT_PERMISSIONS",
        0x40000007 => "CTL_RESULT_ERROR_NOT_AVAILABLE",
        0x40000008 => "CTL_RESULT_ERROR_UNINITIALIZED",
        0x40000009 => "CTL_RESULT_ERROR_UNSUPPORTED_VERSION",
        0x4000000A => "CTL_RESULT_ERROR_UNSUPPORTED_FEATURE",
        0x4000000B => "CTL_RESULT_ERROR_INVALID_ARGUMENT",
        0x4000000C => "CTL_RESULT_ERROR_INVALID_API_HANDLE",
        0x4000000D => "CTL_RESULT_ERROR_INVALID_NULL_HANDLE",
        0x4000000E => "CTL_RESULT_ERROR_INVALID_NULL_POINTER",
        0x4000000F => "CTL_RESULT_ERROR_INVALID_SIZE",
        0x40000010 => "CTL_RESULT_ERROR_UNSUPPORTED_SIZE",
        0x40000011 => "CTL_RESULT_ERROR_UNSUPPORTED_IMAGE_FORMAT",
        0x40000012 => "CTL_RESULT_ERROR_DATA_READ",
        0x40000013 => "CTL_RESULT_ERROR_DATA_WRITE",
        0x40000014 => "CTL_RESULT_ERROR_DATA_NOT_FOUND",
        0x40000015 => "CTL_RESULT_ERROR_NOT_IMPLEMENTED",
        0x40000016 => "CTL_RESULT_ERROR_OS_CALL",
        0x40000017 => "CTL_RESULT_ERROR_KMD_CALL",
        0x40000018 => "CTL_RESULT_ERROR_UNLOAD",
        0x40000019 => "CTL_RESULT_ERROR_ZE_LOADER",
        0x4000001A => "CTL_RESULT_ERROR_INVALID_OPERATION_TYPE",
        0x4000001B => "CTL_RESULT_ERROR_NULL_OS_INTERFACE",
        0x4000001C => "CTL_RESULT_ERROR_NULL_OS_ADAPATER_HANDLE",
        0x4000001D => "CTL_RESULT_ERROR_NULL_OS_DISPLAY_OUTPUT_HANDLE",
        0x4000001E => "CTL_RESULT_ERROR_WAIT_TIMEOUT",
        0x4000001F => "CTL_RESULT_ERROR_PERSISTANCE_NOT_SUPPORTED",
        0x40000020 => "CTL_RESULT_ERROR_PLATFORM_NOT_SUPPORTED",
        0x40000021 => "CTL_RESULT_ERROR_UNKNOWN_APPLICATION_UID",
        0x40000022 => "CTL_RESULT_ERROR_INVALID_ENUMERATION",
        0x40000023 => "CTL_RESULT_ERROR_FILE_DELETE",
        0x40000024 => "CTL_RESULT_ERROR_RESET_DEVICE_REQUIRED",
        0x40000025 => "CTL_RESULT_ERROR_FULL_REBOOT_REQUIRED",
        0x40000026 => "CTL_RESULT_ERROR_LOAD",
        0x40000027 => "CTL_RESULT_ERROR_DEVICE_UNAVAILABLE",
        // Note: CTL_RESULT_ERROR_UNKNOWN and CTL_RESULT_ERROR_GENERIC_END
        // share the same value 0x4000FFFF in the header.
        0x4000FFFF => "CTL_RESULT_ERROR_UNKNOWN",
        0x40010000 => "CTL_RESULT_ERROR_RETRY_OPERATION",
        0x40010001 => "CTL_RESULT_ERROR_IGSC_LOADER",
        0x40010002 => "CTL_RESULT_ERROR_RESTRICTED_APPLICATION",
        0x44000000 => "CTL_RESULT_ERROR_CORE_START",
        0x44000001 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_NOT_SUPPORTED",
        0x44000002 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_VOLTAGE_OUTSIDE_RANGE",
        0x44000003 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_FREQUENCY_OUTSIDE_RANGE",
        0x44000004 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_POWER_OUTSIDE_RANGE",
        0x44000005 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_TEMPERATURE_OUTSIDE_RANGE",
        0x44000006 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_IN_VOLTAGE_LOCKED_MODE",
        0x44000007 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_RESET_REQUIRED",
        0x44000008 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_WAIVER_NOT_SET",
        0x44000009 => "CTL_RESULT_ERROR_CORE_OVERCLOCK_DEPRECATED_API",
        0x4400000A => "CTL_RESULT_ERROR_CORE_LED_GET_STATE_NOT_SUPPORTED_FOR_I2C_LED",
        0x4400000B => "CTL_RESULT_ERROR_CORE_LED_SET_STATE_NOT_SUPPORTED_FOR_I2C_LED",
        0x4400000C => "CTL_RESULT_ERROR_CORE_LED_TOO_FREQUENT_SET_REQUESTS",
        0x4400000D => "CTL_RESULT_ERROR_CORE_OVERCLOCK_VRAM_MEMORY_SPEED_OUTSIDE_RANGE",
        0x4400000E => "CTL_RESULT_ERROR_CORE_OVERCLOCK_INVALID_CUSTOM_VF_CURVE",
        0x0440FFFF => "CTL_RESULT_ERROR_CORE_END",
        0x48000000 => "CTL_RESULT_ERROR_DISPLAY_START",
        0x48000001 => "CTL_RESULT_ERROR_INVALID_AUX_ACCESS_FLAG",
        0x48000002 => "CTL_RESULT_ERROR_INVALID_SHARPNESS_FILTER_FLAG",
        0x48000003 => "CTL_RESULT_ERROR_DISPLAY_NOT_ATTACHED",
        0x48000004 => "CTL_RESULT_ERROR_DISPLAY_NOT_ACTIVE",
        0x48000005 => "CTL_RESULT_ERROR_INVALID_POWERFEATURE_OPTIMIZATION_FLAG",
        0x48000006 => "CTL_RESULT_ERROR_INVALID_POWERSOURCE_TYPE_FOR_DPST",
        0x48000007 => "CTL_RESULT_ERROR_INVALID_PIXTX_GET_CONFIG_QUERY_TYPE",
        0x48000008 => "CTL_RESULT_ERROR_INVALID_PIXTX_SET_CONFIG_OPERATION_TYPE",
        0x48000009 => "CTL_RESULT_ERROR_INVALID_SET_CONFIG_NUMBER_OF_SAMPLES",
        0x4800000A => "CTL_RESULT_ERROR_INVALID_PIXTX_BLOCK_ID",
        0x4800000B => "CTL_RESULT_ERROR_INVALID_PIXTX_BLOCK_TYPE",
        0x4800000C => "CTL_RESULT_ERROR_INVALID_PIXTX_BLOCK_NUMBER",
        0x4800000D => "CTL_RESULT_ERROR_INSUFFICIENT_PIXTX_BLOCK_CONFIG_MEMORY",
        0x4800000E => "CTL_RESULT_ERROR_3DLUT_INVALID_PIPE",
        0x4800000F => "CTL_RESULT_ERROR_3DLUT_INVALID_DATA",
        0x48000010 => "CTL_RESULT_ERROR_3DLUT_NOT_SUPPORTED_IN_HDR",
        0x48000011 => "CTL_RESULT_ERROR_3DLUT_INVALID_OPERATION",
        0x48000012 => "CTL_RESULT_ERROR_3DLUT_UNSUCCESSFUL",
        0x48000013 => "CTL_RESULT_ERROR_AUX_DEFER",
        0x48000014 => "CTL_RESULT_ERROR_AUX_TIMEOUT",
        0x48000015 => "CTL_RESULT_ERROR_AUX_INCOMPLETE_WRITE",
        0x48000016 => "CTL_RESULT_ERROR_I2C_AUX_STATUS_UNKNOWN",
        0x48000017 => "CTL_RESULT_ERROR_I2C_AUX_UNSUCCESSFUL",
        0x48000018 => "CTL_RESULT_ERROR_LACE_INVALID_DATA_ARGUMENT_PASSED",
        0x48000019 => "CTL_RESULT_ERROR_EXTERNAL_DISPLAY_ATTACHED",
        0x4800001A => "CTL_RESULT_ERROR_CUSTOM_MODE_STANDARD_CUSTOM_MODE_EXISTS",
        0x4800001B => "CTL_RESULT_ERROR_CUSTOM_MODE_NON_CUSTOM_MATCHING_MODE_EXISTS",
        0x4800001C => "CTL_RESULT_ERROR_CUSTOM_MODE_INSUFFICIENT_MEMORY",
        0x4800001D => "CTL_RESULT_ERROR_ADAPTER_ALREADY_LINKED",
        0x4800001E => "CTL_RESULT_ERROR_ADAPTER_NOT_IDENTICAL",
        0x4800001F => "CTL_RESULT_ERROR_ADAPTER_NOT_SUPPORTED_ON_LDA_SECONDARY",
        0x48000020 => "CTL_RESULT_ERROR_SET_FBC_FEATURE_NOT_SUPPORTED",
        0x4800FFFF => "CTL_RESULT_ERROR_DISPLAY_END",
        0x60000000 => "CTL_RESULT_ERROR_3D_START",
        0x6000FFFF => "CTL_RESULT_ERROR_3D_END",
        0x50000000 => "CTL_RESULT_ERROR_MEDIA_START",
        0x5000FFFF => "CTL_RESULT_ERROR_MEDIA_END",
        _ => return None,
    };
    Some(s)
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

        let mut init_args = CtlInitArgs::new();
        let mut api_handle: CtlApiHandle = std::ptr::null_mut();

        // SAFETY: init_args is a valid IGCL-versioned struct (size and
        // version set by `CtlInitArgs::new`); api_handle is a valid pointer
        // to a stack-allocated null pointer that IGCL writes through.
        let ret = unsafe { (igcl.init)(&mut init_args, &mut api_handle) };
        if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
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

    // Temperature — `ctlTemperatureGetState` writes a raw `double`,
    // not a versioned struct (Intel's ABI for this call is unusual).
    if let Some(temp_h) = dev.temp_sensor {
        let mut celsius: f64 = 0.0;
        // SAFETY: temp_h cached from discovery; celsius is a valid
        // mutable double on the stack.
        let ret = unsafe { (igcl.temp_get_state)(temp_h.0, &mut celsius) };
        if ret == CTL_RESULT_SUCCESS {
            push_history(&mut info.temp, celsius as i64);
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                "IGCL temperature query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl temperature"));
        }
    }

    // Memory
    if let Some(mem_h) = dev.mem_module {
        let mut state = CtlMemState::new();
        // SAFETY: mem_h cached from discovery; state is a valid
        // IGCL-versioned struct.
        let ret = unsafe { (igcl.mem_get_state)(mem_h.0, &mut state) };
        if ret == CTL_RESULT_SUCCESS && state.total > 0 {
            info.mem_total = state.total;
            info.mem_used = state.total.saturating_sub(state.free);
            let vram_pct =
                crate::collect::counters::percent_u64(info.mem_used, state.total).min(100);
            push_history(&mut info.gpu_percent.vram, vram_pct);
            push_history(&mut info.mem_utilization_percent, vram_pct);
        } else if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                "IGCL memory query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl memory"));
        }
    }

    // Engine utilization (compute delta from active/timestamp pairs)
    if let Some(eng_h) = dev.engine_group {
        let mut activity = CtlEngineStats::new();
        // SAFETY: eng_h cached from discovery; activity is a valid
        // IGCL-versioned struct.
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
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                "IGCL engine activity query failed",
            );
            status.downgrade(CollectStatus::Degraded("igcl engine"));
        }
    }

    // Frequency
    if let Some(freq_h) = dev.freq_domain {
        let mut state = CtlFreqState::new();
        // SAFETY: freq_h cached from discovery; state is a valid
        // IGCL-versioned struct.
        let ret = unsafe { (igcl.freq_get_state)(freq_h.0, &mut state) };
        if ret == CTL_RESULT_SUCCESS && state.actual > 0.0 {
            info.gpu_clock_speed = state.actual as u32;
        } else if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
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
        let timestamp = telemetry.time_stamp.get();

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
            name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
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
        let mut props = CtlDeviceAdapterProperties::new();
        // SAFETY: dev is a valid IGCL device handle; props is a
        // valid IGCL-versioned struct with size and version set.
        let ret = unsafe { (session.igcl.get_device_props)(dev, &mut props) };
        if ret != CTL_RESULT_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuIgcl,
                code = %crate::log::Hex(ret),
                name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                "ctlGetDeviceProperties failed",
            );
            continue;
        }

        let name = name_from_buf(&props.name);
        if name.is_empty() {
            continue;
        }

        // Cache first sub-handle of each telemetry type
        let temp_sensors = unsafe {
            enum_handles(
                dev,
                "ctlEnumTemperatureSensors",
                session.igcl.enum_temp_sensors,
            )
        };
        let mem_modules =
            unsafe { enum_handles(dev, "ctlEnumMemoryModules", session.igcl.enum_mem_modules) };
        let engine_groups =
            unsafe { enum_handles(dev, "ctlEnumEngineGroups", session.igcl.enum_engine_groups) };
        let freq_domains = unsafe {
            enum_handles(
                dev,
                "ctlEnumFrequencyDomains",
                session.igcl.enum_freq_domains,
            )
        };

        // Query max power limit
        let power_domains =
            unsafe { enum_handles(dev, "ctlEnumPowerDomains", session.igcl.enum_power_domains) };
        let mut max_power_mw: i64 = 0;
        if let Some(&pwr) = power_domains.first() {
            let mut power_props = CtlPowerProperties::new();
            // SAFETY: pwr is a valid power handle; power_props is a
            // valid IGCL-versioned struct.
            let ret = unsafe { (session.igcl.power_get_props)(pwr, &mut power_props) };
            if ret == CTL_RESULT_SUCCESS {
                if power_props.max_limit > 0 {
                    max_power_mw = power_props.max_limit as i64;
                }
            } else {
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::GpuIgcl,
                    op = "ctlPowerGetProperties",
                    code = %crate::log::Hex(ret),
                    name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                    "IGCL power properties query failed",
                );
            }
        }

        // Query initial memory total
        let mut mem_total: u64 = 0;
        if let Some(&mem_h) = mem_modules.first() {
            let mut mem_state = CtlMemState::new();
            // SAFETY: mem_h is a valid memory handle; mem_state is a
            // valid IGCL-versioned struct.
            let ret = unsafe { (session.igcl.mem_get_state)(mem_h, &mut mem_state) };
            if ret == CTL_RESULT_SUCCESS {
                mem_total = mem_state.total;
            } else {
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::GpuIgcl,
                    op = "ctlMemoryGetState",
                    code = %crate::log::Hex(ret),
                    name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                    "IGCL memory state query (discovery) failed",
                );
            }
        }

        // Query max clock from frequency domain
        let mut max_clock: u32 = 0;
        if let Some(&freq_h) = freq_domains.first() {
            let mut freq_state = CtlFreqState::new();
            // SAFETY: freq_h is a valid frequency handle; freq_state
            // is a valid IGCL-versioned struct.
            let ret = unsafe { (session.igcl.freq_get_state)(freq_h, &mut freq_state) };
            if ret == CTL_RESULT_SUCCESS {
                if freq_state.tdp > 0.0 {
                    max_clock = freq_state.tdp as u32;
                }
            } else {
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::GpuIgcl,
                    op = "ctlFrequencyGetState",
                    code = %crate::log::Hex(ret),
                    name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
                    "IGCL frequency state query (discovery) failed",
                );
            }
        }

        // Per-device discovery summary. Logged at `info` — the
        // appropriate level per the conventions for a vendor-detected
        // lifecycle event. Reporters who set `log_level = "info"` in
        // `rtop.toml` get a one-line snapshot per device that almost
        // always identifies the root cause of missing telemetry
        // (e.g. `temp_sensors = 0` means the driver exposes no per-
        // sensor handle and the temp box will read zero through the
        // existing path).
        tracing::info!(
            subsystem = %crate::log::Subsystem::GpuIgcl,
            device = %name,
            temp_sensors = temp_sensors.len(),
            mem_modules = mem_modules.len(),
            engine_groups = engine_groups.len(),
            freq_domains = freq_domains.len(),
            power_domains = power_domains.len(),
            mem_total_bytes = mem_total,
            max_power_mw,
            max_clock_mhz = max_clock,
            "device discovered",
        );

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
///
/// `op` names the IGCL call (e.g. `"ctlEnumTemperatureSensors"`) and is
/// included in the `debug` log emitted on the failure path so field
/// reports can identify which enumeration produced an empty handle set
/// because of a hard error versus a legitimately empty count.
unsafe fn enum_handles<H: Copy>(
    device: CtlDeviceHandle,
    op: &'static str,
    enum_fn: unsafe extern "C" fn(CtlDeviceHandle, *mut u32, *mut H) -> u32,
) -> Vec<H> {
    let mut count: u32 = 0;
    // SAFETY: device is a valid IGCL device handle (caller's responsibility
    // per the `unsafe fn` contract); a null target pointer with a valid
    // &mut u32 count is the documented IGCL "query count" call shape.
    let ret = unsafe { enum_fn(device, &mut count, std::ptr::null_mut()) };
    if ret != CTL_RESULT_SUCCESS {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuIgcl,
            op,
            code = %crate::log::Hex(ret),
            name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
            "IGCL enumeration (count) failed",
        );
        return Vec::new();
    }
    if count == 0 {
        return Vec::new();
    }
    // SAFETY: MaybeUninit<H> where H is a pointer type — zeroed is valid for pointers.
    let mut handles: Vec<H> = vec![unsafe { std::mem::zeroed() }; count as usize];
    // SAFETY: device is a valid IGCL device handle; handles is sized to
    // `count` (the value the previous call returned), so the IGCL fill
    // pass writes within the allocated buffer.
    let ret = unsafe { enum_fn(device, &mut count, handles.as_mut_ptr()) };
    if ret != CTL_RESULT_SUCCESS {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuIgcl,
            op,
            code = %crate::log::Hex(ret),
            name = ctl_result_name(ret).unwrap_or("UNKNOWN"),
            "IGCL enumeration (fill) failed",
        );
        return Vec::new();
    }
    handles.truncate(count as usize);
    handles
}

#[cfg(test)]
mod abi_tests {
    use super::*;

    /// Pins every IGCL FFI struct size to the value MSVC x64 produces from
    /// the canonical `igcl_api.h`. If a future struct edit drifts from the
    /// Intel ABI, the failing assertion identifies which type regressed,
    /// before the user sees `CTL_RESULT_ERROR_INVALID_SIZE` (`0x4000000F`)
    /// at runtime.
    #[test]
    fn struct_sizes_match_igcl_abi() {
        assert_eq!(std::mem::size_of::<CtlApplicationId>(), 16);
        assert_eq!(std::mem::size_of::<CtlInitArgs>(), 36);
        assert_eq!(std::mem::size_of::<CtlFirmwareVersion>(), 24);
        assert_eq!(std::mem::size_of::<CtlAdapterBdf>(), 3);
        assert_eq!(std::mem::size_of::<CtlDeviceAdapterProperties>(), 320);
        assert_eq!(std::mem::size_of::<CtlMemState>(), 24);
        assert_eq!(std::mem::size_of::<CtlEngineStats>(), 24);
        assert_eq!(std::mem::size_of::<CtlFreqState>(), 56);
        assert_eq!(std::mem::size_of::<CtlPowerProperties>(), 20);
        assert_eq!(std::mem::size_of::<CtlDataValue>(), 8);
        assert_eq!(std::mem::size_of::<CtlTelemetryItem>(), 24);
        assert_eq!(std::mem::size_of::<CtlPsuInfo>(), 56);
        assert_eq!(std::mem::size_of::<CtlPowerTelemetry>(), 1024);
    }

    #[test]
    fn ctl_result_name_known_codes() {
        assert_eq!(
            ctl_result_name(0x4000000F),
            Some("CTL_RESULT_ERROR_INVALID_SIZE")
        );
        assert_eq!(ctl_result_name(0x00000000), Some("CTL_RESULT_SUCCESS"));
        assert_eq!(
            ctl_result_name(0x40000007),
            Some("CTL_RESULT_ERROR_NOT_AVAILABLE")
        );
        assert_eq!(ctl_result_name(0xDEADBEEF), None);
    }
}
