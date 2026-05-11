//! GPU data collection.
//!
//! Each detected GPU runs in its own collector thread (spawned by
//! [`crate::runner::CollectorManager`]) and publishes per-device
//! snapshots to its own [`crate::runner::LatestSlot`]. This module
//! owns the per-vendor discovery seam and the central
//! [`DeviceCollector`] enum that wraps the three vendor-specific
//! per-device collectors.
//!
//! # Vendor session/device split
//!
//! Each vendor SDK has a different documented thread-safety model;
//! the session/device split is shaped to match each vendor's actual
//! contract rather than forcing a uniform shape across the three:
//!
//! * **NVIDIA** — `NvAPI_Initialize` is process-global, refcounted,
//!   thread-safe per `nvapi.h`. The function-pointer table and the
//!   library handle are wrapped in a single shared [`Arc<NvApiSession>`]
//!   that drops via `NvAPI_Unload` when the last NVIDIA device thread
//!   releases its `Arc`.
//!
//! * **AMD** — ADL2 is multi-context. Per the GPUOpen documentation,
//!   *"do not share one context handle across threads unless you
//!   implement your own synchronisation"*. The function-pointer
//!   table is shared via [`Arc<AdlSession>`] (function pointers are
//!   read-only and trivially `Send + Sync`); each
//!   `AmdDeviceCollector` creates and owns **its own** ADL2 context
//!   (one `ADL2_Main_Control_Create` per device thread, destroyed
//!   via `ADL2_Main_Control_Destroy` on `Drop`).
//!
//! * **Intel** — IGCL's `ctlInit` is treated as a process-singleton
//!   by Intel's official wrapper (`hinstLib` is a static; the docs
//!   do not cover concurrent multi-init). The `api_handle` from
//!   `ctlInit` is wrapped in a shared [`Arc<IgclSession>`] that
//!   drops via `ctlClose` when the last Intel device thread
//!   releases its `Arc`.

mod amd;
mod intel;
mod nvidia;

use crate::collect::Collector;

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
    super::win::percent_u64(power_mw, max_power_mw).min(100)
}

/// Per-device GPU collector wrapping one of the three per-vendor
/// implementations.
///
/// Three vendors are a closed set, so an enum is the idiomatic
/// shape (codebase rule: enums for closed sets, no `Box<dyn Trait>`
/// for polymorphism unless dynamic dispatch is genuinely required).
/// `Collector::collect` and `Collector::snapshot` dispatch via a
/// single per-variant `match` — no virtual call, no allocation.
///
/// Constructed exclusively by the per-vendor `discover()`
/// functions invoked from [`discover`]. `CollectorManager` moves
/// each `DeviceCollector` into its own collector thread.
pub(crate) enum DeviceCollector {
    Nvidia(nvidia::NvidiaDeviceCollector),
    Amd(amd::AmdDeviceCollector),
    Intel(intel::IntelDeviceCollector),
}

impl Collector for DeviceCollector {
    type Snapshot = crate::runner::GpuSnapshot;

    fn collect(&mut self) {
        match self {
            Self::Nvidia(c) => c.collect(),
            Self::Amd(c) => c.collect(),
            Self::Intel(c) => c.collect(),
        }
    }

    fn snapshot(&self) -> Self::Snapshot {
        match self {
            Self::Nvidia(c) => c.snapshot(),
            Self::Amd(c) => c.snapshot(),
            Self::Intel(c) => c.snapshot(),
        }
    }
}

/// Discover every detected GPU across the three vendor SDKs.
///
/// Probes each vendor in canonical order (NVIDIA → AMD → Intel) and
/// concatenates the per-vendor device collectors into a single
/// vector. The order is preserved across rtop runs so
/// `custom_gpu_names[i]` and `gpu_update_ms[i]` continue to address
/// the same physical device they did before.
///
/// Discovery is synchronous: every vendor SDK's library is loaded,
/// every vendor init is called, and every device is enumerated
/// before the function returns. Vendor-init failures contribute
/// zero devices for that vendor (the library handle and function
/// table drop immediately) and never abort discovery for the other
/// vendors.
///
/// The returned vector's length is the discovered device count;
/// callers must not assume it equals [`crate::config::MAX_GPUS`].
pub(crate) fn discover() -> Vec<DeviceCollector> {
    let mut out = Vec::new();
    out.extend(nvidia::discover());
    out.extend(amd::discover());
    out.extend(intel::discover());
    if out.len() > crate::config::MAX_GPUS {
        // The toggle-key keybinds, per-GPU config arrays, and
        // per-GPU dirty bits are all sized by MAX_GPUS. A system
        // with more than MAX_GPUS detected devices truncates to
        // the first MAX_GPUS — anything beyond would have no
        // addressable widget, no toggle key, and no per-device
        // config entry.
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Gpu,
            detected = out.len(),
            cap = crate::config::MAX_GPUS,
            "device count exceeds MAX_GPUS; truncating",
        );
        out.truncate(crate::config::MAX_GPUS);
    }
    tracing::info!(
        subsystem = %crate::log::Subsystem::Gpu,
        devices = out.len(),
        "GPU discovery complete",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_does_not_panic() {
        // Discovery probes vendor DLLs that may or may not be
        // present on the build/test host. The function must
        // gracefully return an empty Vec on any test machine.
        let collectors = discover();
        assert!(collectors.len() <= crate::config::MAX_GPUS);
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
