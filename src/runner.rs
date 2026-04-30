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
use crate::event::AppEvent;
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
// Layout hints (derived from hardware constants across subsystems)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LayoutHints {
    pub(crate) core_count: usize,
    pub(crate) gpu_count: usize,
    pub(crate) disk_count: usize,
    pub(crate) has_swap: bool,
}

// ---------------------------------------------------------------------------
// LatestSlot<T> — generic per-subsystem shared slot with coalescing
// ---------------------------------------------------------------------------

/// Thread-safe slot that always holds the latest value.
///
/// Publishers overwrite; consumers read the latest. Multiple publishes
/// between reads naturally coalesce — only the most recent value is kept.
#[derive(Clone)]
pub(crate) struct LatestSlot<T> {
    inner: Arc<Mutex<Option<Arc<T>>>>,
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
        let _ = event_tx.send(AppEvent::NetReady);
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
// CollectorManager — owns all collector threads
// ---------------------------------------------------------------------------

/// Manages per-collector threads with independent timers.
///
/// Each collector runs on its own thread with a `LatestSlot<T>` for
/// coalescing and publishes wakeup events through the shared channel.
pub(crate) struct CollectorManager {
    cpu_tx: Sender<CollectorCommand>,
    mem_tx: Sender<CollectorCommand>,
    disk_tx: Sender<CollectorCommand>,
    net_tx: Sender<NetCommand>,
    gpu_tx: Sender<CollectorCommand>,
    proc_tx: Sender<CollectorCommand>,

    pub(crate) cpu_slot: LatestSlot<CpuSnapshot>,
    pub(crate) mem_slot: LatestSlot<MemSnapshot>,
    pub(crate) disk_slot: LatestSlot<DiskSnapshot>,
    pub(crate) net_slot: LatestSlot<NetSnapshot>,
    pub(crate) gpu_slot: LatestSlot<GpuSnapshot>,
    pub(crate) proc_slot: LatestSlot<ProcSnapshot>,

    joins: Vec<JoinHandle<()>>,
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
        let (cpu_tx, cpu_rx) = mpsc::channel();
        {
            let slot = cpu_slot.clone();
            let tx = event_tx.clone();
            joins.push(std::thread::spawn(move || {
                let mut cpu = CpuCollector::new();
                cpu.init();
                run_collector_loop(cpu, update_ms, slot, tx, AppEvent::CpuReady, cpu_rx, |c| {
                    CpuSnapshot {
                        info: c.info.clone(),
                        status: c.status.clone(),
                    }
                });
            }));
        }

        // Memory thread
        let (mem_tx, mem_rx) = mpsc::channel();
        {
            let slot = mem_slot.clone();
            let tx = event_tx.clone();
            joins.push(std::thread::spawn(move || {
                run_collector_loop(
                    MemCollector::new(),
                    update_ms,
                    slot,
                    tx,
                    AppEvent::MemReady,
                    mem_rx,
                    |c| MemSnapshot {
                        info: c.info.clone(),
                        status: c.status.clone(),
                    },
                );
            }));
        }

        // Disk thread
        let (disk_tx, disk_rx) = mpsc::channel();
        {
            let slot = disk_slot.clone();
            let tx = event_tx.clone();
            joins.push(std::thread::spawn(move || {
                run_collector_loop(
                    DiskCollector::new(),
                    update_ms,
                    slot,
                    tx,
                    AppEvent::DiskReady,
                    disk_rx,
                    |c| DiskSnapshot {
                        info: c.info.clone(),
                        status: c.status.clone(),
                    },
                );
            }));
        }

        // Network thread (custom loop for ResetTotals)
        let (net_tx, net_rx) = mpsc::channel();
        {
            let slot = net_slot.clone();
            let tx = event_tx.clone();
            joins.push(std::thread::spawn(move || {
                run_net_loop(NetCollector::new(), update_ms, slot, tx, net_rx);
            }));
        }

        // GPU thread
        let (gpu_tx, gpu_rx) = mpsc::channel();
        {
            let slot = gpu_slot.clone();
            let tx = event_tx.clone();
            joins.push(std::thread::spawn(move || {
                run_collector_loop(
                    GpuCollector::new(),
                    update_ms,
                    slot,
                    tx,
                    AppEvent::GpuReady,
                    gpu_rx,
                    |c| GpuSnapshot {
                        gpus: c.gpus.clone(),
                        status: c.status.clone(),
                    },
                );
            }));
        }

        // Process thread
        let (proc_tx, proc_rx) = mpsc::channel();
        {
            let slot = proc_slot.clone();
            let tx = event_tx;
            joins.push(std::thread::spawn(move || {
                let mut proc_collector = ProcCollector::new();
                proc_collector.set_core_count(core_count);
                run_collector_loop(
                    proc_collector,
                    update_ms,
                    slot,
                    tx,
                    AppEvent::ProcReady,
                    proc_rx,
                    |c| ProcSnapshot {
                        procs: c.procs.clone(),
                        status: c.status.clone(),
                    },
                );
            }));
        }

        Self {
            cpu_tx,
            mem_tx,
            disk_tx,
            net_tx,
            gpu_tx,
            proc_tx,
            cpu_slot,
            mem_slot,
            disk_slot,
            net_slot,
            gpu_slot,
            proc_slot,
            joins,
        }
    }

    /// Update the collection interval for a single collector.
    pub(crate) fn set_cpu_interval(&self, ms: u64) {
        let _ = self.cpu_tx.send(CollectorCommand::SetInterval(ms));
    }

    /// Update the collection interval for the memory collector.
    pub(crate) fn set_mem_interval(&self, ms: u64) {
        let _ = self.mem_tx.send(CollectorCommand::SetInterval(ms));
    }

    /// Update the collection interval for the disk collector.
    pub(crate) fn set_disk_interval(&self, ms: u64) {
        let _ = self.disk_tx.send(CollectorCommand::SetInterval(ms));
    }

    /// Update the collection interval for the network collector.
    pub(crate) fn set_net_interval(&self, ms: u64) {
        let _ = self.net_tx.send(NetCommand::SetInterval(ms));
    }

    /// Update the collection interval for the GPU collector.
    pub(crate) fn set_gpu_interval(&self, ms: u64) {
        let _ = self.gpu_tx.send(CollectorCommand::SetInterval(ms));
    }

    /// Update the collection interval for the process collector.
    pub(crate) fn set_proc_interval(&self, ms: u64) {
        let _ = self.proc_tx.send(CollectorCommand::SetInterval(ms));
    }

    /// Reset cumulative network totals for an interface.
    pub(crate) fn reset_net_totals(&self, iface: String) {
        let _ = self.net_tx.send(NetCommand::ResetTotals(iface));
    }

    /// Shut down all collector threads and wait for them to finish.
    pub(crate) fn shutdown(&mut self) {
        let _ = self.cpu_tx.send(CollectorCommand::Shutdown);
        let _ = self.mem_tx.send(CollectorCommand::Shutdown);
        let _ = self.disk_tx.send(CollectorCommand::Shutdown);
        let _ = self.net_tx.send(NetCommand::Shutdown);
        let _ = self.gpu_tx.send(CollectorCommand::Shutdown);
        let _ = self.proc_tx.send(CollectorCommand::Shutdown);
        for join in self.joins.drain(..) {
            let _ = join.join();
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

    #[test]
    fn layout_hints_default() {
        let hints = LayoutHints::default();
        assert_eq!(hints.core_count, 0);
        assert_eq!(hints.gpu_count, 0);
        assert_eq!(hints.disk_count, 0);
        assert!(!hints.has_swap);
    }
}
