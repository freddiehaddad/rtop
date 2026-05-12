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

/// Per-device GPU snapshot. One per detected GPU; published by
/// the per-device collector thread to its own
/// [`CollectorManager::gpu_slots`] slot.
#[derive(Debug, Clone)]
pub(crate) struct GpuSnapshot {
    pub(crate) info: GpuInfo,
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

/// Commands sent to a collector thread.
///
/// Every collector accepts the same command type. Variants that don't
/// apply to a given collector are silently ignored by that collector's
/// loop — see the per-loop match arms in [`run_collector_loop`] (the
/// generic loop) and [`run_net_loop`] (the network loop, which handles
/// the additional [`CollectorCommand::ResetNetTotals`] variant).
pub(crate) enum CollectorCommand {
    /// Change the collection interval.
    SetInterval(u64),
    /// Graceful shutdown.
    Shutdown,
    /// Reset cumulative network totals for an interface. Only the
    /// network collector acts on this; other collectors ignore it.
    ResetNetTotals(String),
}

// ---------------------------------------------------------------------------
// Generic collector thread loop
// ---------------------------------------------------------------------------

/// Run a collector in a loop: collect → publish → sleep → repeat.
///
/// Used for CPU, memory, disk, GPU, and process collectors. The
/// snapshot is built via the collector's own
/// [`Collector::snapshot`] — no `snapshot_fn` closure is threaded
/// through, since the snapshot type is bound at the trait level
/// (`LatestSlot<C::Snapshot>`).
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
            // Not applicable — only the network collector acts on
            // ResetNetTotals; every other collector silently drops it.
            Ok(CollectorCommand::ResetNetTotals(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Run the network collector with support for `ResetNetTotals` commands.
fn run_net_loop(
    mut collector: NetCollector,
    initial_interval_ms: u64,
    slot: LatestSlot<NetSnapshot>,
    event_tx: Sender<AppEvent>,
    cmd_rx: Receiver<CollectorCommand>,
) {
    let mut interval_ms = initial_interval_ms.max(100);
    let publish = |c: &NetCollector| {
        slot.publish(c.snapshot());
        let _ = event_tx.send(AppEvent::SubsystemReady(SubsystemKind::Net));
    };

    loop {
        collector.collect();
        publish(&collector);

        match cmd_rx.recv_timeout(Duration::from_millis(interval_ms)) {
            Ok(CollectorCommand::SetInterval(ms)) => interval_ms = ms.max(100),
            Ok(CollectorCommand::ResetNetTotals(iface)) => {
                if collector.reset_totals(&iface) {
                    publish(&collector);
                }
            }
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
///
/// GPU is fanned out per device: there are `gpu_count` GPU threads
/// (one per detected device), each owning its own
/// [`LatestSlot<GpuSnapshot>`] in [`Self::gpu_slots`] and its own
/// `Sender<CollectorCommand>` in the matching slot of `txs.gpu`.
/// `gpu_slots` and `txs.gpu` both have length `gpu_count`; there
/// are no phantom slots to skip.
pub(crate) struct CollectorManager {
    /// Command sender per subsystem and per GPU device. The GPU
    /// slot of `txs` is a `Vec<Sender<CollectorCommand>>` of
    /// length `gpu_count`.
    txs: PerSubsystem<Sender<CollectorCommand>>,

    pub(crate) cpu_slot: LatestSlot<CpuSnapshot>,
    pub(crate) mem_slot: LatestSlot<MemSnapshot>,
    pub(crate) disk_slot: LatestSlot<DiskSnapshot>,
    pub(crate) net_slot: LatestSlot<NetSnapshot>,
    /// Per-device GPU snapshot slots. Length matches `gpu_count`.
    pub(crate) gpu_slots: Vec<LatestSlot<GpuSnapshot>>,
    pub(crate) proc_slot: LatestSlot<ProcSnapshot>,
    pub(crate) statusbar_slot: LatestSlot<StatusbarSnapshot>,

    /// Number of GPU devices discovered at startup. Fixed for the
    /// lifetime of the process.
    gpu_count: u8,

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
/// in `SubsystemKind::all_for`-canonical order, except
/// `Statusbar` which has a hardcoded cadence
/// ([`crate::collect::statusbar::STATUSBAR_UPDATE_MS`]) and is
/// not user-configurable.
///
/// This is the shared driver behind both startup spawning (where
/// each subsystem's spawn site reads its own field directly for
/// readability) and `apply_refresh` (which iterates here to
/// broadcast every resolved value uniformly). Behavior is
/// byte-identical to the per-field reads in `start`.
fn resolved_intervals(
    refresh: &RefreshConfig,
    gpu_count: u8,
) -> impl Iterator<Item = (SubsystemKind, u64)> + '_ {
    let cpu_ms = refresh.effective(refresh.cpu_update_ms);
    let mem_ms = refresh.effective(refresh.mem_update_ms);
    let disk_ms = refresh.effective(refresh.disk_update_ms);
    let net_ms = refresh.effective(refresh.net_update_ms);
    let gpu_ms = refresh.effective(refresh.gpu_update_ms);
    let proc_ms = refresh.effective(refresh.proc_update_ms);
    [
        (SubsystemKind::Cpu, cpu_ms),
        (SubsystemKind::Mem, mem_ms),
        (SubsystemKind::Disk, disk_ms),
        (SubsystemKind::Net, net_ms),
    ]
    .into_iter()
    .chain((0..gpu_count).map(move |n| (SubsystemKind::Gpu(n), gpu_ms)))
    .chain(std::iter::once((SubsystemKind::Proc, proc_ms)))
}

impl CollectorManager {
    /// Start all collector threads with intervals resolved from
    /// `refresh`.
    ///
    /// `CollectorManager` is the **single resolver** of the
    /// "0 = inherit global" rule (see [`RefreshConfig::effective`]):
    /// every per-widget interval is resolved here, including
    /// `gpu_update_ms` which is broadcast uniformly to every
    /// detected GPU device. The same resolution rule is reapplied
    /// at runtime via [`Self::apply_refresh`], so startup and every
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

        // Network thread (custom loop — handles the optional
        // ResetNetTotals variant of CollectorCommand).
        let (net_tx, net_rx) = mpsc::channel();
        let net_join = {
            let slot = net_slot.clone();
            let tx = event_tx.clone();
            std::thread::spawn(move || {
                run_net_loop(NetCollector::new(), net_ms, slot, tx, net_rx);
            })
        };

        // GPU threads — one per detected device. Discovery is
        // synchronous (loads each vendor DLL, calls each vendor
        // init, enumerates devices) and runs here so that
        // gpu_count is fixed before we publish the manager.
        let devices = crate::collect::gpu::discover();
        let gpu_count = devices.len() as u8;
        let gpu_slots: Vec<LatestSlot<GpuSnapshot>> =
            (0..gpu_count).map(|_| LatestSlot::new()).collect();
        let mut gpu_txs: Vec<Sender<CollectorCommand>> = Vec::with_capacity(gpu_count as usize);
        let mut gpu_joins: Vec<JoinHandle<()>> = Vec::with_capacity(gpu_count as usize);
        for (n, device) in devices.into_iter().enumerate() {
            let kind = SubsystemKind::Gpu(n as u8);
            let (tx, join) = spawn_collector(
                move || device,
                gpu_ms,
                &gpu_slots[n],
                &event_tx,
                AppEvent::SubsystemReady(kind),
            );
            gpu_txs.push(tx);
            gpu_joins.push(join);
        }

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
        let mut joins: Vec<(SubsystemKind, JoinHandle<()>)> =
            Vec::with_capacity(gpu_count as usize + 6);
        joins.push((SubsystemKind::Cpu, cpu_join));
        joins.push((SubsystemKind::Mem, mem_join));
        joins.push((SubsystemKind::Disk, disk_join));
        joins.push((SubsystemKind::Net, net_join));
        for (n, join) in gpu_joins.into_iter().enumerate() {
            joins.push((SubsystemKind::Gpu(n as u8), join));
        }
        joins.push((SubsystemKind::Proc, proc_join));
        joins.push((SubsystemKind::Statusbar, statusbar_join));

        Self {
            txs: PerSubsystem::new(
                cpu_tx,
                mem_tx,
                disk_tx,
                net_tx,
                gpu_txs,
                proc_tx,
                statusbar_tx,
            ),
            cpu_slot,
            mem_slot,
            disk_slot,
            net_slot,
            gpu_slots,
            proc_slot,
            statusbar_slot,
            gpu_count,
            joins,
        }
    }

    /// Number of GPU devices discovered at startup. Fixed for the
    /// lifetime of the process.
    pub(crate) fn gpu_count(&self) -> u8 {
        self.gpu_count
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
        for (kind, ms) in resolved_intervals(refresh, self.gpu_count) {
            send_command(
                self.txs.get(kind),
                kind,
                "apply_refresh",
                CollectorCommand::SetInterval(ms),
            );
        }
    }

    /// Reset cumulative network totals for an interface.
    pub(crate) fn reset_net_totals(&self, iface: String) {
        send_command(
            self.txs.get(SubsystemKind::Net),
            SubsystemKind::Net,
            "reset_net_totals",
            CollectorCommand::ResetNetTotals(iface),
        );
    }

    /// Shut down all collector threads and wait for them to finish.
    pub(crate) fn shutdown(&mut self) {
        for kind in SubsystemKind::all_for(self.gpu_count) {
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
    // shape: which subsystems are covered, in what order, with what
    // resolved value per kind, and how `gpu_count` fans out.
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
    fn resolved_intervals_zero_gpu_count_yields_five_entries() {
        let r = refresh_with(2000, 0, 0, 0, 0, 0, 0);
        let pairs: Vec<_> = resolved_intervals(&r, 0).collect();
        assert_eq!(pairs.len(), 5);
        assert_eq!(pairs[0], (SubsystemKind::Cpu, 2000));
        assert_eq!(pairs[1], (SubsystemKind::Mem, 2000));
        assert_eq!(pairs[2], (SubsystemKind::Disk, 2000));
        assert_eq!(pairs[3], (SubsystemKind::Net, 2000));
        assert_eq!(pairs[4], (SubsystemKind::Proc, 2000));
        assert!(
            !pairs
                .iter()
                .any(|(k, _)| matches!(k, SubsystemKind::Statusbar)),
            "Statusbar must never appear — its cadence is hardcoded \
             via STATUSBAR_UPDATE_MS",
        );
    }

    #[test]
    fn resolved_intervals_fans_gpu_value_across_every_device() {
        let r = refresh_with(2000, 0, 0, 0, 0, 750, 0);
        let pairs: Vec<_> = resolved_intervals(&r, 4).collect();
        assert_eq!(pairs.len(), 9);
        for n in 0..4 {
            let (kind, ms) = pairs[4 + n as usize];
            assert_eq!(kind, SubsystemKind::Gpu(n));
            assert_eq!(ms, 750, "every GPU device shares one resolved value");
        }
    }

    #[test]
    fn resolved_intervals_mix_of_overrides_and_inheritance() {
        // CPU overridden, Mem inherits, Disk overridden, Net inherits,
        // GPU overridden, Proc inherits. The contract is that each
        // subsystem independently applies effective(widget_ms).
        let r = refresh_with(2000, 250, 0, 500, 0, 1000, 0);
        let pairs: Vec<(SubsystemKind, u64)> = resolved_intervals(&r, 2).collect();
        let by_kind: std::collections::HashMap<_, _> = pairs.iter().copied().collect();
        assert_eq!(by_kind[&SubsystemKind::Cpu], 250);
        assert_eq!(by_kind[&SubsystemKind::Mem], 2000);
        assert_eq!(by_kind[&SubsystemKind::Disk], 500);
        assert_eq!(by_kind[&SubsystemKind::Net], 2000);
        assert_eq!(by_kind[&SubsystemKind::Gpu(0)], 1000);
        assert_eq!(by_kind[&SubsystemKind::Gpu(1)], 1000);
        assert_eq!(by_kind[&SubsystemKind::Proc], 2000);
    }

    #[test]
    fn resolved_intervals_canonical_order_matches_subsystem_kind() {
        // Iteration order is contract: Cpu, Mem, Disk, Net,
        // Gpu(0..gpu_count), Proc. This mirrors
        // `SubsystemKind::all_for` minus Statusbar.
        let r = refresh_with(1000, 0, 0, 0, 0, 0, 0);
        let kinds: Vec<SubsystemKind> = resolved_intervals(&r, 3).map(|(k, _)| k).collect();
        let expected: Vec<SubsystemKind> = SubsystemKind::all_for(3)
            .filter(|k| !matches!(k, SubsystemKind::Statusbar))
            .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn resolved_intervals_applies_clamps_per_subsystem() {
        // Each subsystem hits the rule independently — under-100
        // overrides are floored, over-ceiling are capped.
        let r = refresh_with(2000, 50, 86_400_001, 0, 99, 200, -1);
        let by_kind: std::collections::HashMap<_, _> = resolved_intervals(&r, 1).collect();
        assert_eq!(by_kind[&SubsystemKind::Cpu], 100, "50 → floored to 100");
        assert_eq!(
            by_kind[&SubsystemKind::Mem],
            86_400_000,
            "86_400_001 → capped at 86_400_000",
        );
        assert_eq!(by_kind[&SubsystemKind::Disk], 2000, "0 → inherits global",);
        assert_eq!(by_kind[&SubsystemKind::Net], 100, "99 → floored to 100");
        assert_eq!(by_kind[&SubsystemKind::Gpu(0)], 200);
        assert_eq!(
            by_kind[&SubsystemKind::Proc],
            2000,
            "-1 → inherits global (defensive)",
        );
    }
}
