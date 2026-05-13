use crate::collect::CollectStatus;
use crate::collect::win::{OwnedLibrary, percent_u64};
use std::ffi::c_void;

use super::{clamp_percent, power_percent, push_history};

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

/// ADL memory allocation callback.
///
/// Invoked by ADL2 entry points from C code, so a panic across this FFI
/// boundary would be undefined behavior. Non-positive sizes and any
/// `Layout` construction failure are surfaced to ADL as a null return
/// (the documented C-ABI signal for allocation failure) rather than
/// being unwrapped.
unsafe extern "C" fn adl_malloc(size: i32) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let Ok(layout) = std::alloc::Layout::from_size_align(size as usize, 8) else {
        return std::ptr::null_mut();
    };
    // SAFETY: layout has size > 0 (the `size <= 0` early return rejects
    // both the zero-size case `alloc::alloc` documents as UB and the
    // negative-i32 case that would overflow when cast to usize) and a
    // power-of-two alignment of 8. The returned pointer is owned by ADL,
    // which is responsible for releasing it through its internal path.
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
// AMD vendor session and per-device state
// ---------------------------------------------------------------------------

/// Extract a C string from a fixed-size byte buffer (ADL adapter
/// name fields use this shape).
fn string_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

/// AMD vendor session: loaded library, resolved function-pointer
/// table, and one shared `ADL2_Main_Control_Create` context. Owned
/// once by [`super::GpuCollector`] when at least one AMD device is
/// detected; constructed by [`discover`] and dropped when the
/// collector drops.
///
/// The single shared context replaces the pre-collapse
/// per-device-context shape. ADL2 documents "do not share one
/// context handle across threads"; with single-threaded GPU
/// collection that constraint is trivially satisfied — the
/// `GpuCollector` thread is the only thread that ever touches this
/// session.
pub(super) struct AdlSession {
    adl: AdlFunctions,
    /// Opaque ADL2 context pointer from `ADL2_Main_Control_Create`.
    /// Released by [`AdlSession::drop`] via
    /// `ADL2_Main_Control_Destroy`.
    context: *mut c_void,
}

// SAFETY: AdlSession is owned exclusively by the single
// GpuCollector thread; the `context` pointer is opaque and never
// dereferenced from Rust. ADL2's "don't share contexts across
// threads" rule is satisfied because there is exactly one thread
// touching this session for its entire lifetime. No `Sync` impl is
// added — the session is owned (not Arc-shared) so it is never
// borrowed from multiple threads concurrently.
unsafe impl Send for AdlSession {}

impl AdlSession {
    /// Load `atiadlxx.dll`, resolve the function table, and create
    /// the shared ADL2 context. Returns `None` if the library is
    /// absent or context creation fails.
    fn load() -> Option<Self> {
        let adl = AdlFunctions::load()?;
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: adl.main_control_create was loaded from atiadlxx.dll;
        // adl_malloc is a valid C-ABI callback; 1 = enumerate connected
        // adapters only.
        let ret = unsafe { (adl.main_control_create)(adl_malloc, 1, &mut raw) };
        if ret != ADL_OK || raw.is_null() {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuAdl,
                code = %crate::log::Hex(ret),
                "ADL2_Main_Control_Create failed",
            );
            return None;
        }
        Some(Self { adl, context: raw })
    }
}

impl Drop for AdlSession {
    fn drop(&mut self) {
        if !self.context.is_null() {
            // SAFETY: context was created by ADL2_Main_Control_Create
            // on the same function table; main_control_destroy was
            // resolved alongside and is called exactly once per
            // successful create.
            unsafe {
                let _ = (self.adl.main_control_destroy)(self.context);
            }
            self.context = std::ptr::null_mut();
        }
    }
}

/// Slim per-device AMD state. One per detected adapter; held
/// inside [`super::GpuCollector`]'s `Vec<DeviceEntry>`. Carries
/// only the per-device adapter index — every ADL2 call is
/// `(session.context, device.adapter_index, …)`.
pub(super) struct DeviceState {
    pub(super) adapter_index: i32,
}

/// One collection cycle for a single AMD device.
///
/// Reads function pointers and the shared context from `session`,
/// the per-device adapter index from `dev`, mutates the rendered
/// `info` in place, and downgrades `status` on partial failures.
pub(super) fn collect(
    session: &AdlSession,
    dev: &DeviceState,
    info: &mut crate::domain::gpu::GpuInfo,
    status: &mut CollectStatus,
) {
    let adl = &session.adl;
    let ctx = session.context;
    let idx = dev.adapter_index;

    // PMLog one-shot query — provides utilization, clocks, temp, power.
    let mut pmlog = AdlPMLogDataOutput::zeroed();
    // SAFETY: ctx is the shared session context; idx is the
    // adapter index recorded during discovery; pmlog is a valid
    // pointer to a zeroed output struct.
    let ret = unsafe { (adl.query_pmlog_data_get)(ctx, idx, &mut pmlog) };
    if ret == ADL_OK {
        // Utilization (direct %).
        if let Some(pct) = pmlog.get(SENSOR_ACTIVITY_GFX) {
            push_history(
                &mut info.gpu_percent.utilization,
                clamp_percent(pct.max(0) as u32),
            );
        }

        // Clock speed (direct MHz).
        if let Some(mhz) = pmlog.get(SENSOR_CLK_GFXCLK) {
            info.gpu_clock_speed = mhz.max(0) as u32;
        }

        // Temperature (direct °C).
        if let Some(temp) = pmlog.get(SENSOR_TEMPERATURE_EDGE) {
            push_history(&mut info.temp, temp as i64);
        }

        // Power — prefer board power (RDNA3+), fall back to ASIC power.
        // Sensor values are in watts; domain stores milliwatts.
        let power_w = pmlog
            .get(SENSOR_BOARD_POWER)
            .or_else(|| pmlog.get(SENSOR_ASIC_POWER));
        if let Some(w) = power_w {
            let power_mw = w.max(0) as u64 * 1000;
            info.pwr_usage = power_mw as i64;
            let pwr_pct = power_percent(power_mw, info.pwr_max_usage as u64);
            push_history(&mut info.gpu_percent.power, pwr_pct);
        }
    } else {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuAdl,
            code = %crate::log::Hex(ret),
            "ADL PMLog query failed",
        );
        status.downgrade(CollectStatus::Degraded("adl pmlog"));
    }

    // VRAM usage (direct MB from ADL).
    let mut vram_used_mb: i32 = 0;
    // SAFETY: ctx and idx are valid; vram_used_mb is a valid pointer.
    let ret = unsafe { (adl.dedicated_vram_usage_get)(ctx, idx, &mut vram_used_mb) };
    if ret == ADL_OK && vram_used_mb > 0 {
        info.mem_used = vram_used_mb as u64 * 1024 * 1024;
        let vram_pct = percent_u64(info.mem_used, info.mem_total).min(100);
        push_history(&mut info.gpu_percent.vram, vram_pct);
        push_history(&mut info.mem_utilization_percent, vram_pct);
    } else if ret != ADL_OK {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuAdl,
            code = %crate::log::Hex(ret),
            "ADL VRAM usage query failed",
        );
        status.downgrade(CollectStatus::Degraded("adl vram"));
    }
}

/// Discovery result for the AMD vendor.
///
/// Returned by [`discover`] when at least one device was detected;
/// owned by [`super::GpuCollector`] which holds the session for the
/// lifetime of every device entry derived from this bundle.
pub(super) struct AdlBundle {
    pub(super) session: AdlSession,
    pub(super) entries: Vec<(DeviceState, crate::domain::gpu::GpuInfo)>,
}

/// Discover every AMD GPU. Returns `None` when ADL is unavailable
/// or no adapters are detected (vendor session drops on the spot —
/// no library / context resources outlive an empty discovery).
pub(super) fn discover() -> Option<AdlBundle> {
    let session = AdlSession::load()?;

    let adapters = enumerate_adapters(&session.adl, session.context)?;
    if adapters.is_empty() {
        return None;
    }

    tracing::info!(
        subsystem = %crate::log::Subsystem::GpuAdl,
        devices = adapters.len(),
        "vendor initialized",
    );

    let entries: Vec<(DeviceState, crate::domain::gpu::GpuInfo)> = adapters
        .into_iter()
        .enumerate()
        .map(|(idx, adapter)| {
            // Pre-fill static fields (stable id, name, max power,
            // total VRAM) using the shared session context. The
            // device entry retains only its adapter_index for
            // subsequent per-cycle queries.
            let info = build_initial_info(&session.adl, session.context, &adapter, idx);
            (
                DeviceState {
                    adapter_index: adapter.adapter_index,
                },
                info,
            )
        })
        .collect();

    Some(AdlBundle { session, entries })
}

/// Per-adapter metadata extracted from `AdapterInfo` and retained
/// for the device's lifetime. The raw `AdapterInfo` is not retained
/// because most of its fields (driver path, display name, OS index)
/// are unused.
struct AdapterMeta {
    adapter_index: i32,
    name: String,
    /// `AdapterInfo::str_udid` — ADL's per-device "Unique Device
    /// IDentifier", a stable string identifier intended to survive
    /// driver updates and reboots. Empty when the driver did not
    /// populate the field (very old drivers); the caller's
    /// stable-id fallback handles the empty case.
    udid: String,
}

/// Enumerate ADL adapters using `ctx`, deduplicating by PCI bus
/// number and skipping inactive entries.
fn enumerate_adapters(adl: &AdlFunctions, ctx: *mut c_void) -> Option<Vec<AdapterMeta>> {
    let mut num_adapters: i32 = 0;
    // SAFETY: ctx is valid; num_adapters is a valid pointer.
    let ret = unsafe { (adl.adapter_number_of_adapters_get)(ctx, &mut num_adapters) };
    if ret != ADL_OK || num_adapters <= 0 {
        return None;
    }

    let count = num_adapters as usize;
    let mut adapter_infos: Vec<AdapterInfo> = (0..count).map(|_| AdapterInfo::zeroed()).collect();
    let buf_size = (count * std::mem::size_of::<AdapterInfo>()) as i32;
    // SAFETY: adapter_infos is a valid buffer of count AdapterInfo structs.
    let ret = unsafe { (adl.adapter_adapter_info_get)(ctx, adapter_infos.as_mut_ptr(), buf_size) };
    if ret != ADL_OK {
        return None;
    }

    let mut out = Vec::new();
    let mut seen_bus = std::collections::HashSet::new();
    for info in &adapter_infos {
        let mut active: i32 = 0;
        // SAFETY: ctx is valid; adapter index from ADL; active is valid pointer.
        let ret = unsafe { (adl.adapter_active_get)(ctx, info.i_adapter_index, &mut active) };
        if ret != ADL_OK || active == 0 {
            continue;
        }
        if !seen_bus.insert(info.i_bus_number) {
            continue;
        }
        out.push(AdapterMeta {
            adapter_index: info.i_adapter_index,
            name: string_from_buf(&info.str_adapter_name),
            udid: string_from_buf(&info.str_udid),
        });
    }
    Some(out)
}

/// Pre-fill the per-device `GpuInfo` with static fields (stable id,
/// name, max power reference, total VRAM) using this device's own
/// context.
fn build_initial_info(
    adl: &AdlFunctions,
    ctx: *mut c_void,
    adapter: &AdapterMeta,
    vendor_relative_index: usize,
) -> crate::domain::gpu::GpuInfo {
    let stable_id = if adapter.udid.is_empty() {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::GpuAdl,
            adapter = adapter.adapter_index,
            "ADL UDID empty; using vendor-relative adapter index fallback",
        );
        format!("AMD:adapter:{vendor_relative_index}")
    } else {
        format!("AMD:{}", adapter.udid)
    };
    let mut gpu = crate::domain::gpu::GpuInfo {
        stable_id,
        name: adapter.name.clone(),
        ..crate::domain::gpu::GpuInfo::default()
    };

    // PMLog one-shot snapshot for the initial power-limit reference.
    let mut pmlog = AdlPMLogDataOutput::zeroed();
    // SAFETY: ctx and adapter.adapter_index are valid; pmlog is a
    // valid pointer.
    let ret = unsafe { (adl.query_pmlog_data_get)(ctx, adapter.adapter_index, &mut pmlog) };
    if ret == ADL_OK {
        let power_w = pmlog
            .get(SENSOR_BOARD_POWER)
            .or_else(|| pmlog.get(SENSOR_ASIC_POWER));
        if let Some(w) = power_w {
            // Sensor reports watts; domain stores milliwatts. Use
            // the initial reading as a baseline — the meter rescales
            // if power exceeds this on later samples.
            gpu.pwr_max_usage = w.max(1) as i64 * 1000;
        }
    }

    // Total VRAM.
    let mut mem = AdlMemoryInfoX4::zeroed();
    // SAFETY: ctx and adapter.adapter_index are valid; mem is a
    // valid pointer.
    let ret = unsafe { (adl.adapter_memory_info_x4_get)(ctx, adapter.adapter_index, &mut mem) };
    if ret == ADL_OK && mem.i_memory_size > 0 {
        gpu.mem_total = (mem.i_memory_size * 1024 * 1024) as u64;
    }

    gpu
}
