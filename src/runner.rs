use crate::collect::CollectStatus;
use crate::collect::Collector;
use crate::collect::cpu::CpuCollector;
use crate::collect::disk::DiskCollector;
use crate::collect::gpu::GpuCollector;
use crate::collect::memory::MemCollector;
use crate::collect::network::NetCollector;
use crate::collect::process::ProcCollector;
use crate::domain::{
    cpu::CpuInfo, disk::DiskData, gpu::GpuInfo, memory::MemInfo, network::NetInfo,
    process::ProcInfo,
};
use crate::event::{AppEvent, PerSubsystem, SubsystemKind};
use std::sync::{
    Arc, Mutex,
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

#[derive(Debug, Clone)]
pub(crate) struct GpuSnapshot {
    pub(crate) gpus: Vec<GpuInfo>,
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

// ---------------------------------------------------------------------------
// LatestSlot<T> — generic per-subsystem shared slot with coalescing
// ---------------------------------------------------------------------------

/// Thread-safe slot that always holds the latest value.
///
/// Publishers overwrite; consumers read the latest. Multiple publishes
/// between reads naturally coalesce — only the most recent value is kept.
pub(crate) struct LatestSlot<T> {
    inner: Arc<Mutex<Option<Arc<T>>>>,
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
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Store new data, replacing any previous value.
    pub(crate) fn publish(&self, data: T) {
        let mut slot = self.inner.lock().expect("slot mutex poisoned");
        *slot = Some(Arc::new(data));
    }

    /// Read the latest value, if any.
    pub(crate) fn latest(&self) -> Option<Arc<T>> {
        self.inner.lock().expect("slot mutex poisoned").clone()
    }
}

// ---------------------------------------------------------------------------
// Collector commands
// ---------------------------------------------------------------------------

/// Commands sent to a collector thread.
pub(crate) enum CollectorCommand {
    /// Change the collection interval.
    SetInterval(u64),
    /// Graceful shutdown.
    Shutdown,
}

/// Commands sent to the network collector thread.
pub(crate) enum NetCommand {
    /// Change the collection interval.
    SetInterval(u64),
    /// Reset cumulative network totals for an interface.
    ResetTotals(String),
    /// Graceful shutdown.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Generic collector thread loop
// ---------------------------------------------------------------------------

/// Run a collector in a loop: collect → publish → sleep → repeat.
///
/// Used for CPU, memory, disk, GPU, and process collectors.
fn run_collector_loop<C, S>(
    mut collector: C,
    initial_interval_ms: u64,
    slot: LatestSlot<S>,
    event_tx: Sender<AppEvent>,
    wakeup: AppEvent,
    cmd_rx: Receiver<CollectorCommand>,
    snapshot_fn: impl Fn(&C) -> S,
) where
    C: Collector,
{
    let mut interval_ms = initial_interval_ms.max(100);
    loop {
        collector.collect();
        slot.publish(snapshot_fn(&collector));
        let _ = event_tx.send(wakeup);

        match cmd_rx.recv_timeout(Duration::from_millis(interval_ms)) {
            Ok(CollectorCommand::SetInterval(ms)) => interval_ms = ms.max(100),
            Ok(CollectorCommand::Shutdown) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Run the network collector with support for `ResetTotals` commands.
fn run_net_loop(
    mut collector: NetCollector,
    initial_interval_ms: u64,
    slot: LatestSlot<NetSnapshot>,
    event_tx: Sender<AppEvent>,
    cmd_rx: Receiver<NetCommand>,
) {
    let mut interval_ms = initial_interval_ms.max(100);
    let publish = |c: &NetCollector| {
        slot.publish(NetSnapshot {
            nets: c.nets.clone(),
            status: c.status.clone(),
        });
        let _ = event_tx.send(AppEvent::SubsystemReady(SubsystemKind::Net));
    };

    loop {
        collector.collect();
        publish(&collector);

        match cmd_rx.recv_timeout(Duration::from_millis(interval_ms)) {
            Ok(NetCommand::SetInterval(ms)) => interval_ms = ms.max(100),
            Ok(NetCommand::ResetTotals(iface)) => {
                if collector.reset_totals(&iface) {
                    publish(&collector);
                }
            }
            Ok(NetCommand::Shutdown) => break,
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
fn spawn_collector<C, S>(
    collector_fn: impl FnOnce() -> C + Send + 'static,
    update_ms: u64,
    slot: &LatestSlot<S>,
    event_tx: &Sender<AppEvent>,
    wakeup: AppEvent,
    snapshot_fn: impl Fn(&C) -> S + Send + 'static,
) -> (Sender<CollectorCommand>, JoinHandle<()>)
where
    C: Collector,
    S: Send + Sync + 'static,
{
    let (tx, rx) = mpsc::channel();
    let slot = slot.clone();
    let event_tx = event_tx.clone();
    let handle = std::thread::spawn(move || {
        run_collector_loop(
            collector_fn(),
            update_ms,
            slot,
            event_tx,
            wakeup,
            rx,
            snapshot_fn,
        );
    });
    (tx, handle)
}

// ---------------------------------------------------------------------------
// CollectorManager — owns all collector threads
// ---------------------------------------------------------------------------

/// Per-collector command channel sender.
///
/// Most collectors use the uniform [`CollectorCommand`] type for
/// `SetInterval`/`Shutdown`. The network collector additionally
/// supports `ResetTotals(String)`, so its channel carries the
/// wider [`NetCommand`] type. Wrapping both in a single enum lets
/// `CollectorManager` store a uniform `PerSubsystem<CollectorTx>`
/// while still dispatching the right wire format per subsystem at
/// the boundary.
pub(crate) enum CollectorTx {
    Standard(Sender<CollectorCommand>),
    Net(Sender<NetCommand>),
}

impl CollectorTx {
    /// Send a `SetInterval` command. The wire format depends on
    /// the wrapped sender type.
    fn set_interval(&self, target: &'static str, ms: u64) {
        match self {
            CollectorTx::Standard(tx) => {
                if let Err(e) = tx.send(CollectorCommand::SetInterval(ms)) {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Runner,
                        target,
                        error = %e,
                        "interval command send failed",
                    );
                }
            }
            CollectorTx::Net(tx) => {
                if let Err(e) = tx.send(NetCommand::SetInterval(ms)) {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Runner,
                        target,
                        error = %e,
                        "interval command send failed",
                    );
                }
            }
        }
    }

    /// Send a `Shutdown` command. Errors are intentionally
    /// discarded — by this point the collector thread may have
    /// already exited (e.g. on a panic), and the join in
    /// `shutdown` will surface any real failure.
    fn shutdown(&self) {
        match self {
            CollectorTx::Standard(tx) => {
                let _ = tx.send(CollectorCommand::Shutdown);
            }
            CollectorTx::Net(tx) => {
                let _ = tx.send(NetCommand::Shutdown);
            }
        }
    }
}

/// Manages per-collector threads with independent timers.
///
/// Each collector runs on its own thread with a `LatestSlot<T>` for
/// coalescing and publishes wakeup events through the shared channel.
pub(crate) struct CollectorManager {
    /// One command sender per subsystem, keyed by [`SubsystemKind`].
    /// Net carries the wider [`CollectorTx::Net`] variant for its
    /// `ResetTotals` command; every other subsystem is `Standard`.
    txs: PerSubsystem<CollectorTx>,

    pub(crate) cpu_slot: LatestSlot<CpuSnapshot>,
    pub(crate) mem_slot: LatestSlot<MemSnapshot>,
    pub(crate) disk_slot: LatestSlot<DiskSnapshot>,
    pub(crate) net_slot: LatestSlot<NetSnapshot>,
    pub(crate) gpu_slot: LatestSlot<GpuSnapshot>,
    pub(crate) proc_slot: LatestSlot<ProcSnapshot>,

    joins: Vec<(&'static str, JoinHandle<()>)>,
}

impl CollectorManager {
    /// Start all collector threads with the given initial interval.
    pub(crate) fn start(update_ms: u64, event_tx: Sender<AppEvent>) -> Self {
        let core_count = crate::collect::cpu::get_core_count();

        let cpu_slot = LatestSlot::new();
        let mem_slot = LatestSlot::new();
        let disk_slot = LatestSlot::new();
        let net_slot = LatestSlot::new();
        let gpu_slot = LatestSlot::new();
        let proc_slot = LatestSlot::new();

        let mut joins = Vec::with_capacity(6);

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
            |c| CpuSnapshot {
                info: c.info.clone(),
                status: c.status.clone(),
            },
        );
        joins.push(("cpu", cpu_join));

        // Memory thread
        let (mem_tx, mem_join) = spawn_collector(
            MemCollector::new,
            update_ms,
            &mem_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Mem),
            |c| MemSnapshot {
                info: c.info.clone(),
                status: c.status.clone(),
            },
        );
        joins.push(("memory", mem_join));

        // Disk thread
        let (disk_tx, disk_join) = spawn_collector(
            DiskCollector::new,
            update_ms,
            &disk_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Disk),
            |c| DiskSnapshot {
                info: c.info.clone(),
                status: c.status.clone(),
            },
        );
        joins.push(("disk", disk_join));

        // Network thread (custom loop — uses NetCommand for ResetTotals)
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

        // GPU thread
        let (gpu_tx, gpu_join) = spawn_collector(
            GpuCollector::new,
            update_ms,
            &gpu_slot,
            &event_tx,
            AppEvent::SubsystemReady(SubsystemKind::Gpu),
            |c| GpuSnapshot {
                gpus: c.gpus.clone(),
                status: c.status.clone(),
            },
        );
        joins.push(("gpu", gpu_join));

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
            |c| ProcSnapshot {
                procs: c.procs.clone(),
                status: c.status.clone(),
            },
        );
        joins.push(("process", proc_join));

        Self {
            txs: PerSubsystem::new(
                CollectorTx::Standard(cpu_tx),
                CollectorTx::Standard(mem_tx),
                CollectorTx::Standard(disk_tx),
                CollectorTx::Net(net_tx),
                CollectorTx::Standard(gpu_tx),
                CollectorTx::Standard(proc_tx),
            ),
            cpu_slot,
            mem_slot,
            disk_slot,
            net_slot,
            gpu_slot,
            proc_slot,
            joins,
        }
    }

    /// Update the collection interval for the named subsystem.
    pub(crate) fn set_interval(&self, kind: SubsystemKind, ms: u64) {
        self.txs.get(kind).set_interval(kind.as_str(), ms);
    }

    /// Reset cumulative network totals for an interface.
    pub(crate) fn reset_net_totals(&self, iface: String) {
        let CollectorTx::Net(tx) = self.txs.get(SubsystemKind::Net) else {
            // Invariant: the net slot is always populated with
            // CollectorTx::Net by `start()`. The struct fields are
            // private so no other code can mutate this.
            unreachable!("net subsystem must always carry a Net sender");
        };
        if let Err(e) = tx.send(NetCommand::ResetTotals(iface)) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Runner,
                target = SubsystemKind::Net.as_str(),
                error = %e,
                "reset-totals command send failed",
            );
        }
    }

    /// Shut down all collector threads and wait for them to finish.
    pub(crate) fn shutdown(&mut self) {
        for kind in SubsystemKind::ALL {
            self.txs.get(kind).shutdown();
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
