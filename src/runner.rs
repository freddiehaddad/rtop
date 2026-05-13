use crate::collect::CollectStatus;
use crate::collect::Collector;
use crate::collect::cpu::CpuCollector;
use crate::collect::disk::DiskCollector;
use crate::collect::memory::MemCollector;
use crate::collect::network::NetCollector;
use crate::collect::process::ProcCollector;
use crate::collect::statusbar::{STATUSBAR_UPDATE_MS, StatusbarCollector};
use crate::config::RefreshConfig;
use crate::domain::{
    cpu::CpuInfo, disk::DiskData, gpu::GpuInfo, memory::MemInfo, network::NetInfo,
    process::ProcInfo,
};
use crate::event::{AppEvent, PerSubsystem, SubsystemKind};
use arc_swap::ArcSwapOption;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};
use std::thread::JoinHandle;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Per-subsystem snapshot types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct CpuSnapshot {
    pub(crate) info: CpuInfo,
    pub(crate) status: CollectStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct DiskSnapshot {
    pub(crate) info: DiskData,
    pub(crate) status: CollectStatus,
}

/// GPU snapshot containing every detected device's data, published
/// once per cycle by [`CollectorManager::gpu_slot`]. Mirrors
/// [`NetSnapshot`] — one snapshot, one Vec of devices, one status.
#[derive(Debug, Clone)]
pub(crate) struct GpuSnapshot {
    pub(crate) devices: Vec<GpuInfo>,
    pub(crate) status: CollectStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct MemSnapshot {
    pub(crate) info: MemInfo,
    pub(crate) status: CollectStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct NetSnapshot {
    pub(crate) nets: Vec<NetInfo>,
    pub(crate) status: CollectStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcSnapshot {
    pub(crate) procs: Vec<ProcInfo>,
    pub(crate) status: CollectStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusbarSnapshot {
    pub(crate) info: crate::collect::statusbar::StatusbarInfo,
}

// ---------------------------------------------------------------------------
// LatestSlot<T> — generic per-subsystem shared slot with coalescing
// ---------------------------------------------------------------------------

/// Thread-safe slot that always holds the latest value.
///
/// Publishers overwrite; consumers read the latest. Multiple publishes
/// between reads naturally coalesce — only the most recent value is
/// kept. Both operations are lock-free atomic swaps via
/// [`arc_swap::ArcSwapOption`].
pub(crate) struct LatestSlot<T> {
    inner: Arc<ArcSwapOption<T>>,
}

impl<T> Clone for LatestSlot<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> LatestSlot<T> {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ArcSwapOption::empty()),
        }
    }

    /// Store new data, replacing any previous value. Lock-free.
    pub(crate) fn publish(&self, data: T) {
        self.inner.store(Some(Arc::new(data)));
    }

    /// Read the latest value, if any. Lock-free.
    pub(crate) fn latest(&self) -> Option<Arc<T>> {
        self.inner.load_full()
    }
}

// ---------------------------------------------------------------------------
// Collector commands
// ---------------------------------------------------------------------------

/// Universal control commands accepted by every collector loop.
pub(crate) enum CollectorCommand {
    /// Change the collection interval.
    SetInterval(u64),
    /// Graceful shutdown.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Collector thread loop
// ---------------------------------------------------------------------------

/// Run a collector in a loop: collect → publish → sleep → repeat.
///
/// Used for every per-widget collector (CPU, memory, disk, network,
/// GPU, process) and the statusbar. The snapshot is built via the
/// collector's own [`Collector::snapshot`] — no `snapshot_fn` closure
/// is threaded through, since the snapshot type is bound at the trait
/// level (`LatestSlot<C::Snapshot>`).
fn run_collector_loop<C>(
    mut collector: C,
    initial_interval_ms: u64,
    slot: LatestSlot<C::Snapshot>,
    event_tx: Sender<AppEvent>,
    wakeup: AppEvent,
    cmd_rx: Receiver<CollectorCommand>,
) where
    C: Collector,
{
    let mut interval_ms = initial_interval_ms.max(100);
    loop {
        collector.collect();
        slot.publish(collector.snapshot());
        let _ = event_tx.send(wakeup);

        match cmd_rx.recv_timeout(Duration::from_millis(interval_ms)) {
            Ok(CollectorCommand::SetInterval(ms)) => interval_ms = ms.max(100),
            Ok(CollectorCommand::Shutdown) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Collector thread spawning helper
// ---------------------------------------------------------------------------

/// Spawn a collector thread with standard channel + slot wiring.
///
/// Creates the command channel, clones the slot and event sender,
/// spawns the thread, and returns the command sender and join handle.
/// The collector is constructed inside the thread via `collector_fn`
/// so types that are not `Send` (e.g. GPU backends) work correctly.
/// The snapshot is produced by the collector's
/// [`Collector::snapshot`] inside the loop — no closure parameter
/// is required.
fn spawn_collector<C>(
    collector_fn: impl FnOnce() -> C + Send + 'static,
    update_ms: u64,
    slot: &LatestSlot<C::Snapshot>,
    event_tx: &Sender<AppEvent>,
    wakeup: AppEvent,
) -> (Sender<CollectorCommand>, JoinHandle<()>)
where
    C: Collector,
{
    let (tx, rx) = mpsc::channel();
    let slot = slot.clone();
    let event_tx = event_tx.clone();
    let handle = std::thread::spawn(move || {
        run_collector_loop(collector_fn(), update_ms, slot, event_tx, wakeup, rx);
    });
    (tx, handle)
}

// ---------------------------------------------------------------------------
// CollectorManager — owns all collector threads
// ---------------------------------------------------------------------------

/// Manages per-collector threads with independent timers.
///
/// Each collector runs on its own thread with a `LatestSlot<T>` for
/// coalescing and publishes wakeup events through the shared channel.
/// All collectors share the same [`CollectorCommand`] wire format —
/// variants that don't apply to a given subsystem are silently
/// dropped by that collector's loop.
pub(crate) struct CollectorManager {
    /// Command sender per subsystem.
    txs: PerSubsystem<Sender<CollectorCommand>>,

    pub(crate) cpu_slot: LatestSlot<CpuSnapshot>,
    pub(crate) mem_slot: LatestSlot<MemSnapshot>,
    pub(crate) disk_slot: LatestSlot<DiskSnapshot>,
    pub(crate) net_slot: LatestSlot<NetSnapshot>,
    pub(crate) gpu_slot: LatestSlot<GpuSnapshot>,
    pub(crate) proc_slot: LatestSlot<ProcSnapshot>,
    pub(crate) statusbar_slot: LatestSlot<StatusbarSnapshot>,

    joins: Vec<(SubsystemKind, JoinHandle<()>)>,
}

/// Send `cmd` to the named subsystem's collector thread, logging a
/// warning on send failure. Centralised so every send goes through one
/// audit point.
fn send_command(
    tx: &Sender<CollectorCommand>,
    target: SubsystemKind,
    op: &'static str,
    cmd: CollectorCommand,
) {
    if let Err(e) = tx.send(cmd) {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Runner,
            target = %target,
            op,
            error = %e,
            "command send failed",
        );
    }
}

/// Resolve every collector subsystem's effective interval from
/// `refresh` and yield one `(SubsystemKind, u64)` per subsystem
/// in `SubsystemKind::iter`-canonical order, except `Statusbar`
/// which has a hardcoded cadence
/// ([`crate::collect::statusbar::STATUSBAR_UPDATE_MS`]) and is
/// not user-configurable.
///
/// This is the shared driver behind both startup spawning (where
/// each subsystem's spawn site reads its own field directly for
/// readability) and `apply_refresh` (which iterates here to
/// broadcast every resolved value uniformly).
fn resolved_intervals(refresh: &RefreshConfig) -> impl Iterator<Item = (SubsystemKind, u64)> {
    [
        (SubsystemKind::Cpu, refresh.effective(refresh.cpu_update_ms)),
        (SubsystemKind::Mem, refresh.effective(refresh.mem_update_ms)),
        (
            SubsystemKind::Disk,
            refresh.effective(refresh.disk_update_ms),
        ),
        (SubsystemKind::Net, refresh.effective(refresh.net_update_ms)),
        (SubsystemKind::Gpu, refresh.effective(refresh.gpu_update_ms)),
        (
            SubsystemKind::Proc,
            refresh.effective(refresh.proc_update_ms),
        ),
    ]
    .into_iter()
}

impl CollectorManager {
    /// Start all collector threads with intervals resolved from
    /// `refresh`.
    ///
    /// `CollectorManager` is the **single resolver** of the
    /// "0 = inherit global" rule (see [`RefreshConfig::effective`]):
    /// every per-widget interval is resolved here. The same
    /// resolution rule is reapplied at runtime via
    /// [`Self::apply_refresh`], so startup and every
    /// refresh-related action go through one code path.
    pub(crate) fn start(refresh: &RefreshConfig, event_tx: Sender<AppEvent>) -> Self {
        let core_count = crate::collect::cpu::get_core_count();

        let cpu_ms = refresh.effective(refresh.cpu_update_ms);
        let mem_ms = refresh.effective(refresh.mem_update_ms);
        let disk_ms = refresh.effective(refresh.disk_update_ms);
        let net_ms = refresh.effective(refresh.net_update_ms);
        let gpu_ms = refresh.effective(refresh.gpu_update_ms);
        let proc_ms = refresh.effective(refresh.proc_update_ms);

        let cpu_slot = LatestSlot::new();
        let mem_slot = LatestSlot::new();
        let disk_slot = LatestSlot::new();
        let net_slot = LatestSlot::new();
        let gpu_slot = LatestSlot::new();
        let proc_slot = LatestSlot::new();
        let statusbar_slot = LatestSlot::new();

        // CPU thread
        let (cpu_tx, cpu_join) = spawn_collector(
            || {
                let mut cpu = CpuCollector::new();
                cpu.init();
                cpu
            },
            cpu_ms,
            &cpu_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Cpu),
        );

        // Memory thread
        let (mem_tx, mem_join) = spawn_collector(
            MemCollector::new,
            mem_ms,
            &mem_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Mem),
        );

        // Disk thread
        let (disk_tx, disk_join) = spawn_collector(
            DiskCollector::new,
            disk_ms,
            &disk_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Disk),
        );

        // Network thread
        let (net_tx, net_join) = spawn_collector(
            NetCollector::new,
            net_ms,
            &net_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Net),
        );

        // GPU thread — single aggregator that owns every detected
        // device's vendor session and per-device state. Discovery
        // (vendor DLL loads, vendor inits, device enumeration) runs
        // inside the spawned thread via `GpuCollector::new`,
        // matching the pattern used by every other subsystem.
        let (gpu_tx, gpu_join) = spawn_collector(
            crate::collect::gpu::GpuCollector::new,
            gpu_ms,
            &gpu_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Gpu),
        );

        // Process thread
        let (proc_tx, proc_join) = spawn_collector(
            move || {
                let mut proc_collector = ProcCollector::new();
                proc_collector.set_core_count(core_count);
                proc_collector
            },
            proc_ms,
            &proc_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Proc),
        );

        // Statusbar thread — fixed 1 Hz cadence (the wall-clock seconds
        // digit advances at human-noticeable cadence and uptime stays
        // in sync). Cadence is hardcoded in the collector module; if a
        // future change wants user-configurable cadence, replace
        // `STATUSBAR_UPDATE_MS` with a `RefreshConfig` lookup here.
        //
        // The snapshot omits a `status` field because
        // `GetTickCount64` is infallible — there is no degraded
        // mode to surface. Mirroring the other subsystems'
        // `info`/`status` shape would only add a permanently-`Ok`
        // field that no caller could meaningfully consult.
        let (statusbar_tx, statusbar_join) = spawn_collector(
            StatusbarCollector::new,
            STATUSBAR_UPDATE_MS,
            &statusbar_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Statusbar),
        );

        // Build the joins table in canonical-subsystem order so
        // shutdown joins in a stable sequence. The SubsystemKind
        // tag drives the diagnostic log target via `as_str()`.
        let joins: Vec<(SubsystemKind, JoinHandle<()>)> = vec![
            (SubsystemKind::Cpu, cpu_join),
            (SubsystemKind::Mem, mem_join),
            (SubsystemKind::Disk, disk_join),
            (SubsystemKind::Net, net_join),
            (SubsystemKind::Gpu, gpu_join),
            (SubsystemKind::Proc, proc_join),
            (SubsystemKind::Statusbar, statusbar_join),
        ];

        Self {
            txs: PerSubsystem::new(
                cpu_tx,
                mem_tx,
                disk_tx,
                net_tx,
                gpu_tx,
                proc_tx,
                statusbar_tx,
            ),
            cpu_slot,
            mem_slot,
            disk_slot,
            net_slot,
            gpu_slot,
            proc_slot,
            statusbar_slot,
            joins,
        }
    }

    /// Re-resolve every per-widget interval from `refresh` and
    /// broadcast `SetInterval` to every collector.
    ///
    /// This is the **only** runtime entry point for changing
    /// collector intervals. Every refresh-related user action
    /// (preset cycle, options-menu commit, `+`/`-` step, config
    /// reload) calls this with the freshly-mutated `&RefreshConfig`,
    /// guaranteeing every subsystem ends up at exactly
    /// `refresh.effective(refresh.<widget>_update_ms)`.
    ///
    /// Sending the same value as the previous call is intentionally
    /// harmless: the worker loop's next `recv_timeout` returns
    /// `Ok(SetInterval(ms))`, the loop assigns the same `interval_ms`,
    /// and behavior is unchanged. This keeps the broadcast loop free
    /// of per-subsystem change tracking.
    ///
    /// The statusbar is intentionally excluded — its cadence is
    /// hardcoded at the collector module (see
    /// [`crate::collect::statusbar::STATUSBAR_UPDATE_MS`]) and
    /// `RefreshConfig` carries no field for it.
    pub(crate) fn apply_refresh(&self, refresh: &RefreshConfig) {
        for (kind, ms) in resolved_intervals(refresh) {
            send_command(
                self.txs.get(kind),
                kind,
                "apply_refresh",
                CollectorCommand::SetInterval(ms),
            );
        }
    }

    /// Shut down all collector threads and wait for them to finish.
    pub(crate) fn shutdown(&mut self) {
        for kind in SubsystemKind::iter() {
            // Shutdown send errors are intentionally discarded:
            // by this point the collector thread may have already
            // exited (e.g. on a panic), and the join below will
            // surface any real failure.
            let _ = self.txs.get(kind).send(CollectorCommand::Shutdown);
        }
        for (target, join) in self.joins.drain(..) {
            if let Err(panic) = join.join() {
                let payload = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("(non-string panic payload)");
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Runner,
                    target = %target,
                    payload,
                    "collector thread panicked",
                );
            }
        }
    }
}

impl Drop for CollectorManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_slot_publishes_and_reads() {
        let slot = LatestSlot::new();
        assert!(slot.latest().is_none());

        slot.publish(42_u32);
        let val = slot.latest().expect("should have value");
        assert_eq!(*val, 42);
    }

    #[test]
    fn latest_slot_overwrites_previous() {
        let slot = LatestSlot::new();
        slot.publish(1_u32);
        slot.publish(2_u32);
        assert_eq!(*slot.latest().unwrap(), 2);
    }

    #[test]
    fn latest_slot_clone_shares_state() {
        let slot = LatestSlot::new();
        let clone = slot.clone();
        slot.publish(99_u32);
        assert_eq!(*clone.latest().unwrap(), 99);
    }

    #[test]
    fn cpu_snapshot_clones_data() {
        let snap = CpuSnapshot {
            info: CpuInfo::default(),
            status: CollectStatus::Ok,
        };
        assert_eq!(snap.info.core_count, 0);
        assert_eq!(snap.status, CollectStatus::Ok);
    }

    // ---------------------------------------------------------------
    // resolved_intervals — the iterator that drives apply_refresh.
    // The rule itself (RefreshConfig::effective) is exhaustively
    // tested in src/config/tests.rs; the tests below lock the
    // shape: which subsystems are covered, in what order, and with
    // what resolved value per kind.
    // ---------------------------------------------------------------

    fn refresh_with(
        global: i64,
        cpu: i64,
        mem: i64,
        disk: i64,
        net: i64,
        gpu: i64,
        proc_ms: i64,
    ) -> RefreshConfig {
        RefreshConfig {
            update_ms: global,
            cpu_update_ms: cpu,
            mem_update_ms: mem,
            disk_update_ms: disk,
            net_update_ms: net,
            gpu_update_ms: gpu,
            proc_update_ms: proc_ms,
        }
    }

    #[test]
    fn resolved_intervals_yields_one_entry_per_user_configurable_subsystem() {
        let r = refresh_with(2000, 0, 0, 0, 0, 0, 0);
        let pairs: Vec<_> = resolved_intervals(&r).collect();
        assert_eq!(
            pairs,
            vec![
                (SubsystemKind::Cpu, 2000),
                (SubsystemKind::Mem, 2000),
                (SubsystemKind::Disk, 2000),
                (SubsystemKind::Net, 2000),
                (SubsystemKind::Gpu, 2000),
                (SubsystemKind::Proc, 2000),
            ],
        );
        assert!(
            !pairs
                .iter()
                .any(|(k, _)| matches!(k, SubsystemKind::Statusbar)),
            "Statusbar must never appear — its cadence is hardcoded \
             via STATUSBAR_UPDATE_MS",
        );
    }

    #[test]
    fn resolved_intervals_mix_of_overrides_and_inheritance() {
        // CPU overridden, Mem inherits, Disk overridden, Net inherits,
        // GPU overridden, Proc inherits. The contract is that each
        // subsystem independently applies effective(widget_ms).
        let r = refresh_with(2000, 250, 0, 500, 0, 1000, 0);
        let by_kind: std::collections::HashMap<_, _> = resolved_intervals(&r).collect();
        assert_eq!(by_kind[&SubsystemKind::Cpu], 250);
        assert_eq!(by_kind[&SubsystemKind::Mem], 2000);
        assert_eq!(by_kind[&SubsystemKind::Disk], 500);
        assert_eq!(by_kind[&SubsystemKind::Net], 2000);
        assert_eq!(by_kind[&SubsystemKind::Gpu], 1000);
        assert_eq!(by_kind[&SubsystemKind::Proc], 2000);
    }

    #[test]
    fn resolved_intervals_canonical_order_matches_subsystem_kind() {
        // Iteration order is contract: Cpu, Mem, Disk, Net, Gpu,
        // Proc. This mirrors `SubsystemKind::iter` minus Statusbar.
        let r = refresh_with(1000, 0, 0, 0, 0, 0, 0);
        let kinds: Vec<SubsystemKind> = resolved_intervals(&r).map(|(k, _)| k).collect();
        let expected: Vec<SubsystemKind> = SubsystemKind::iter()
            .filter(|k| !matches!(k, SubsystemKind::Statusbar))
            .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn resolved_intervals_applies_clamps_per_subsystem() {
        // Each subsystem hits the rule independently — under-100
        // overrides are floored, over-ceiling are capped.
        let r = refresh_with(2000, 50, 86_400_001, 0, 99, 200, -1);
        let by_kind: std::collections::HashMap<_, _> = resolved_intervals(&r).collect();
        assert_eq!(by_kind[&SubsystemKind::Cpu], 100, "50 → floored to 100");
        assert_eq!(
            by_kind[&SubsystemKind::Mem],
            86_400_000,
            "86_400_001 → capped at 86_400_000",
        );
        assert_eq!(by_kind[&SubsystemKind::Disk], 2000, "0 → inherits global",);
        assert_eq!(by_kind[&SubsystemKind::Net], 100, "99 → floored to 100");
        assert_eq!(by_kind[&SubsystemKind::Gpu], 200);
        assert_eq!(
            by_kind[&SubsystemKind::Proc],
            2000,
            "-1 → inherits global (defensive)",
        );
    }
}
