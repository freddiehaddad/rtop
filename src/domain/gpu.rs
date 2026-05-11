use std::collections::VecDeque;

/// GPU usage percentage histories.
#[derive(Debug, Clone, Default)]
pub struct GpuPercent {
    pub utilization: VecDeque<i64>,
    pub vram: VecDeque<i64>,
    pub power: VecDeque<i64>,
}

/// GPU monitoring data for a single GPU device.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// Vendor-prefixed stable identifier for this device. Format
    /// is `"<VENDOR>:<UUID-or-fallback>"`:
    ///
    /// * NVIDIA: `"NVIDIA:GPU-12345678-1234-..."` — sourced from
    ///   `nvmlDeviceGetUUID`. Fallback `"NVIDIA:adapter:N"` (with
    ///   vendor-relative index) when NVML is unavailable or the
    ///   UUID call fails.
    /// * AMD: `"AMD:<UDID>"` — sourced from `AdapterInfoX4::strUDID`.
    ///   Fallback `"AMD:adapter:N"` if the UDID is empty.
    /// * Intel: `"INTEL:adapter:N"` — IGCL does not expose a
    ///   per-card UUID at this struct level, so the
    ///   vendor-relative discovery index is the only stable id
    ///   available today.
    ///
    /// Used by [`crate::app::GpuViewState::reconcile`] to match
    /// the persisted `view.gpu_iface` against the live device list.
    /// Bound at discovery time; immutable for the device's
    /// lifetime.
    pub stable_id: String,
    /// GPU device name (e.g. "NVIDIA RTX 4090").
    pub name: String,
    /// Usage histories.
    pub gpu_percent: GpuPercent,
    /// GPU core clock speed in MHz.
    pub gpu_clock_speed: u32,
    /// Maximum GPU core clock speed in MHz.
    pub gpu_max_clock_speed: u32,
    /// Current power draw in milliwatts.
    pub pwr_usage: i64,
    /// Maximum power limit in milliwatts.
    pub pwr_max_usage: i64,
    /// Temperature history in °C.
    pub temp: VecDeque<i64>,
    /// Total VRAM in bytes.
    pub mem_total: u64,
    /// Used VRAM in bytes.
    pub mem_used: u64,
    /// VRAM utilization percentage history.
    pub mem_utilization_percent: VecDeque<i64>,
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            stable_id: String::new(),
            name: String::new(),
            gpu_percent: GpuPercent::default(),
            gpu_clock_speed: 0,
            gpu_max_clock_speed: 0,
            pwr_usage: 0,
            pwr_max_usage: 255_000,
            temp: VecDeque::from([0]),
            mem_total: 0,
            mem_used: 0,
            mem_utilization_percent: VecDeque::from([0]),
        }
    }
}

impl GpuInfo {
    /// Hash representing the **values currently rendered** by
    /// [`crate::ui::gpu_widget`]. Used by the per-frame pull
    /// pipeline to decide which GPU widget instances actually
    /// need to redraw.
    ///
    /// Today's GPU widget renders only the **latest** sample of
    /// each history (`.back()`), plus the scalar fields. This
    /// fingerprint mirrors that contract: the time-series
    /// histories themselves are *not* hashed because two snapshots
    /// that share the same trailing sample render identically,
    /// even if the historical window has rolled forward.
    ///
    /// **MUST be updated** if [`crate::ui::gpu_widget::draw`]
    /// starts rendering additional fields (e.g. an in-cell
    /// history graph). Without that update the renderer would
    /// skip frames whose history changed but whose latest sample
    /// did not, producing a stale graph.
    pub fn render_fingerprint(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.name.hash(&mut hasher);
        self.gpu_percent
            .utilization
            .back()
            .copied()
            .hash(&mut hasher);
        self.gpu_percent.vram.back().copied().hash(&mut hasher);
        self.gpu_percent.power.back().copied().hash(&mut hasher);
        self.gpu_clock_speed.hash(&mut hasher);
        self.gpu_max_clock_speed.hash(&mut hasher);
        self.pwr_usage.hash(&mut hasher);
        self.pwr_max_usage.hash(&mut hasher);
        self.temp.back().copied().hash(&mut hasher);
        self.mem_total.hash(&mut hasher);
        self.mem_used.hash(&mut hasher);
        self.mem_utilization_percent
            .back()
            .copied()
            .hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_info_default_has_empty_percent() {
        let gpu = GpuInfo::default();
        assert!(gpu.gpu_percent.utilization.is_empty());
        assert!(gpu.gpu_percent.vram.is_empty());
        assert!(gpu.gpu_percent.power.is_empty());
    }

    fn make_gpu(util: i64, temp: i64, clock: u32, mem_used: u64) -> GpuInfo {
        let mut g = GpuInfo::default();
        g.gpu_percent.utilization.push_back(util);
        g.temp.push_back(temp);
        g.gpu_clock_speed = clock;
        g.mem_used = mem_used;
        g
    }

    #[test]
    fn render_fingerprint_changes_when_displayed_value_changes() {
        let a = make_gpu(50, 60, 1500, 1024 * 1024);
        let b = make_gpu(51, 60, 1500, 1024 * 1024);
        assert_ne!(
            a.render_fingerprint(),
            b.render_fingerprint(),
            "utilization change must shift the fingerprint",
        );
    }

    #[test]
    fn render_fingerprint_stable_for_equal_displayed_values() {
        let a = make_gpu(50, 60, 1500, 1024 * 1024);
        let b = make_gpu(50, 60, 1500, 1024 * 1024);
        assert_eq!(a.render_fingerprint(), b.render_fingerprint());
    }

    #[test]
    fn render_fingerprint_ignores_stale_history_window() {
        // Today's gpu_widget renders only `.back()` of every
        // history. Two snapshots whose latest samples are identical
        // but whose history windows differ MUST hash equal — the
        // user would see no visual difference.
        let mut a = make_gpu(50, 60, 1500, 1024 * 1024);
        a.gpu_percent.utilization.push_front(10);
        a.gpu_percent.utilization.push_front(20);
        let b = make_gpu(50, 60, 1500, 1024 * 1024);
        assert_eq!(
            a.render_fingerprint(),
            b.render_fingerprint(),
            "history-window roll must NOT shift the fingerprint",
        );
    }

    #[test]
    fn render_fingerprint_includes_temperature_changes() {
        let a = make_gpu(50, 60, 1500, 1024 * 1024);
        let b = make_gpu(50, 65, 1500, 1024 * 1024);
        assert_ne!(a.render_fingerprint(), b.render_fingerprint());
    }

    #[test]
    fn render_fingerprint_includes_mem_used_changes() {
        let a = make_gpu(50, 60, 1500, 1024 * 1024);
        let b = make_gpu(50, 60, 1500, 2 * 1024 * 1024);
        assert_ne!(a.render_fingerprint(), b.render_fingerprint());
    }
}
