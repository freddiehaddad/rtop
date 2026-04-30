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
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Coordinates all data collectors.
pub struct Runner {
    pub cpu: CpuCollector,
    pub disk: DiskCollector,
    pub gpu: GpuCollector,
    pub mem: MemCollector,
    pub net: NetCollector,
    pub proc_collector: ProcCollector,
}

impl Runner {
    /// Create a new runner with default collectors.
    pub fn new() -> Self {
        let mut cpu = CpuCollector::new();
        cpu.init();
        Self {
            cpu,
            disk: DiskCollector::new(),
            gpu: GpuCollector::new(),
            mem: MemCollector::new(),
            net: NetCollector::new(),
            proc_collector: ProcCollector::new(),
        }
    }

    /// Run one collection cycle for all collectors.
    pub fn collect_all(&mut self) {
        self.cpu.collect();
        self.disk.collect();
        self.gpu.collect();
        self.mem.collect();
        self.net.collect();
        self.proc_collector.set_core_count(self.cpu.info.core_count);
        self.proc_collector.collect();
    }

    /// Clone the current public collector data into an immutable render snapshot.
    pub(crate) fn snapshot(&self, seq: u64) -> CollectionSnapshot {
        CollectionSnapshot::from_runner(self, seq)
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable point-in-time data used by the UI renderer.
#[derive(Debug, Clone)]
pub(crate) struct CollectionSnapshot {
    pub(crate) seq: u64,
    pub(crate) cpu: CpuSnapshot,
    pub(crate) disk: DiskSnapshot,
    pub(crate) gpu: GpuSnapshot,
    pub(crate) mem: MemSnapshot,
    pub(crate) net: NetSnapshot,
    pub(crate) proc_data: ProcSnapshot,
}

impl CollectionSnapshot {
    fn from_runner(runner: &Runner, seq: u64) -> Self {
        Self {
            seq,
            cpu: CpuSnapshot {
                info: runner.cpu.info.clone(),
                status: runner.cpu.status.clone(),
            },
            disk: DiskSnapshot {
                info: runner.disk.info.clone(),
                status: runner.disk.status.clone(),
            },
            gpu: GpuSnapshot {
                gpus: runner.gpu.gpus.clone(),
                status: runner.gpu.status.clone(),
            },
            mem: MemSnapshot {
                info: runner.mem.info.clone(),
                status: runner.mem.status.clone(),
            },
            net: NetSnapshot {
                nets: runner.net.nets.clone(),
                status: runner.net.status.clone(),
            },
            proc_data: ProcSnapshot {
                procs: runner.proc_collector.procs.clone(),
                status: runner.proc_collector.status.clone(),
            },
        }
    }

    pub(crate) fn layout_hints(&self) -> LayoutHints {
        LayoutHints {
            core_count: self.cpu.info.core_count,
            gpu_count: self.gpu.gpus.len(),
            disk_count: self.disk.info.disks.len(),
            has_swap: self.mem.info.stats.swap_total > 0,
        }
    }
}

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LayoutHints {
    pub(crate) core_count: usize,
    pub(crate) gpu_count: usize,
    pub(crate) disk_count: usize,
    pub(crate) has_swap: bool,
}

/// Shared latest-snapshot store used to hand immutable frames to the UI.
#[derive(Clone, Default)]
pub(crate) struct LatestSnapshot {
    inner: Arc<Mutex<Option<Arc<CollectionSnapshot>>>>,
}

impl LatestSnapshot {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn publish(&self, snapshot: CollectionSnapshot) -> Arc<CollectionSnapshot> {
        let snapshot = Arc::new(snapshot);
        let mut latest = self.inner.lock().expect("latest snapshot mutex poisoned");
        *latest = Some(Arc::clone(&snapshot));
        snapshot
    }

    pub(crate) fn latest(&self) -> Option<Arc<CollectionSnapshot>> {
        self.inner
            .lock()
            .expect("latest snapshot mutex poisoned")
            .clone()
    }

    pub(crate) fn latest_if_new(&self, last_seen_seq: u64) -> Option<Arc<CollectionSnapshot>> {
        self.latest()
            .filter(|snapshot| snapshot.seq > last_seen_seq)
    }
}

/// Background owner for all collectors.
pub(crate) struct CollectionWorker {
    latest: LatestSnapshot,
    tx: Sender<WorkerCommand>,
    join: Option<JoinHandle<()>>,
}

enum WorkerCommand {
    SetUpdateMs(u64),
    ResetNetTotals { iface: String },
    Shutdown,
}

impl CollectionWorker {
    pub(crate) fn start(update_ms: u64, event_tx: Sender<AppEvent>) -> Self {
        let latest = LatestSnapshot::new();
        let worker_latest = latest.clone();
        let (tx, rx) = mpsc::channel();
        let join =
            thread::spawn(move || run_collection_worker(update_ms, worker_latest, rx, event_tx));

        Self {
            latest,
            tx,
            join: Some(join),
        }
    }

    pub(crate) fn latest_if_new(&self, last_seen_seq: u64) -> Option<Arc<CollectionSnapshot>> {
        self.latest.latest_if_new(last_seen_seq)
    }

    pub(crate) fn set_update_ms(&self, update_ms: u64) {
        let _ = self.tx.send(WorkerCommand::SetUpdateMs(update_ms));
    }

    pub(crate) fn reset_net_totals(&self, iface: String) {
        let _ = self.tx.send(WorkerCommand::ResetNetTotals { iface });
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = self.tx.send(WorkerCommand::Shutdown);
            let _ = join.join();
        }
    }
}

impl Drop for CollectionWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_collection_worker(
    initial_update_ms: u64,
    latest: LatestSnapshot,
    rx: Receiver<WorkerCommand>,
    event_tx: Sender<AppEvent>,
) {
    let mut runner = Runner::new();
    let mut update_ms = initial_update_ms.max(100);
    let mut seq = 0;
    collect_and_publish(&mut runner, &latest, &mut seq, &event_tx);
    let mut next_collect = Instant::now() + Duration::from_millis(update_ms);

    loop {
        let timeout = next_collect.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(WorkerCommand::SetUpdateMs(ms)) => {
                update_ms = ms.max(100);
                next_collect = Instant::now() + Duration::from_millis(update_ms);
            }
            Ok(WorkerCommand::ResetNetTotals { iface }) => {
                if runner.net.reset_totals(&iface) {
                    publish_snapshot(&runner, &latest, &mut seq, &event_tx);
                }
            }
            Ok(WorkerCommand::Shutdown) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                collect_and_publish(&mut runner, &latest, &mut seq, &event_tx);
                next_collect = Instant::now() + Duration::from_millis(update_ms);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn collect_and_publish(
    runner: &mut Runner,
    latest: &LatestSnapshot,
    seq: &mut u64,
    event_tx: &Sender<AppEvent>,
) {
    runner.collect_all();
    publish_snapshot(runner, latest, seq, event_tx);
}

fn publish_snapshot(
    runner: &Runner,
    latest: &LatestSnapshot,
    seq: &mut u64,
    event_tx: &Sender<AppEvent>,
) {
    *seq = seq.saturating_add(1);
    latest.publish(runner.snapshot(*seq));
    let _ = event_tx.send(AppEvent::SnapshotReady);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner_with_sample_data() -> Runner {
        let mut runner = Runner {
            cpu: CpuCollector::default(),
            disk: DiskCollector::default(),
            gpu: GpuCollector::default(),
            mem: MemCollector::default(),
            net: NetCollector::default(),
            proc_collector: ProcCollector::default(),
        };
        runner.cpu.info.core_count = 8;
        runner.mem.info.stats.swap_total = 1024;
        runner.net.nets.push(NetInfo {
            name: "Ethernet".into(),
            ..NetInfo::default()
        });
        runner.proc_collector.procs.push(ProcInfo {
            pid: 42,
            name: "sample.exe".into(),
            ..ProcInfo::default()
        });
        runner
    }

    #[test]
    fn snapshot_clones_public_runner_data() {
        let runner = runner_with_sample_data();
        let snapshot = runner.snapshot(7);

        assert_eq!(snapshot.seq, 7);
        assert_eq!(snapshot.cpu.info.core_count, 8);
        assert_eq!(snapshot.net.nets.len(), 1);
        assert_eq!(snapshot.net.nets[0].name, "Ethernet");
        assert_eq!(snapshot.proc_data.procs[0].pid, 42);
    }

    #[test]
    fn snapshot_layout_hints_reflect_collected_data() {
        let runner = runner_with_sample_data();
        let hints = runner.snapshot(1).layout_hints();

        assert_eq!(hints.core_count, 8);
        assert!(hints.has_swap);
    }

    #[test]
    fn latest_snapshot_publishes_and_clones_arc() {
        let store = LatestSnapshot::new();
        let snapshot = runner_with_sample_data().snapshot(1);

        let published = store.publish(snapshot);
        let latest = store.latest().expect("snapshot should be available");

        assert!(Arc::ptr_eq(&published, &latest));
        assert_eq!(latest.seq, 1);
    }

    #[test]
    fn latest_if_new_returns_only_newer_sequences() {
        let store = LatestSnapshot::new();
        store.publish(runner_with_sample_data().snapshot(3));

        assert!(store.latest_if_new(2).is_some());
        assert!(store.latest_if_new(3).is_none());
        assert!(store.latest_if_new(4).is_none());
    }
}
