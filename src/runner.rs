use crate::collect::CollectStatus;
use crate::collect::Collector;
use crate::collect::cpu::CpuCollector;
use crate::collect::disk::DiskCollector;
use crate::collect::memory::MemCollector;
use crate::collect::network::NetCollector;
use crate::collect::process::ProcCollector;
use crate::collect::statusbar::{STATUSBAR_UPDATE_MS, StatusbarCollector};
use crate::config::MAX_GPUS;
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
/// `Sender<CollectorCommand>` in the matching slot of `txs`. GPU
/// indices `>= gpu_count` carry an empty `LatestSlot` and a `None`
/// sender — `set_interval` and `shutdown` skip those slots
/// gracefully.
pub(crate) struct CollectorManager {
    /// Command sender per non-GPU subsystem and per GPU device
    /// slot. GPU slots beyond `gpu_count` are `None` (no thread to
    /// address).
    txs: PerSubsystem<Option<Sender<CollectorCommand>>>,

    pub(crate) cpu_slot: LatestSlot<CpuSnapshot>,
    pub(crate) mem_slot: LatestSlot<MemSnapshot>,
    pub(crate) disk_slot: LatestSlot<DiskSnapshot>,
    pub(crate) net_slot: LatestSlot<NetSnapshot>,
    /// Per-device GPU snapshot slots. Slots beyond `gpu_count` stay
    /// empty for the lifetime of the process.
    pub(crate) gpu_slots: [LatestSlot<GpuSnapshot>; MAX_GPUS],
    pub(crate) proc_slot: LatestSlot<ProcSnapshot>,
    pub(crate) statusbar_slot: LatestSlot<StatusbarSnapshot>,

    /// Number of GPU devices discovered at startup. Fixed for the
    /// lifetime of the process.
    gpu_count: u8,

    joins: Vec<(&'static str, JoinHandle<()>)>,
}

/// Send `cmd` to the named subsystem's collector thread, logging a
/// warning on send failure. Centralised so every send goes through one
/// audit point. A `None` sender (GPU slot beyond `gpu_count`) is a
/// no-op — there is no thread to address.
fn send_command(
    tx: Option<&Sender<CollectorCommand>>,
    target: &'static str,
    op: &'static str,
    cmd: CollectorCommand,
) {
    let Some(tx) = tx else { return };
    if let Err(e) = tx.send(cmd) {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Runner,
            target,
            op,
            error = %e,
            "command send failed",
        );
    }
}

impl CollectorManager {
    /// Start all collector threads with the given initial interval.
    ///
    /// `gpu_intervals[n]` is the **already-resolved** effective
    /// interval for GPU `n` (the caller has already passed
    /// `config.refresh.gpu_update_ms[n]` through
    /// [`crate::config::Config::effective_interval`]). Slots
    /// beyond the discovered device count are simply unread, so
    /// callers can pre-fill the entire array unconditionally.
    pub(crate) fn start(
        update_ms: u64,
        event_tx: Sender<AppEvent>,
        gpu_intervals: [u64; MAX_GPUS],
    ) -> Self {
        let core_count = crate::collect::cpu::get_core_count();

        let cpu_slot = LatestSlot::new();
        let mem_slot = LatestSlot::new();
        let disk_slot = LatestSlot::new();
        let net_slot = LatestSlot::new();
        let gpu_slots: [LatestSlot<GpuSnapshot>; MAX_GPUS] =
            std::array::from_fn(|_| LatestSlot::new());
        let proc_slot = LatestSlot::new();
        let statusbar_slot = LatestSlot::new();

        let mut joins = Vec::with_capacity(MAX_GPUS + 7);

        // CPU thread
        let (cpu_tx, cpu_join) = spawn_collector(
            || {
                let mut cpu = CpuCollector::new();
                cpu.init();
                cpu
            },
            update_ms,
            &cpu_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Cpu),
        );
        joins.push(("cpu", cpu_join));

        // Memory thread
        let (mem_tx, mem_join) = spawn_collector(
            MemCollector::new,
            update_ms,
            &mem_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Mem),
        );
        joins.push(("memory", mem_join));

        // Disk thread
        let (disk_tx, disk_join) = spawn_collector(
            DiskCollector::new,
            update_ms,
            &disk_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Disk),
        );
        joins.push(("disk", disk_join));

        // Network thread (custom loop — handles the optional
        // ResetNetTotals variant of CollectorCommand).
        let (net_tx, net_rx) = mpsc::channel();
        {
            let slot = net_slot.clone();
            let tx = event_tx.clone();
            joins.push((
                "network",
                std::thread::spawn(move || {
                    run_net_loop(NetCollector::new(), update_ms, slot, tx, net_rx);
                }),
            ));
        }

        // GPU threads — one per detected device. Discovery is
        // synchronous (loads each vendor DLL, calls each vendor
        // init, enumerates devices) and runs here so that
        // gpu_count is fixed before we publish the manager. Slots
        // beyond gpu_count keep `None` senders forever.
        let devices = crate::collect::gpu::discover();
        let gpu_count = devices.len() as u8;
        let mut gpu_txs: [Option<Sender<CollectorCommand>>; MAX_GPUS] =
            std::array::from_fn(|_| None);
        for (n, device) in devices.into_iter().enumerate() {
            let kind = SubsystemKind::Gpu(n as u8);
            let (tx, join) = spawn_collector(
                move || device,
                gpu_intervals[n],
                &gpu_slots[n],
                &event_tx,
                AppEvent::SubsystemReady(kind),
            );
            gpu_txs[n] = Some(tx);
            // `kind.as_str()` returns the interned "gpuN" string
            // from the same const table that powers the
            // diagnostics field on every gpu-tagged tracing event;
            // sharing that name here keeps the join-handle target
            // label aligned with the rest of the gpu logging.
            joins.push((kind.as_str(), join));
        }

        // Process thread
        let (proc_tx, proc_join) = spawn_collector(
            move || {
                let mut proc_collector = ProcCollector::new();
                proc_collector.set_core_count(core_count);
                proc_collector
            },
            update_ms,
            &proc_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Proc),
        );
        joins.push(("process", proc_join));

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
        joins.push(("statusbar", statusbar_join));

        Self {
            txs: PerSubsystem::new(
                Some(cpu_tx),
                Some(mem_tx),
                Some(disk_tx),
                Some(net_tx),
                gpu_txs,
                Some(proc_tx),
                Some(statusbar_tx),
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
    /// lifetime of the process. Slots `n >= gpu_count` in
    /// [`Self::gpu_slots`] stay empty; `set_interval` and
    /// `shutdown` skip the matching `None` sender entries.
    pub(crate) fn gpu_count(&self) -> u8 {
        self.gpu_count
    }

    /// Update the collection interval for the named subsystem.
    pub(crate) fn set_interval(&self, kind: SubsystemKind, ms: u64) {
        send_command(
            self.txs.get(kind).as_ref(),
            kind.as_str(),
            "set_interval",
            CollectorCommand::SetInterval(ms),
        );
    }

    /// Reset cumulative network totals for an interface.
    pub(crate) fn reset_net_totals(&self, iface: String) {
        send_command(
            self.txs.get(SubsystemKind::Net).as_ref(),
            SubsystemKind::Net.as_str(),
            "reset_net_totals",
            CollectorCommand::ResetNetTotals(iface),
        );
    }

    /// Shut down all collector threads and wait for them to finish.
    pub(crate) fn shutdown(&mut self) {
        for kind in SubsystemKind::ALL {
            // GPU slots beyond gpu_count carry None — no thread to
            // address. Shutdown send errors on present senders are
            // intentionally discarded: by this point the collector
            // thread may have already exited (e.g. on a panic), and
            // the join below will surface any real failure.
            if let Some(tx) = self.txs.get(kind).as_ref() {
                let _ = tx.send(CollectorCommand::Shutdown);
            }
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
                    target,
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
}
