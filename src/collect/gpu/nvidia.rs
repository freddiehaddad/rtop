use crate::collect::CollectStatus;
use crate::collect::win::OwnedLibrary;
use std::ffi::{c_char, c_void};

use super::{clamp_percent, power_percent, push_history};

// ---------------------------------------------------------------------------
// NvAPI constants
// ---------------------------------------------------------------------------

const NVAPI_OK: i32 = 0;
const NVAPI_MAX_PHYSICAL_GPUS: usize = 64;
const NVAPI_SHORT_STRING_MAX: usize = 64;

/// Graphics clock domain index in `NvClockFrequencies`.
const CLOCK_DOMAIN_GRAPHICS: usize = 0;

/// NvAPI clock type: current frequency.
const CLOCK_TYPE_CURRENT: u32 = 0;
/// NvAPI clock type: boost frequency (used for max clock).
const CLOCK_TYPE_BOOST: u32 = 2;

/// Power topology domain representing total board power.
const POWER_DOMAIN_BOARD: u32 = 0;

/// Per Cent Mille base: 100,000 PCM = 100%.
const PCM_100_PERCENT: u64 = 100_000;

/// Thermal target for the GPU die.
const NVAPI_THERMAL_TARGET_GPU: i32 = 1;
/// Request all thermal sensors.
const NVAPI_THERMAL_TARGET_ALL: u32 = 15;

// ---------------------------------------------------------------------------
// NVML types (minimal — only for default power limit / TDP lookup)
// ---------------------------------------------------------------------------

const NVML_SUCCESS: u32 = 0;

type NvmlDevice = *mut c_void;
type NvmlInitV2 = unsafe extern "C" fn() -> u32;
type NvmlShutdownFn = unsafe extern "C" fn() -> u32;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut u32) -> u32;
type NvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(u32, *mut NvmlDevice) -> u32;
type NvmlDeviceGetPowerManagementDefaultLimit = unsafe extern "C" fn(NvmlDevice, *mut u32) -> u32;

// ---------------------------------------------------------------------------
// NvAPI repr(C) structs
// ---------------------------------------------------------------------------

type NvPhysicalGpuHandle = *mut c_void;

/// GPU utilization domain (one per metric type).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvUtilDomain {
    /// Bit 0: domain is present.
    flags: u32,
    /// Utilization percentage (0–100).
    percentage: u32,
}

/// Dynamic P-state utilization info.
#[repr(C)]
struct NvDynamicPstatesInfoEx {
    version: u32,
    flags: u32,
    utilizations: [NvUtilDomain; 8],
}

/// Single thermal sensor reading.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvThermalSensor {
    controller: i32,
    default_min_temp: i32,
    default_max_temp: i32,
    current_temp: i32,
    target: i32,
}

/// GPU thermal settings (up to 3 sensors).
#[repr(C)]
struct NvThermalSettings {
    version: u32,
    count: u32,
    sensors: [NvThermalSensor; 3],
}

/// Display driver memory info (V3). All size fields are in KB.
#[repr(C)]
struct NvMemoryInfo {
    version: u32,
    dedicated_video_memory_kb: u32,
    avail_dedicated_video_memory_kb: u32,
    system_video_memory_kb: u32,
    shared_system_memory_kb: u32,
    cur_avail_dedicated_video_memory_kb: u32,
    dedicated_video_memory_evictions_size_kb: u32,
    dedicated_video_memory_eviction_count: u32,
}

/// Single clock domain entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvClockDomain {
    /// Bit 0: domain is present.
    flags: u32,
    /// Frequency in kHz.
    frequency_khz: u32,
}

/// GPU clock frequencies (up to 32 domains).
#[repr(C)]
struct NvClockFrequencies {
    version: u32,
    clock_type: u32,
    domains: [NvClockDomain; 32],
}

/// Single power topology entry (undocumented NvAPI).
/// Values are in Per Cent Mille (PCM): 100,000 = 100% of TDP.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvPowerTopoEntry {
    domain: u32,
    flags: u32,
    power_pcm: u32,
    unknown: u32,
}

/// Power topology status (up to 4 entries, undocumented NvAPI).
#[repr(C)]
struct NvPowerTopo {
    version: u32,
    count: u32,
    entries: [NvPowerTopoEntry; 4],
}

/// Single power info entry (undocumented NvAPI). Values are in PCM.
/// Fields at known offsets hold min/default/max power as a percentage
/// of TDP (100,000 PCM = 100%); intervening fields are reserved.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvPowerInfoEntry {
    pstate: u32,
    reserved_0: [u32; 2],
    min_power_pcm: u32,
    reserved_1: [u32; 2],
    def_power_pcm: u32,
    reserved_2: [u32; 2],
    max_power_pcm: u32,
    reserved_3: u32,
}

/// Power policies info (up to 4 entries, undocumented NvAPI).
#[repr(C)]
struct NvPowerInfo {
    version: u32,
    valid: u8,
    count: u8,
    padding: [u8; 2],
    entries: [NvPowerInfoEntry; 4],
}

// ---------------------------------------------------------------------------
// NvAPI struct version constants (computed at compile time)
// ---------------------------------------------------------------------------

/// NvAPI struct version: `sizeof(T) | (ver << 16)`.
const fn nvapi_version<T>(ver: u32) -> u32 {
    std::mem::size_of::<T>() as u32 | (ver << 16)
}

const PSTATES_INFO_VER: u32 = nvapi_version::<NvDynamicPstatesInfoEx>(1);
const THERMAL_SETTINGS_VER: u32 = nvapi_version::<NvThermalSettings>(2);
const MEMORY_INFO_VER: u32 = nvapi_version::<NvMemoryInfo>(3);
const CLOCK_FREQUENCIES_VER: u32 = nvapi_version::<NvClockFrequencies>(3);
const POWER_TOPO_VER: u32 = nvapi_version::<NvPowerTopo>(1);
const POWER_INFO_VER: u32 = nvapi_version::<NvPowerInfo>(1);

// ---------------------------------------------------------------------------
// NvAPI function pointer types
// ---------------------------------------------------------------------------

type NvApiInitialize = unsafe extern "C" fn() -> i32;
type NvApiUnload = unsafe extern "C" fn() -> i32;
type NvApiEnumPhysicalGPUs =
    unsafe extern "C" fn(handles: *mut NvPhysicalGpuHandle, count: *mut u32) -> i32;
type NvApiGpuGetFullName =
    unsafe extern "C" fn(handle: NvPhysicalGpuHandle, name: *mut c_char) -> i32;
type NvApiGetDynamicPstatesInfoEx =
    unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvDynamicPstatesInfoEx) -> i32;
type NvApiGetThermalSettings =
    unsafe extern "C" fn(NvPhysicalGpuHandle, u32, *mut NvThermalSettings) -> i32;
type NvApiGetMemoryInfo = unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvMemoryInfo) -> i32;
type NvApiGetAllClockFrequencies =
    unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvClockFrequencies) -> i32;
type NvApiClientPowerTopoGetStatus =
    unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvPowerTopo) -> i32;
type NvApiClientPowerPoliciesGetInfo =
    unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvPowerInfo) -> i32;

// ---------------------------------------------------------------------------
// NvAPI function IDs (resolved via nvapi_QueryInterface)
// ---------------------------------------------------------------------------

const ID_INITIALIZE: u32 = 0x0150_e828;
const ID_UNLOAD: u32 = 0xd22b_dd7e;
const ID_ENUM_PHYSICAL_GPUS: u32 = 0xe5ac_921f;
const ID_GPU_GET_FULL_NAME: u32 = 0xceee_8e9f;
const ID_GPU_GET_DYNAMIC_PSTATES_INFO_EX: u32 = 0x60de_d2ed;
const ID_GPU_GET_THERMAL_SETTINGS: u32 = 0xe364_0a56;
const ID_GPU_GET_MEMORY_INFO: u32 = 0x07f9_b368;
const ID_GPU_GET_ALL_CLOCK_FREQUENCIES: u32 = 0xdcb6_16c3;
const ID_GPU_CLIENT_POWER_TOPO_GET_STATUS: u32 = 0xedcf_624e;
const ID_GPU_CLIENT_POWER_POLICIES_GET_INFO: u32 = 0x3420_6d86;

// ---------------------------------------------------------------------------
// NVML TDP lookup helper
// ---------------------------------------------------------------------------

/// Query the default power management limit (TDP) in milliwatts for each GPU
/// via NVML. Returns a vec of per-device TDP values. Returns an empty vec if
/// NVML is unavailable (e.g. not installed).
fn query_nvml_tdp(device_count: usize) -> Option<Vec<u32>> {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::PCSTR;

    let dll_name: Vec<u16> = "nvml.dll\0".encode_utf16().collect();
    // SAFETY: LoadLibraryW receives a valid null-terminated UTF-16 DLL name.
    let library =
        OwnedLibrary::new(unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())) }.ok()?)?;
    let handle = library.get();

    macro_rules! load_fn {
        ($name:literal, $ty:ty) => {{
            // SAFETY: handle is a valid loaded nvml.dll module.
            let proc = unsafe { GetProcAddress(handle, PCSTR(concat!($name, "\0").as_ptr())) }?;
            // SAFETY: GetProcAddress returned a non-null address for a known
            // NVML symbol; the target type matches the documented signature.
            unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc) }
        }};
    }

    let init: NvmlInitV2 = load_fn!("nvmlInit_v2", NvmlInitV2);
    let shutdown: NvmlShutdownFn = load_fn!("nvmlShutdown", NvmlShutdownFn);
    let get_count: NvmlDeviceGetCountV2 = load_fn!("nvmlDeviceGetCount_v2", NvmlDeviceGetCountV2);
    let get_handle: NvmlDeviceGetHandleByIndexV2 = load_fn!(
        "nvmlDeviceGetHandleByIndex_v2",
        NvmlDeviceGetHandleByIndexV2
    );
    let get_default_limit: NvmlDeviceGetPowerManagementDefaultLimit = load_fn!(
        "nvmlDeviceGetPowerManagementDefaultLimit",
        NvmlDeviceGetPowerManagementDefaultLimit
    );

    // SAFETY: init was loaded from nvml.dll.
    let ret = unsafe { init() };
    if ret != NVML_SUCCESS {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuNvml,
            code = %crate::log::Hex(ret),
            "nvmlInit_v2 failed",
        );
        return None;
    }

    let mut nvml_count: u32 = 0;
    // SAFETY: nvml_count is a valid pointer.
    let _ = unsafe { get_count(&mut nvml_count) };

    let limit = (nvml_count as usize).min(device_count);
    let mut tdps = Vec::with_capacity(limit);
    for i in 0..limit as u32 {
        let mut dev: NvmlDevice = std::ptr::null_mut();
        // SAFETY: i is within the reported count.
        if unsafe { get_handle(i, &mut dev) } != NVML_SUCCESS || dev.is_null() {
            tdps.push(0);
            continue;
        }
        let mut limit_mw: u32 = 0;
        // SAFETY: dev is a valid handle; limit_mw is a valid pointer.
        let ret = unsafe { get_default_limit(dev, &mut limit_mw) };
        if ret == NVML_SUCCESS {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvml,
                device = i,
                limit_mw,
                "default power limit read",
            );
            tdps.push(limit_mw);
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvml,
                device = i,
                code = %crate::log::Hex(ret),
                "GetPowerManagementDefaultLimit failed",
            );
            tdps.push(0);
        }
    }

    // SAFETY: shutdown matches nvmlShutdown. Library handle stays valid
    // via OwnedLibrary until this function returns.
    unsafe { shutdown() };
    // OwnedLibrary drops here, freeing nvml.dll.

    Some(tdps)
}

/// Convert a Per Cent Mille value to milliwatts using a TDP reference.
fn pcm_to_mw(pcm: u64, tdp_mw: u64) -> u64 {
    pcm.saturating_mul(tdp_mw) / PCM_100_PERCENT
}

// ---------------------------------------------------------------------------
// NvApiFunctions — dynamically loaded function table
// ---------------------------------------------------------------------------

struct NvApiFunctions {
    _library: OwnedLibrary,
    initialize: NvApiInitialize,
    unload: NvApiUnload,
    enum_physical_gpus: NvApiEnumPhysicalGPUs,
    gpu_get_full_name: NvApiGpuGetFullName,
    get_dynamic_pstates_info_ex: NvApiGetDynamicPstatesInfoEx,
    get_thermal_settings: NvApiGetThermalSettings,
    get_memory_info: NvApiGetMemoryInfo,
    get_all_clock_frequencies: NvApiGetAllClockFrequencies,
    client_power_topo_get_status: NvApiClientPowerTopoGetStatus,
    client_power_policies_get_info: NvApiClientPowerPoliciesGetInfo,
}

impl NvApiFunctions {
    fn load() -> Option<Self> {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::PCSTR;

        let dll_name: Vec<u16> = "nvapi64.dll\0".encode_utf16().collect();
        // SAFETY: LoadLibraryW receives a valid null-terminated UTF-16 DLL name.
        // The returned handle is checked via ok()? before use.
        let library = OwnedLibrary::new(
            unsafe { LoadLibraryW(windows::core::PCWSTR(dll_name.as_ptr())) }.ok()?,
        )?;

        // NvAPI exports a single symbol: nvapi_QueryInterface.
        // All other functions are resolved by numeric ID through it.
        let handle = library.get();
        // SAFETY: handle is a valid loaded nvapi64.dll module. The symbol name
        // is a static null-terminated byte string.
        let query_interface: unsafe extern "C" fn(u32) -> *mut c_void = unsafe {
            let proc = GetProcAddress(handle, PCSTR(c"nvapi_QueryInterface".as_ptr().cast()))?;
            std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(proc)
        };

        /// Resolve an NvAPI function pointer by ID. Returns `None` if the
        /// function is not present in this driver version.
        macro_rules! resolve {
            ($id:expr, $ty:ty) => {{
                // SAFETY: query_interface was obtained from a valid nvapi64.dll
                // export. The ID is a known NvAPI function identifier.
                let ptr = unsafe { query_interface($id) };
                if ptr.is_null() {
                    return None;
                }
                // SAFETY: query_interface returned a non-null pointer for a
                // known NvAPI function ID. The target type matches the
                // documented NvAPI signature for this ID.
                unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
            }};
        }

        Some(Self {
            _library: library,
            initialize: resolve!(ID_INITIALIZE, NvApiInitialize),
            unload: resolve!(ID_UNLOAD, NvApiUnload),
            enum_physical_gpus: resolve!(ID_ENUM_PHYSICAL_GPUS, NvApiEnumPhysicalGPUs),
            gpu_get_full_name: resolve!(ID_GPU_GET_FULL_NAME, NvApiGpuGetFullName),
            get_dynamic_pstates_info_ex: resolve!(
                ID_GPU_GET_DYNAMIC_PSTATES_INFO_EX,
                NvApiGetDynamicPstatesInfoEx
            ),
            get_thermal_settings: resolve!(ID_GPU_GET_THERMAL_SETTINGS, NvApiGetThermalSettings),
            get_memory_info: resolve!(ID_GPU_GET_MEMORY_INFO, NvApiGetMemoryInfo),
            get_all_clock_frequencies: resolve!(
                ID_GPU_GET_ALL_CLOCK_FREQUENCIES,
                NvApiGetAllClockFrequencies
            ),
            client_power_topo_get_status: resolve!(
                ID_GPU_CLIENT_POWER_TOPO_GET_STATUS,
                NvApiClientPowerTopoGetStatus
            ),
            client_power_policies_get_info: resolve!(
                ID_GPU_CLIENT_POWER_POLICIES_GET_INFO,
                NvApiClientPowerPoliciesGetInfo
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// NvApiSession — shared per-vendor session
// ---------------------------------------------------------------------------

/// `Send + Sync` newtype around `NvPhysicalGpuHandle` (`*mut c_void`).
///
/// NvAPI handles are opaque identifiers that NvAPI dereferences
/// internally; the rtop side never reads through them. NvAPI itself
/// is documented thread-safe per `nvapi.h`, so a handle owned by
/// one device thread can be passed back to NvAPI from that same
/// thread without external synchronisation. The wrapper exists
/// purely to satisfy the `Send + Sync` requirement on the per-device
/// collector type that crosses a thread boundary in
/// [`crate::runner::spawn_collector`].
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SendNvHandle(NvPhysicalGpuHandle);

// SAFETY: NvAPI handles are opaque identifiers used only by NvAPI
// itself, which is thread-safe per nvapi.h. We never dereference the
// pointer from Rust.
unsafe impl Send for SendNvHandle {}
// SAFETY: see Send impl above.
unsafe impl Sync for SendNvHandle {}

/// Shared NvAPI session: loaded library, resolved function-pointer
/// table, and the initialised NvAPI runtime.
///
/// Constructed once during [`discover`] and shared across every
/// `NvidiaDeviceCollector` via `Arc<NvApiSession>`. The `Drop` impl
/// calls `NvAPI_Unload` exactly once when the last device thread
/// releases its `Arc` — NvAPI's documented refcounted-init model
/// means the unload only takes effect when the refcount drops to
/// zero, but rtop matches that with a single `Initialize`/`Unload`
/// pair per process.
pub(super) struct NvApiSession {
    nvapi: NvApiFunctions,
}

impl NvApiSession {
    /// Load `nvapi64.dll`, resolve the function table, and call
    /// `NvAPI_Initialize`. Returns `None` if the DLL is absent or
    /// initialise fails.
    fn load() -> Option<Self> {
        let nvapi = NvApiFunctions::load()?;

        // SAFETY: nvapi.initialize was resolved from nvapi64.dll
        // and matches the NvAPI_Initialize signature.
        let ret = unsafe { (nvapi.initialize)() };
        if ret != NVAPI_OK {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvapi,
                code = %crate::log::Hex(ret),
                "NvAPI_Initialize failed",
            );
            return None;
        }
        Some(Self { nvapi })
    }
}

impl Drop for NvApiSession {
    fn drop(&mut self) {
        // SAFETY: nvapi.unload was resolved from nvapi64.dll alongside
        // initialize. We call it exactly once per successful
        // `Initialize` (the session is constructed only when
        // initialize returns NVAPI_OK).
        unsafe {
            let _ = (self.nvapi.unload)();
        }
    }
}

// ---------------------------------------------------------------------------
// NvidiaDeviceCollector — one per detected NVIDIA GPU
// ---------------------------------------------------------------------------

/// Per-device NVIDIA GPU collector. Owns its `Arc<NvApiSession>`
/// clone, its NVAPI device handle, the device's TDP (queried from
/// NVML during discovery), and the in-memory `GpuInfo` that the
/// renderer reads.
pub(crate) struct NvidiaDeviceCollector {
    session: std::sync::Arc<NvApiSession>,
    device: SendNvHandle,
    /// Default power limit (TDP) in milliwatts, from NVML during
    /// discovery. Used to convert NvAPI Per-Cent-Mille power
    /// readings into milliwatts.
    tdp_mw: u64,
    info: crate::domain::gpu::GpuInfo,
    status: CollectStatus,
}

impl NvidiaDeviceCollector {
    pub(super) fn collect(&mut self) {
        self.status = CollectStatus::Ok;
        let session = &self.session.nvapi;
        let device = self.device.0;

        // Utilization
        let mut pstates = NvDynamicPstatesInfoEx {
            version: PSTATES_INFO_VER,
            flags: 0,
            utilizations: [NvUtilDomain::default(); 8],
        };
        // SAFETY: device is a valid handle obtained during discovery.
        let ret = unsafe { (session.get_dynamic_pstates_info_ex)(device, &mut pstates) };
        if ret == NVAPI_OK {
            let gpu_util = &pstates.utilizations[0];
            if gpu_util.flags & 1 != 0 {
                push_history(
                    &mut self.info.gpu_percent.utilization,
                    clamp_percent(gpu_util.percentage),
                );
            }
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvapi,
                code = %crate::log::Hex(ret),
                "NvAPI utilization query failed",
            );
            self.status
                .downgrade(CollectStatus::Degraded("nvapi utilization"));
        }

        // Temperature — request all sensors, pick the GPU target.
        let mut thermal = NvThermalSettings {
            version: THERMAL_SETTINGS_VER,
            count: 0,
            sensors: [NvThermalSensor::default(); 3],
        };
        // SAFETY: device is valid; NVAPI_THERMAL_TARGET_ALL requests all sensors.
        let ret = unsafe {
            (session.get_thermal_settings)(device, NVAPI_THERMAL_TARGET_ALL, &mut thermal)
        };
        if ret == NVAPI_OK {
            let temp = thermal.sensors[..thermal.count as usize]
                .iter()
                .find(|s| s.target == NVAPI_THERMAL_TARGET_GPU)
                .or_else(|| thermal.sensors.first())
                .map(|s| s.current_temp as i64)
                .unwrap_or(0);
            push_history(&mut self.info.temp, temp);
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvapi,
                code = %crate::log::Hex(ret),
                "NvAPI temperature query failed",
            );
            self.status
                .downgrade(CollectStatus::Degraded("nvapi temperature"));
        }

        // Memory (values in KB)
        let mut mem = NvMemoryInfo {
            version: MEMORY_INFO_VER,
            dedicated_video_memory_kb: 0,
            avail_dedicated_video_memory_kb: 0,
            system_video_memory_kb: 0,
            shared_system_memory_kb: 0,
            cur_avail_dedicated_video_memory_kb: 0,
            dedicated_video_memory_evictions_size_kb: 0,
            dedicated_video_memory_eviction_count: 0,
        };
        // SAFETY: device is valid; mem is a valid versioned struct.
        let ret = unsafe { (session.get_memory_info)(device, &mut mem) };
        if ret == NVAPI_OK {
            let total = mem.dedicated_video_memory_kb as u64 * 1024;
            let avail = mem.cur_avail_dedicated_video_memory_kb as u64 * 1024;
            let used = total.saturating_sub(avail);
            self.info.mem_total = total;
            self.info.mem_used = used;
            let vram_pct = crate::collect::win::percent_u64(used, total).min(100);
            push_history(&mut self.info.gpu_percent.vram, vram_pct);
            push_history(&mut self.info.mem_utilization_percent, vram_pct);
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvapi,
                code = %crate::log::Hex(ret),
                "NvAPI memory query failed",
            );
            self.status
                .downgrade(CollectStatus::Degraded("nvapi memory"));
        }

        // Power — pick the Board domain entry (PCM) and convert to mW.
        if self.tdp_mw > 0 {
            let mut topo = NvPowerTopo {
                version: POWER_TOPO_VER,
                count: 0,
                entries: [NvPowerTopoEntry::default(); 4],
            };
            // SAFETY: device is valid; topo is a valid versioned struct.
            let ret = unsafe { (session.client_power_topo_get_status)(device, &mut topo) };
            if ret == NVAPI_OK && topo.count > 0 {
                let n = (topo.count as usize).min(topo.entries.len());
                let board_pcm = topo.entries[..n]
                    .iter()
                    .find(|e| e.domain == POWER_DOMAIN_BOARD)
                    .or_else(|| topo.entries.first())
                    .map(|e| e.power_pcm as u64)
                    .unwrap_or(0);
                let power_mw = pcm_to_mw(board_pcm, self.tdp_mw);
                self.info.pwr_usage = power_mw as i64;
                let pwr_pct = power_percent(power_mw, self.info.pwr_max_usage as u64);
                push_history(&mut self.info.gpu_percent.power, pwr_pct);
            } else {
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::GpuNvapi,
                    code = %crate::log::Hex(ret),
                    "NvAPI power topology query failed",
                );
                self.status
                    .downgrade(CollectStatus::Degraded("nvapi power"));
            }
        }

        // Clock speed (current, in kHz → MHz)
        let mut clocks = NvClockFrequencies {
            version: CLOCK_FREQUENCIES_VER,
            clock_type: CLOCK_TYPE_CURRENT,
            domains: [NvClockDomain::default(); 32],
        };
        // SAFETY: device is valid; clocks is a valid versioned struct.
        let ret = unsafe { (session.get_all_clock_frequencies)(device, &mut clocks) };
        if ret == NVAPI_OK {
            let gfx = &clocks.domains[CLOCK_DOMAIN_GRAPHICS];
            if gfx.flags & 1 != 0 {
                self.info.gpu_clock_speed = gfx.frequency_khz / 1000;
            }
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuNvapi,
                code = %crate::log::Hex(ret),
                "NvAPI clock query failed",
            );
            self.status
                .downgrade(CollectStatus::Degraded("nvapi clock"));
        }
    }

    pub(super) fn snapshot(&self) -> crate::runner::GpuSnapshot {
        crate::runner::GpuSnapshot {
            info: self.info.clone(),
            status: self.status.clone(),
        }
    }
}

/// Discover every NVIDIA GPU and return one
/// [`super::DeviceCollector`] wrapper per device. Returns an empty
/// vector if NvAPI is unavailable or no GPUs are detected.
pub(super) fn discover() -> Vec<super::DeviceCollector> {
    let Some(session) = NvApiSession::load() else {
        return Vec::new();
    };
    let session = std::sync::Arc::new(session);

    let mut handles = [std::ptr::null_mut::<c_void>(); NVAPI_MAX_PHYSICAL_GPUS];
    let mut count: u32 = 0;
    // SAFETY: handles is a valid array of NVAPI_MAX_PHYSICAL_GPUS
    // pointers; count is a valid pointer to a stack-allocated u32.
    let ret = unsafe { (session.nvapi.enum_physical_gpus)(handles.as_mut_ptr(), &mut count) };
    if ret != NVAPI_OK {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::GpuNvapi,
            code = %crate::log::Hex(ret),
            "NvAPI_EnumPhysicalGPUs failed",
        );
        return Vec::new();
    }
    let count = (count as usize).min(NVAPI_MAX_PHYSICAL_GPUS);
    if count == 0 {
        return Vec::new();
    }
    let devices: Vec<NvPhysicalGpuHandle> = handles[..count].to_vec();

    // Query default TDP per device from NVML (needed to convert
    // NvAPI PCM power values to milliwatts).
    let tdp_mw_vec: Vec<u64> = query_nvml_tdp(count)
        .unwrap_or_default()
        .into_iter()
        .map(u64::from)
        .chain(std::iter::repeat(0u64))
        .take(count)
        .collect();

    tracing::info!(
        subsystem = %crate::log::Subsystem::GpuNvapi,
        devices = count,
        "vendor initialized",
    );

    devices
        .into_iter()
        .zip(tdp_mw_vec)
        .map(|(device, tdp_mw)| {
            let info = build_initial_info(&session.nvapi, device, tdp_mw);
            super::DeviceCollector::Nvidia(NvidiaDeviceCollector {
                session: std::sync::Arc::clone(&session),
                device: SendNvHandle(device),
                tdp_mw,
                info,
                status: CollectStatus::Ok,
            })
        })
        .collect()
}

/// Pre-fill the per-device `GpuInfo` with static fields (name,
/// max-power reference, max boost clock) that the collector
/// renders but never re-queries on each cycle. Mirrors AMD's
/// [`build_initial_info`](super::amd) helper for parity.
fn build_initial_info(
    nvapi: &NvApiFunctions,
    device: NvPhysicalGpuHandle,
    tdp_mw: u64,
) -> crate::domain::gpu::GpuInfo {
    let mut info = crate::domain::gpu::GpuInfo::default();

    // GPU name.
    let mut name_buf = [0u8; NVAPI_SHORT_STRING_MAX];
    // SAFETY: device is a valid handle; name_buf is 64 bytes
    // matching NvAPI_ShortString.
    let ret = unsafe { (nvapi.gpu_get_full_name)(device, name_buf.as_mut_ptr() as *mut c_char) };
    if ret == NVAPI_OK {
        info.name = crate::collect::win::string_from_c_buf(&name_buf);
    }

    // Power limit — use TDP from NVML, scaled by the NvAPI power
    // policy max percentage (PCM) when the user has raised the
    // limit above default.
    if tdp_mw > 0 {
        info.pwr_max_usage = tdp_mw as i64;

        let mut power_info = NvPowerInfo {
            version: POWER_INFO_VER,
            valid: 0,
            count: 0,
            padding: [0; 2],
            entries: [NvPowerInfoEntry::default(); 4],
        };
        // SAFETY: device is valid; power_info is a valid versioned struct.
        let ret = unsafe { (nvapi.client_power_policies_get_info)(device, &mut power_info) };
        if ret == NVAPI_OK && power_info.count > 0 {
            let max_pcm = power_info.entries[0].max_power_pcm as u64;
            if max_pcm > PCM_100_PERCENT {
                info.pwr_max_usage = pcm_to_mw(max_pcm, tdp_mw) as i64;
            }
        }
    }

    // Max clock (boost frequency).
    let mut clocks = NvClockFrequencies {
        version: CLOCK_FREQUENCIES_VER,
        clock_type: CLOCK_TYPE_BOOST,
        domains: [NvClockDomain::default(); 32],
    };
    // SAFETY: device is valid; clocks is a valid versioned struct.
    let ret = unsafe { (nvapi.get_all_clock_frequencies)(device, &mut clocks) };
    if ret == NVAPI_OK {
        let gfx = &clocks.domains[CLOCK_DOMAIN_GRAPHICS];
        if gfx.flags & 1 != 0 {
            info.gpu_max_clock_speed = gfx.frequency_khz / 1000;
        }
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_nvapi_layout() {
        assert_eq!(std::mem::size_of::<NvUtilDomain>(), 8);
        assert_eq!(std::mem::size_of::<NvDynamicPstatesInfoEx>(), 72);
        assert_eq!(std::mem::size_of::<NvThermalSensor>(), 20);
        assert_eq!(std::mem::size_of::<NvThermalSettings>(), 68);
        assert_eq!(std::mem::size_of::<NvMemoryInfo>(), 32);
        assert_eq!(std::mem::size_of::<NvClockDomain>(), 8);
        assert_eq!(std::mem::size_of::<NvClockFrequencies>(), 264);
        assert_eq!(std::mem::size_of::<NvPowerTopoEntry>(), 16);
        assert_eq!(std::mem::size_of::<NvPowerTopo>(), 72);
        assert_eq!(std::mem::size_of::<NvPowerInfoEntry>(), 44);
        assert_eq!(std::mem::size_of::<NvPowerInfo>(), 184);
    }

    #[test]
    fn version_constants_encode_correctly() {
        assert_eq!(PSTATES_INFO_VER, 72 | (1 << 16));
        assert_eq!(THERMAL_SETTINGS_VER, 68 | (2 << 16));
        assert_eq!(MEMORY_INFO_VER, 32 | (3 << 16));
        assert_eq!(CLOCK_FREQUENCIES_VER, 264 | (3 << 16));
        assert_eq!(POWER_TOPO_VER, 72 | (1 << 16));
        assert_eq!(POWER_INFO_VER, 184 | (1 << 16));
    }
}
