use std::collections::{HashMap, VecDeque};

/// GPU monitoring data for a single GPU device.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// GPU device name (e.g. "NVIDIA RTX 4090").
    pub name: String,
    /// Usage histories. Keys: "gpu-totals", "gpu-vram-totals", "gpu-pwr-totals".
    pub gpu_percent: HashMap<String, VecDeque<i64>>,
    /// GPU core clock speed in MHz.
    pub gpu_clock_speed: u32,
    /// Memory clock speed in MHz.
    pub mem_clock_speed: u64,
    /// Current power draw in milliwatts.
    pub pwr_usage: i64,
    /// Maximum power limit in milliwatts.
    pub pwr_max_usage: i64,
    /// GPU power state (0-15 for NVIDIA).
    pub pwr_state: i64,
    /// Temperature history in °C.
    pub temp: VecDeque<i64>,
    /// Shutdown/critical temperature threshold.
    pub temp_max: i64,
    /// Total VRAM in bytes.
    pub mem_total: u64,
    /// Used VRAM in bytes.
    pub mem_used: u64,
    /// VRAM utilization percentage history.
    pub mem_utilization_percent: VecDeque<i64>,
    /// PCIe transmit throughput in KB/s.
    pub pcie_tx: u64,
    /// PCIe receive throughput in KB/s.
    pub pcie_rx: u64,
    /// Video encoder utilization percentage.
    pub encoder_utilization: u64,
    /// Video decoder utilization percentage.
    pub decoder_utilization: u64,
    /// Which metrics this GPU supports.
    pub supported: GpuSupported,
}

/// Flags indicating which GPU metrics are available for this device.
#[derive(Debug, Clone)]
pub struct GpuSupported {
    pub gpu_utilization: bool,
    pub mem_utilization: bool,
    pub gpu_clock: bool,
    pub mem_clock: bool,
    pub pwr_usage: bool,
    pub pwr_state: bool,
    pub temp_info: bool,
    pub mem_total: bool,
    pub mem_used: bool,
    pub pcie_txrx: bool,
    pub encoder_utilization: bool,
    pub decoder_utilization: bool,
}

impl Default for GpuSupported {
    fn default() -> Self {
        Self {
            gpu_utilization: true,
            mem_utilization: true,
            gpu_clock: true,
            mem_clock: true,
            pwr_usage: true,
            pwr_state: true,
            temp_info: true,
            mem_total: true,
            mem_used: true,
            pcie_txrx: true,
            encoder_utilization: true,
            decoder_utilization: true,
        }
    }
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            gpu_percent: [
                ("gpu-totals".into(), VecDeque::new()),
                ("gpu-vram-totals".into(), VecDeque::new()),
                ("gpu-pwr-totals".into(), VecDeque::new()),
            ]
            .into_iter()
            .collect(),
            gpu_clock_speed: 0,
            mem_clock_speed: 0,
            pwr_usage: 0,
            pwr_max_usage: 255_000,
            pwr_state: 0,
            temp: VecDeque::from([0]),
            temp_max: 110,
            mem_total: 0,
            mem_used: 0,
            mem_utilization_percent: VecDeque::from([0]),
            pcie_tx: 0,
            pcie_rx: 0,
            encoder_utilization: 0,
            decoder_utilization: 0,
            supported: GpuSupported::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_info_default_has_correct_keys() {
        let gpu = GpuInfo::default();
        assert!(gpu.gpu_percent.contains_key("gpu-totals"));
        assert!(gpu.gpu_percent.contains_key("gpu-vram-totals"));
        assert!(gpu.gpu_percent.contains_key("gpu-pwr-totals"));
    }

    #[test]
    fn gpu_supported_default_all_true() {
        let sup = GpuSupported::default();
        assert!(sup.gpu_utilization);
        assert!(sup.mem_utilization);
        assert!(sup.temp_info);
        assert!(sup.pcie_txrx);
    }
}
