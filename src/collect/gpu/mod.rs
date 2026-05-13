//! GPU data collection.
//!
//! A single [`GpuCollector`] aggregates every detected device
//! across the three vendor SDKs (NVIDIA, AMD, Intel) and runs in
//! one collector thread spawned by
//! [`crate::runner::CollectorManager`]. Each collect cycle queries
//! every device sequentially and publishes a single
//! [`crate::runner::GpuSnapshot`] containing every device's data —
//! mirroring the [`crate::collect::network::NetCollector`] shape.
//!
//! # Vendor session model
//!
//! Each vendor exposes a session type ([`nvidia::NvApiSession`],
//! [`amd::AdlSession`], [`intel::IgclSession`]) that owns the loaded
//! library, the resolved function-pointer table, and the vendor
//! init refcount / context. The session is owned **once** by
//! [`GpuCollector`] when at least one device of that vendor was
//! detected; per-device state lives in slim
//! `nvidia/amd/intel::DeviceState` structs that carry only the
//! genuinely per-device fields (handles, prev counters).
//!
//! # Drop ordering
//!
//! [`GpuCollector`] declares `devices` first so it drops first. The
//! per-device handles inside `devices` are vendor-opaque pointers
//! whose validity expires when the vendor session tears down
//! (`NvAPI_Unload`, `ADL2_Main_Control_Destroy`, `ctlClose`); the
//! field declaration order ensures every handle is released before
//! its parent session.

mod amd;
mod intel;
mod nvidia;

use crate::collect::{CollectStatus, Collector};

const MAX_HISTORY: usize = 300;

/// Push a value onto a history deque, capping at `MAX_HISTORY`.
fn push_history(history: &mut std::collections::VecDeque<i64>, value: i64) {
    history.push_back(value);
    if history.len() > MAX_HISTORY {
        history.pop_front();
    }
}

/// Clamp a percentage value to [0, 100].
fn clamp_percent(value: u32) -> i64 {
    value.min(100) as i64
}

/// Calculate power percentage from current draw and max limit (both in mW).
fn power_percent(power_mw: u64, max_power_mw: u64) -> i64 {
    if max_power_mw == 0 {
        return 0;
    }
    super::counters::percent_u64(power_mw, max_power_mw).min(100)
}

/// Per-device state, vendor-discriminated. One per detected device,
/// held inside [`GpuCollector`]'s `Vec<DeviceEntry>`.
///
/// The variant payload is the slim per-vendor `DeviceState` that
/// carries only the genuinely per-device fields. Vendor sessions
/// (function pointers, library handles, init refcounts, AMD's
/// shared context, IGCL's `api_handle`) live exactly once each in
/// the parent [`GpuCollector`]; per-cycle collection borrows them
/// by reference.
enum GpuDevice {
    Nvidia(nvidia::DeviceState),
    Amd(amd::DeviceState),
    Intel(intel::DeviceState),
}

/// A single device entry: vendor-specific state, the device's
/// rendered `GpuInfo` (mutated in place by per-cycle collection),
/// and the device's status for the most recent cycle.
struct DeviceEntry {
    device: GpuDevice,
    info: crate::domain::gpu::GpuInfo,
    status: CollectStatus,
}

/// Single GPU collector aggregating every detected device.
///
/// Implements [`Collector`] like every other subsystem; spawned
/// once via [`crate::runner::CollectorManager::start`] using the
/// shared `spawn_collector` helper. One collect cycle queries every
/// device sequentially in vendor → discovery order; one snapshot
/// per cycle contains every device's data.
pub(crate) struct GpuCollector {
    /// Per-device entries in vendor → discovery order. Declared
    /// first so this field drops first: every per-device handle
    /// inside the entries releases before the vendor session
    /// fields below tear down the loader / init refcount /
    /// shared context.
    devices: Vec<DeviceEntry>,

    /// Vendor sessions. `Some(_)` when at least one device of that
    /// vendor was discovered; `None` when the vendor's library was
    /// absent or returned no devices (in which case discovery
    /// dropped the session immediately rather than retaining an
    /// idle library handle).
    nvapi: Option<nvidia::NvApiSession>,
    adl: Option<amd::AdlSession>,
    igcl: Option<intel::IgclSession>,

    /// Worst per-device status this cycle, reset to `Ok` at the
    /// start of every `collect()` call.
    status: CollectStatus,
}

impl GpuCollector {
    /// Discover every detected GPU across the three vendor SDKs.
    ///
    /// Probes each vendor in canonical NVIDIA → AMD → Intel order
    /// and concatenates the per-vendor device entries. Discovery
    /// order is stable for any configuration that does not
    /// physically rearrange devices, so per-device identities
    /// (the `info.stable_id` strings the renderer matches against
    /// `view.gpu_iface`) survive across rtop runs.
    ///
    /// Vendor probes that find no devices return `None` and drop
    /// any library handle they loaded. Probes never abort discovery
    /// for the other vendors.
    ///
    /// There is no architectural cap on detected device count;
    /// hardware realities (PCIe slots, vendor SDK enumeration
    /// limits) bound it well below `usize::MAX`.
    pub(crate) fn new() -> Self {
        let mut devices: Vec<DeviceEntry> = Vec::new();

        let nvapi = nvidia::discover().map(|bundle| {
            devices.extend(bundle.entries.into_iter().map(|(d, info)| DeviceEntry {
                device: GpuDevice::Nvidia(d),
                info,
                status: CollectStatus::Ok,
            }));
            bundle.session
        });
        let adl = amd::discover().map(|bundle| {
            devices.extend(bundle.entries.into_iter().map(|(d, info)| DeviceEntry {
                device: GpuDevice::Amd(d),
                info,
                status: CollectStatus::Ok,
            }));
            bundle.session
        });
        let igcl = intel::discover().map(|bundle| {
            devices.extend(bundle.entries.into_iter().map(|(d, info)| DeviceEntry {
                device: GpuDevice::Intel(d),
                info,
                status: CollectStatus::Ok,
            }));
            bundle.session
        });

        tracing::info!(
            subsystem = %crate::log::Subsystem::Gpu,
            devices = devices.len(),
            "GPU discovery complete",
        );

        Self {
            devices,
            nvapi,
            adl,
            igcl,
            status: CollectStatus::Ok,
        }
    }
}

impl Default for GpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for GpuCollector {
    type Snapshot = crate::runner::GpuSnapshot;

    fn collect(&mut self) {
        self.status = CollectStatus::Ok;
        for entry in &mut self.devices {
            entry.status = CollectStatus::Ok;
            match &mut entry.device {
                GpuDevice::Nvidia(d) => nvidia::collect(
                    self.nvapi.as_ref().expect(
                        "GpuDevice::Nvidia entry implies nvapi.is_some() — both are populated \
                         together by GpuCollector::new",
                    ),
                    d,
                    &mut entry.info,
                    &mut entry.status,
                ),
                GpuDevice::Amd(d) => amd::collect(
                    self.adl.as_ref().expect(
                        "GpuDevice::Amd entry implies adl.is_some() — both are populated \
                         together by GpuCollector::new",
                    ),
                    d,
                    &mut entry.info,
                    &mut entry.status,
                ),
                GpuDevice::Intel(d) => intel::collect(
                    self.igcl.as_ref().expect(
                        "GpuDevice::Intel entry implies igcl.is_some() — both are populated \
                         together by GpuCollector::new",
                    ),
                    d,
                    &mut entry.info,
                    &mut entry.status,
                ),
            }
            self.status.downgrade(entry.status.clone());
        }
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::GpuSnapshot {
            devices: self.devices.iter().map(|e| e.info.clone()).collect(),
            status: self.status.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_collector_new_does_not_panic() {
        // Discovery probes vendor DLLs that may or may not be
        // present on the build/test host. The constructor must
        // gracefully return whatever it finds (often a collector
        // with zero devices on machines without supported GPUs)
        // on any test machine.
        let _ = GpuCollector::new();
    }

    #[test]
    fn gpu_collector_collect_with_no_devices_publishes_empty_snapshot() {
        // On hosts without any vendor DLL, GpuCollector::new()
        // populates an empty `devices` vec. `collect()` must be a
        // no-op and `snapshot()` must return an empty `devices`
        // Vec with `Ok` status. (On hosts WITH a vendor present,
        // this test still passes — we only assert the empty case
        // when devices is empty.)
        let mut c = GpuCollector::new();
        if c.devices.is_empty() {
            c.collect();
            let snap = c.snapshot();
            assert!(snap.devices.is_empty());
            assert_eq!(snap.status, CollectStatus::Ok);
        }
    }

    #[test]
    fn clamp_percent_caps_at_100() {
        assert_eq!(clamp_percent(42), 42);
        assert_eq!(clamp_percent(150), 100);
    }

    #[test]
    fn power_percent_zero_on_zero_limit() {
        assert_eq!(power_percent(50, 0), 0);
    }

    #[test]
    fn power_percent_calculates_correctly() {
        assert_eq!(power_percent(50, 100), 50);
        assert_eq!(power_percent(200, 100), 100);
    }

    #[test]
    fn push_history_caps_at_max() {
        let mut history = std::collections::VecDeque::new();
        for i in 0..MAX_HISTORY + 10 {
            push_history(&mut history, i as i64);
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(*history.front().unwrap(), 10);
    }
}
