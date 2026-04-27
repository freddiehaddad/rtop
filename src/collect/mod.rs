pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod process;

/// Trait for all data collectors.
///
/// Each collector implements `collect()` to perform one collection cycle,
/// updating its internal state. Data is accessed via the collector's
/// public fields, not through this trait.
pub trait Collector {
    /// Perform one data collection cycle.
    fn collect(&mut self);
}
