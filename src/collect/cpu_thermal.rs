//! CPU temperature and power collection via PawnIO.
//!
//! This module owns the vendor-specific MSR / SMN sequences that read CPU
//! temperature and RAPL energy counters. The collector dispatches at runtime
//! based on the detected CPU vendor (Intel / AMD Family 17h+) and falls back
//! to producing no data when PawnIO is unavailable.

use crate::collect::pawnio::{AffinityGuard, Error as PawnError, Module, PawnIo};
use std::arch::x86_64::__cpuid;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Intel MSRs
// ---------------------------------------------------------------------------

/// Per-core thermal status: bits[22:16] = distance from TjMax in °C.
const MSR_IA32_THERM_STATUS: u32 = 0x019C;
/// Package thermal status: same encoding as IA32_THERM_STATUS.
const MSR_IA32_PACKAGE_THERM_STATUS: u32 = 0x01B1;
/// Per-core TjMax target: bits[23:16] = TjMax in °C.
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x01A2;
/// RAPL power unit: bits[12:8] = ESU (energy scale unit).
const MSR_RAPL_POWER_UNIT: u32 = 0x0606;
/// Package energy counter (32-bit, monotonic, wraps).
const MSR_PKG_ENERGY_STATUS: u32 = 0x0611;

/// Bit 31 of IA32_THERM_STATUS — set when reading is valid.
const THERM_STATUS_VALID: u64 = 1 << 31;
/// Mask for distance-from-TjMax bits in IA32_THERM_STATUS [22:16].
const THERM_DISTANCE_MASK: u64 = 0x7F << 16;
/// Mask for TjMax in IA32_TEMPERATURE_TARGET bits [23:16].
const TEMP_TARGET_TJMAX_MASK: u64 = 0xFF << 16;
/// Mask for energy unit in MSR_RAPL_POWER_UNIT bits [12:8].
const RAPL_ENERGY_UNIT_MASK: u64 = 0x1F << 8;

// ---------------------------------------------------------------------------
// AMD Family 17h+ MSRs and SMN registers
// ---------------------------------------------------------------------------

/// Power unit MSR (AMD): bits[12:8] = ESU.
const MSR_AMD_PWR_UNIT: u32 = 0xC001_0299;
/// Package energy status (AMD): 32-bit monotonic counter, wraps.
const MSR_AMD_PKG_ENERGY_STAT: u32 = 0xC001_029B;
/// SMN address of THM_TCON_CUR_TMP for AMD Family 17h+.
const SMN_THM_TCON_CUR_TMP: u32 = 0x0005_9800;
/// Bit 19 of THM_TCON_CUR_TMP: 1 = -49°C offset applies.
const THM_TEMP_RANGE_SEL: u32 = 1 << 19;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// One sample of CPU thermal and power data.
#[derive(Default, Debug, Clone)]
pub struct ThermalSample {
    /// Package temperature in degrees Celsius.
    pub package_temp: Option<i64>,
    /// Per-core temperatures in degrees Celsius (index = logical core).
    pub core_temps: Vec<i64>,
    /// Current CPU package power in watts.
    pub watts: Option<f64>,
    /// Maximum (TDP) CPU package power in watts.
    pub max_watts: Option<f64>,
}

/// CPU temperature and power collector backed by PawnIO.
///
/// Construct with [`ThermalCollector::detect`]; if PawnIO is unavailable or
/// the CPU vendor is unsupported, the resulting collector is inactive and
/// returns empty samples.
pub struct ThermalCollector {
    inner: Backend,
}

impl Default for ThermalCollector {
    /// Returns an inactive collector. Use [`ThermalCollector::detect`] to
    /// probe for PawnIO and load a vendor-specific module.
    fn default() -> Self {
        Self {
            inner: Backend::Inactive,
        }
    }
}

enum Backend {
    Inactive,
    Intel(IntelBackend),
    Amd(AmdBackend),
}

impl ThermalCollector {
    /// Detect the CPU vendor and load the matching PawnIO module.
    pub fn detect(core_count: usize) -> Self {
        let backend = match cpu_vendor() {
            CpuVendor::Intel => IntelBackend::load(core_count)
                .map(|b| {
                    tracing::debug!("Intel thermal init: {} cores", b.tj_max.len());
                    Backend::Intel(b)
                })
                .unwrap_or_else(|e| {
                    tracing::debug!("Intel thermal init failed: {e}");
                    Backend::Inactive
                }),
            CpuVendor::Amd => AmdBackend::load()
                .map(|b| {
                    tracing::debug!("AMD thermal init succeeded");
                    Backend::Amd(b)
                })
                .unwrap_or_else(|e| {
                    tracing::debug!("AMD thermal init failed: {e}");
                    Backend::Inactive
                }),
            CpuVendor::Other => Backend::Inactive,
        };
        Self { inner: backend }
    }

    /// Returns true if the collector is producing thermal/power data.
    pub fn is_active(&self) -> bool {
        !matches!(self.inner, Backend::Inactive)
    }

    /// Read one cycle of thermal/power data.
    pub fn sample(&mut self) -> ThermalSample {
        match &mut self.inner {
            Backend::Inactive => ThermalSample::default(),
            Backend::Intel(intel) => intel.sample(),
            Backend::Amd(amd) => amd.sample(),
        }
    }
}

// ---------------------------------------------------------------------------
// CPU vendor detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum CpuVendor {
    Intel,
    Amd,
    Other,
}

fn cpu_vendor() -> CpuVendor {
    // __cpuid leaf 0 is universally supported on x86_64 and is a safe intrinsic.
    let leaf0 = __cpuid(0);
    let mut vendor = [0u8; 12];
    vendor[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
    match &vendor {
        b"GenuineIntel" => CpuVendor::Intel,
        b"AuthenticAMD" => CpuVendor::Amd,
        _ => CpuVendor::Other,
    }
}

// ---------------------------------------------------------------------------
// Intel backend
// ---------------------------------------------------------------------------

struct IntelBackend {
    pawnio: PawnIo,
    /// Per-logical-core TjMax values (°C). Hybrid CPUs (P/E cores) have
    /// different TjMax values across cores.
    tj_max: Vec<u32>,
    /// Energy unit in joules per RAPL counter increment.
    energy_unit_j: f64,
    /// Energy state for power differentiation.
    energy_state: Option<EnergyState>,
    /// Highest package power observed in watts since the collector started.
    /// Reported as `max_watts` so the meter scales like LHM/HWiNFO/GPU-Z,
    /// which all show running peak rather than a static spec value.
    peak_watts: Option<f64>,
    /// True until the first successful sample is logged at debug level.
    first_sample_pending: bool,
}

impl IntelBackend {
    fn load(core_count: usize) -> Result<Self, PawnError> {
        let pawnio = PawnIo::load(Module::IntelMsr)?;

        // Read TjMax per core (with thread affinity) — hybrid CPUs have
        // different TjMax for P-cores and E-cores.
        let tj_max = (0..core_count)
            .map(|core| read_intel_tj_max(&pawnio, core).unwrap_or(100))
            .collect();

        // RAPL energy unit is package-wide — read once on the current core.
        let unit_raw = pawnio.read_msr(MSR_RAPL_POWER_UNIT)?;
        let energy_esu = ((unit_raw & RAPL_ENERGY_UNIT_MASK) >> 8) as u32;
        let energy_unit_j = 1.0 / ((1u64 << energy_esu) as f64);

        Ok(Self {
            pawnio,
            tj_max,
            energy_unit_j,
            energy_state: None,
            peak_watts: None,
            first_sample_pending: true,
        })
    }

    fn sample(&mut self) -> ThermalSample {
        let core_temps: Vec<i64> = self
            .tj_max
            .iter()
            .enumerate()
            .map(|(core, &tj)| read_intel_core_temp(&self.pawnio, core, tj))
            .map(|temp| temp.unwrap_or(0))
            .collect();

        let package_temp = self
            .pawnio
            .read_msr(MSR_IA32_PACKAGE_THERM_STATUS)
            .ok()
            .and_then(|raw| decode_intel_temp(raw, *self.tj_max.first().unwrap_or(&100)));

        let watts = self
            .pawnio
            .read_msr(MSR_PKG_ENERGY_STATUS)
            .ok()
            .and_then(|raw| {
                let now = Instant::now();
                let counter = (raw & 0xFFFF_FFFF) as u32;
                let watts = self
                    .energy_state
                    .as_ref()
                    .and_then(|prev| compute_watts(prev, counter, now, self.energy_unit_j));
                self.energy_state = Some(EnergyState { counter, when: now });
                watts
            });

        if let Some(w) = watts {
            self.peak_watts = Some(self.peak_watts.map_or(w, |p| p.max(w)));
        }

        if self.first_sample_pending && watts.is_some() {
            tracing::debug!(
                "Intel thermal first sample: package={:?} °C, watts={:.1}",
                package_temp,
                watts.unwrap_or(0.0)
            );
            self.first_sample_pending = false;
        }

        ThermalSample {
            package_temp,
            core_temps,
            watts,
            max_watts: self.peak_watts,
        }
    }
}

fn read_intel_tj_max(pawnio: &PawnIo, core: usize) -> Result<u32, PawnError> {
    let _guard = AffinityGuard::pin(core_group(core), core_mask(core))?;
    let raw = pawnio.read_msr(MSR_IA32_TEMPERATURE_TARGET)?;
    Ok(((raw & TEMP_TARGET_TJMAX_MASK) >> 16) as u32)
}

fn read_intel_core_temp(pawnio: &PawnIo, core: usize, tj_max: u32) -> Option<i64> {
    let _guard = AffinityGuard::pin(core_group(core), core_mask(core)).ok()?;
    let raw = pawnio.read_msr(MSR_IA32_THERM_STATUS).ok()?;
    decode_intel_temp(raw, tj_max)
}

fn decode_intel_temp(raw: u64, tj_max: u32) -> Option<i64> {
    if raw & THERM_STATUS_VALID == 0 {
        return None;
    }
    let distance = ((raw & THERM_DISTANCE_MASK) >> 16) as i64;
    Some(tj_max as i64 - distance)
}

// ---------------------------------------------------------------------------
// AMD backend
// ---------------------------------------------------------------------------

struct AmdBackend {
    pawnio: PawnIo,
    /// Energy unit in joules per RAPL counter increment.
    energy_unit_j: f64,
    /// Energy state for power differentiation.
    energy_state: Option<EnergyState>,
    /// Highest package power observed in watts since the collector started.
    /// Reported as `max_watts` so the meter scales like LHM/HWiNFO/GPU-Z,
    /// which all show running peak rather than a static spec value.
    peak_watts: Option<f64>,
    /// True until the first successful sample is logged at debug level.
    first_sample_pending: bool,
}

impl AmdBackend {
    fn load() -> Result<Self, PawnError> {
        let pawnio = PawnIo::load(Module::AmdFamily17)?;
        let unit_raw = pawnio.read_msr(MSR_AMD_PWR_UNIT)?;
        let energy_esu = ((unit_raw & RAPL_ENERGY_UNIT_MASK) >> 8) as u32;
        let energy_unit_j = 1.0 / ((1u64 << energy_esu) as f64);
        Ok(Self {
            pawnio,
            energy_unit_j,
            energy_state: None,
            peak_watts: None,
            first_sample_pending: true,
        })
    }

    fn sample(&mut self) -> ThermalSample {
        let package_temp = self
            .pawnio
            .read_smn(SMN_THM_TCON_CUR_TMP)
            .ok()
            .map(decode_amd_temp);

        let watts = self
            .pawnio
            .read_msr(MSR_AMD_PKG_ENERGY_STAT)
            .ok()
            .and_then(|raw| {
                let now = Instant::now();
                let counter = (raw & 0xFFFF_FFFF) as u32;
                let watts = self
                    .energy_state
                    .as_ref()
                    .and_then(|prev| compute_watts(prev, counter, now, self.energy_unit_j));
                self.energy_state = Some(EnergyState { counter, when: now });
                watts
            });

        if let Some(w) = watts {
            self.peak_watts = Some(self.peak_watts.map_or(w, |p| p.max(w)));
        }

        if self.first_sample_pending && watts.is_some() {
            tracing::debug!(
                "AMD thermal first sample: package={:?} °C, watts={:.1}",
                package_temp,
                watts.unwrap_or(0.0)
            );
            self.first_sample_pending = false;
        }

        ThermalSample {
            package_temp,
            core_temps: Vec::new(),
            watts,
            max_watts: self.peak_watts,
        }
    }
}

fn decode_amd_temp(raw: u32) -> i64 {
    let mut temp_c = ((raw >> 21) as f64) * 0.125;
    if raw & THM_TEMP_RANGE_SEL != 0 {
        temp_c -= 49.0;
    }
    temp_c.round() as i64
}

// ---------------------------------------------------------------------------
// Shared RAPL energy differentiation
// ---------------------------------------------------------------------------

struct EnergyState {
    counter: u32,
    when: Instant,
}

/// Compute average watts since the previous sample using the wrapping delta
/// between two 32-bit RAPL energy counters.
fn compute_watts(
    prev: &EnergyState,
    counter: u32,
    now: Instant,
    energy_unit_j: f64,
) -> Option<f64> {
    let dt = now.saturating_duration_since(prev.when).as_secs_f64();
    if dt <= 0.0 {
        return None;
    }
    let delta = counter.wrapping_sub(prev.counter) as f64;
    Some(delta * energy_unit_j / dt)
}

// ---------------------------------------------------------------------------
// Processor group / mask helpers (Windows supports >64 logical CPUs split
// across multiple processor groups).
// ---------------------------------------------------------------------------

/// Return the processor group containing logical core `index`.
fn core_group(index: usize) -> u16 {
    (index / 64) as u16
}

/// Return the affinity mask for logical core `index` within its group.
fn core_mask(index: usize) -> usize {
    1usize << (index % 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_temp_decode_subtracts_distance_from_tj_max() {
        // Valid bit set, distance = 15
        let raw: u64 = THERM_STATUS_VALID | (15u64 << 16);
        assert_eq!(decode_intel_temp(raw, 100), Some(85));
    }

    #[test]
    fn intel_temp_decode_returns_none_when_valid_bit_clear() {
        let raw: u64 = 15u64 << 16; // distance present but invalid
        assert_eq!(decode_intel_temp(raw, 100), None);
    }

    #[test]
    fn amd_temp_decode_normal_range() {
        // (4 << 21) >> 21 = 4 → 4 × 0.125 = 0.5 → round to 1
        let raw: u32 = 4 << 21;
        assert_eq!(decode_amd_temp(raw), 1);
    }

    #[test]
    fn amd_temp_decode_with_offset_subtracts_49() {
        // value = 600 → 600 × 0.125 = 75°C, offset → 26°C
        let raw: u32 = (600u32 << 21) | THM_TEMP_RANGE_SEL;
        assert_eq!(decode_amd_temp(raw), 26);
    }

    #[test]
    fn compute_watts_handles_wrapping_counter() {
        // Counter wrapped from 0xFFFF_FFFE to 4 (delta = 6)
        let prev = EnergyState {
            counter: 0xFFFF_FFFE,
            when: Instant::now(),
        };
        let now = prev.when + std::time::Duration::from_secs(1);
        let unit = 1.0 / 16384.0;
        let watts = compute_watts(&prev, 4, now, unit).unwrap();
        // 6 * (1/16384) / 1 = ~0.000366
        assert!((watts - 6.0 * unit).abs() < 1e-9);
    }

    #[test]
    fn compute_watts_returns_none_for_zero_duration() {
        let when = Instant::now();
        let prev = EnergyState { counter: 0, when };
        assert!(compute_watts(&prev, 100, when, 1.0).is_none());
    }

    #[test]
    fn core_group_splits_at_64() {
        assert_eq!(core_group(0), 0);
        assert_eq!(core_group(63), 0);
        assert_eq!(core_group(64), 1);
        assert_eq!(core_group(127), 1);
        assert_eq!(core_group(128), 2);
    }

    #[test]
    fn core_mask_bit_within_group() {
        assert_eq!(core_mask(0), 1);
        assert_eq!(core_mask(1), 2);
        assert_eq!(core_mask(63), 1usize << 63);
        assert_eq!(core_mask(64), 1);
    }
}
