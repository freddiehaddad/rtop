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
// AMD vendor session and per-device collector
// ---------------------------------------------------------------------------

/// Extract a C string from a fixed-size byte buffer (ADL adapter
/// name fields use this shape).
fn string_from_buf(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

/// Owned ADL2 context. RAII: drops via `ADL2_Main_Control_Destroy`.
/// Holds an `Arc<AdlSession>` so the destroy function pointer
/// stays reachable until the context is freed.
///
/// Each `AmdDeviceCollector` owns its own `AdlContext` (per ADL2's
/// "do not share contexts across threads" guidance). The
/// enumeration context built in [`discover`] is also an
/// `AdlContext` and is freed automatically when it goes out of
/// scope. The pointer is opaque and never dereferenced from Rust.
struct AdlContext {
    session: std::sync::Arc<AdlSession>,
    raw: *mut c_void,
}

// SAFETY: ADL2 contexts are owned exclusively by the per-device
// collector that created them; ADL2 itself synchronises calls
// against a single context. The pointer is opaque and never
// dereferenced from Rust.
unsafe impl Send for AdlContext {}

impl Drop for AdlContext {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: raw was created by ADL2_Main_Control_Create
            // on the session we still hold an Arc to;
            // main_control_destroy was resolved from the same
            // library and is called exactly once per context.
            unsafe {
                let _ = (self.session.adl.main_control_destroy)(self.raw);
            }
            self.raw = std::ptr::null_mut();
        }
    }
}

/// Shared AMD vendor session: loaded library + resolved
/// function-pointer table only. There is no shared ADL2 context —
/// each `AmdDeviceCollector` creates its own — so the session has
/// no `Drop` work beyond the `OwnedLibrary` field's automatic
/// cleanup.
pub(super) struct AdlSession {
    adl: AdlFunctions,
}

impl AdlSession {
    fn load() -> Option<Self> {
        let adl = AdlFunctions::load()?;
        Some(Self { adl })
    }

    /// Open a fresh ADL2 context bound to this session. Returns
    /// `None` if `ADL2_Main_Control_Create` fails. The caller
    /// owns the context; dropping it releases the underlying ADL2
    /// resource via `Drop for AdlContext`.
    fn open_context(self: &std::sync::Arc<Self>) -> Option<AdlContext> {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: adl.main_control_create was loaded from atiadlxx.dll;
        // adl_malloc is a valid C-ABI callback; 1 = enumerate connected
        // adapters only.
        let ret = unsafe { (self.adl.main_control_create)(adl_malloc, 1, &mut raw) };
        if ret != ADL_OK || raw.is_null() {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuAdl,
                code = %crate::log::Hex(ret),
                "ADL2_Main_Control_Create failed",
            );
            return None;
        }
        Some(AdlContext {
            session: std::sync::Arc::clone(self),
            raw,
        })
    }
}

/// Per-device AMD GPU collector. Owns its `AdlContext` (which in
/// turn carries an `Arc<AdlSession>`); destroying the context is
/// automatic via `Drop for AdlContext` when the collector drops.
pub(crate) struct AmdDeviceCollector {
    context: AdlContext,
    adapter_index: i32,
    info: crate::domain::gpu::GpuInfo,
    status: CollectStatus,
}

impl AmdDeviceCollector {
    pub(super) fn collect(&mut self) {
        self.status = CollectStatus::Ok;
        let adl = &self.context.session.adl;
        let ctx = self.context.raw;
        let idx = self.adapter_index;

        // PMLog one-shot query — provides utilization, clocks, temp, power.
        let mut pmlog = AdlPMLogDataOutput::zeroed();
        // SAFETY: ctx is a valid context owned by this collector;
        // idx is the adapter index recorded during discovery; pmlog
        // is a valid pointer to a zeroed output struct.
        let ret = unsafe { (adl.query_pmlog_data_get)(ctx, idx, &mut pmlog) };
        if ret == ADL_OK {
            // Utilization (direct %).
            if let Some(pct) = pmlog.get(SENSOR_ACTIVITY_GFX) {
                push_history(
                    &mut self.info.gpu_percent.utilization,
                    clamp_percent(pct.max(0) as u32),
                );
            }

            // Clock speed (direct MHz).
            if let Some(mhz) = pmlog.get(SENSOR_CLK_GFXCLK) {
                self.info.gpu_clock_speed = mhz.max(0) as u32;
            }

            // Temperature (direct °C).
            if let Some(temp) = pmlog.get(SENSOR_TEMPERATURE_EDGE) {
                push_history(&mut self.info.temp, temp as i64);
            }

            // Power — prefer board power (RDNA3+), fall back to ASIC power.
            // Sensor values are in watts; domain stores milliwatts.
            let power_w = pmlog
                .get(SENSOR_BOARD_POWER)
                .or_else(|| pmlog.get(SENSOR_ASIC_POWER));
            if let Some(w) = power_w {
                let power_mw = w.max(0) as u64 * 1000;
                self.info.pwr_usage = power_mw as i64;
                let pwr_pct = power_percent(power_mw, self.info.pwr_max_usage as u64);
                push_history(&mut self.info.gpu_percent.power, pwr_pct);
            }
        } else {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuAdl,
                code = %crate::log::Hex(ret),
                "ADL PMLog query failed",
            );
            self.status.downgrade(CollectStatus::Degraded("adl pmlog"));
        }

        // VRAM usage (direct MB from ADL).
        let mut vram_used_mb: i32 = 0;
        // SAFETY: ctx and idx are valid; vram_used_mb is a valid pointer.
        let ret = unsafe { (adl.dedicated_vram_usage_get)(ctx, idx, &mut vram_used_mb) };
        if ret == ADL_OK && vram_used_mb > 0 {
            self.info.mem_used = vram_used_mb as u64 * 1024 * 1024;
            let vram_pct = percent_u64(self.info.mem_used, self.info.mem_total).min(100);
            push_history(&mut self.info.gpu_percent.vram, vram_pct);
            push_history(&mut self.info.mem_utilization_percent, vram_pct);
        } else if ret != ADL_OK {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::GpuAdl,
                code = %crate::log::Hex(ret),
                "ADL VRAM usage query failed",
            );
            self.status.downgrade(CollectStatus::Degraded("adl vram"));
        }
    }

    pub(super) fn snapshot(&self) -> crate::runner::GpuSnapshot {
        crate::runner::GpuSnapshot {
            info: self.info.clone(),
            status: self.status.clone(),
        }
    }
}

/// Discover every AMD GPU and return one [`super::DeviceCollector`]
/// per detected adapter.
pub(super) fn discover() -> Vec<super::DeviceCollector> {
    let Some(session) = AdlSession::load() else {
        return Vec::new();
    };
    let session = std::sync::Arc::new(session);

    // Enumerate adapters using a temporary context. The context
    // drops at the end of this scope via Drop for AdlContext —
    // per-device collectors open their own contexts in the loop
    // below per ADL2's "do not share contexts across threads"
    // guidance (see vendor docs at the top of collect/gpu/mod.rs).
    let adapters = {
        let Some(enum_ctx) = session.open_context() else {
            return Vec::new();
        };
        match enumerate_adapters(&session.adl, enum_ctx.raw) {
            Some(adapters) => adapters,
            None => return Vec::new(),
        }
    };

    if adapters.is_empty() {
        return Vec::new();
    }

    tracing::info!(
        subsystem = %crate::log::Subsystem::GpuAdl,
        devices = adapters.len(),
        "vendor initialized",
    );

    adapters
        .into_iter()
        .filter_map(|adapter| {
            let device_ctx = session.open_context()?;
            // Pre-fill static fields (name, max power, total VRAM)
            // using this device's own context. The same context is
            // then retained by the collector for all subsequent
            // per-cycle queries from the device thread.
            let info = build_initial_info(&session.adl, device_ctx.raw, &adapter);
            Some(super::DeviceCollector::Amd(AmdDeviceCollector {
                context: device_ctx,
                adapter_index: adapter.adapter_index,
                info,
                status: CollectStatus::Ok,
            }))
        })
        .collect()
}

/// Adapter metadata captured during enumeration. Holds only what
/// the per-device collector needs for its own context init; the
/// raw `AdapterInfo` is not retained because most of its fields
/// (driver path, display name, OS index) are unused.
struct AdapterMeta {
    adapter_index: i32,
    name: String,
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
        });
    }
    Some(out)
}

/// Pre-fill the per-device `GpuInfo` with static fields (name, max
/// power reference, total VRAM) using this device's own context.
fn build_initial_info(
    adl: &AdlFunctions,
    ctx: *mut c_void,
    adapter: &AdapterMeta,
) -> crate::domain::gpu::GpuInfo {
    let mut gpu = crate::domain::gpu::GpuInfo {
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
