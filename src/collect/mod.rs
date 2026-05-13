pub(crate) mod counters;
pub mod cpu;
pub(crate) mod cpu_thermal;
pub mod disk;
pub mod gpu;
pub mod memory;
pub mod network;
pub(crate) mod pawnio;
pub mod process;
pub mod process_display;
pub mod statusbar;
pub(crate) mod win;

/// Health status of a collector after a collection cycle.
///
/// Each collector sets its `status` field during `collect()`:
/// - `Ok` — all data collected successfully (default, reset each cycle).
/// - `Degraded` — partial data (e.g. CPU% works but temperature is unavailable).
/// - `Failed` — no usable data collected this cycle (API failure, etc.).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum CollectStatus {
    #[default]
    Ok,
    Degraded(&'static str),
    Failed(&'static str),
}

impl CollectStatus {
    /// Severity rank for ordering: Ok(0) < Degraded(1) < Failed(2).
    fn rank(&self) -> u8 {
        match self {
            CollectStatus::Ok => 0,
            CollectStatus::Degraded(_) => 1,
            CollectStatus::Failed(_) => 2,
        }
    }

    /// Worsen the status if `new` is more severe than the current value.
    /// Never upgrades (e.g. Failed will not be overwritten by Degraded).
    pub fn downgrade(&mut self, new: CollectStatus) {
        if new.rank() > self.rank() {
            *self = new;
        }
    }
}

/// Trait for all data collectors.
///
/// Each collector implements `collect()` to perform one collection cycle,
/// updating its internal state, and `snapshot()` to produce the
/// publishable per-cycle snapshot value that
/// [`crate::runner::CollectorManager`] writes to its
/// [`crate::runner::LatestSlot`]. Tying the snapshot type to the
/// collector at the trait level lets the spawn site spell
/// `LatestSlot<C::Snapshot>` exactly once and removes the per-spawn
/// snapshot-construction closure that this trait used to require.
///
/// `pub(crate)` because the trait is used only within this binary
/// crate; it is never part of an externally-consumable surface.
/// The associated `Snapshot` type therefore can reference
/// `pub(crate)` snapshot structs in [`crate::runner`] without
/// leaking a more-private type through a more-public trait.
pub(crate) trait Collector {
    /// The published snapshot type for this collector. One per
    /// collector, fixed at the trait level so the publish slot
    /// (`LatestSlot<C::Snapshot>`) cannot drift from what the
    /// collector actually produces.
    ///
    /// The `Send + Sync + 'static` bound matches the storage
    /// requirement of [`crate::runner::LatestSlot`] (which wraps an
    /// `arc_swap::ArcSwapOption<T>` over an `Arc<T>`); pinning it
    /// here means individual collectors do not have to repeat it.
    type Snapshot: Send + Sync + 'static;

    /// Perform one data collection cycle.
    fn collect(&mut self);

    /// Build the publishable snapshot from the collector's current
    /// state. Pure; does not mutate `self`.
    fn snapshot(&self) -> Self::Snapshot;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_is_ok() {
        let status = CollectStatus::default();
        assert_eq!(status, CollectStatus::Ok);
    }

    #[test]
    fn downgrade_ok_to_degraded() {
        let mut status = CollectStatus::Ok;
        status.downgrade(CollectStatus::Degraded("partial"));
        assert_eq!(status, CollectStatus::Degraded("partial"));
    }

    #[test]
    fn downgrade_ok_to_failed() {
        let mut status = CollectStatus::Ok;
        status.downgrade(CollectStatus::Failed("total"));
        assert_eq!(status, CollectStatus::Failed("total"));
    }

    #[test]
    fn downgrade_degraded_to_failed() {
        let mut status = CollectStatus::Degraded("partial");
        status.downgrade(CollectStatus::Failed("total"));
        assert_eq!(status, CollectStatus::Failed("total"));
    }

    #[test]
    fn downgrade_does_not_upgrade_failed_to_degraded() {
        let mut status = CollectStatus::Failed("total");
        status.downgrade(CollectStatus::Degraded("partial"));
        assert_eq!(status, CollectStatus::Failed("total"));
    }

    #[test]
    fn downgrade_does_not_upgrade_failed_to_ok() {
        let mut status = CollectStatus::Failed("total");
        status.downgrade(CollectStatus::Ok);
        assert_eq!(status, CollectStatus::Failed("total"));
    }

    #[test]
    fn downgrade_does_not_upgrade_degraded_to_ok() {
        let mut status = CollectStatus::Degraded("partial");
        status.downgrade(CollectStatus::Ok);
        assert_eq!(status, CollectStatus::Degraded("partial"));
    }

    #[test]
    fn downgrade_same_rank_keeps_original() {
        let mut status = CollectStatus::Degraded("first");
        status.downgrade(CollectStatus::Degraded("second"));
        assert_eq!(status, CollectStatus::Degraded("first"));
    }

    #[test]
    fn rank_ordering() {
        assert!(CollectStatus::Ok.rank() < CollectStatus::Degraded("").rank());
        assert!(CollectStatus::Degraded("").rank() < CollectStatus::Failed("").rank());
    }
}
