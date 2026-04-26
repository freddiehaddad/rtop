use crate::collect::cpu::CpuCollector;
use crate::collect::disk::DiskCollector;
use crate::collect::gpu::GpuCollector;
use crate::collect::memory::MemCollector;
use crate::collect::network::NetCollector;
use crate::collect::process::ProcCollector;

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
        let core_count = self.cpu.info.core_count;
        self.proc_collector.collect(core_count);
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
