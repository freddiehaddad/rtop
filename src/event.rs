use crate::config::MAX_GPUS;
use crate::input;

/// Identifier for a data-collection subsystem.
///
/// Each subsystem corresponds to one collector thread and one
/// publish slot. `SubsystemKind` is the dispatch key used by
/// [`AppEvent::SubsystemReady`] and by per-subsystem plumbing in
/// `runner` / `app` so the same subsystems are no longer
/// enumerated by hand at every site.
///
/// The GPU variant carries the device index (`0..MAX_GPUS`) so the
/// per-device collector thread layer (one thread per detected GPU)
/// can route ready events and command channels per device.
/// Construction sites must always use indices in `0..MAX_GPUS`;
/// the [`Self::as_str`] fallback documents what happens if that
/// invariant is violated.
///
/// This is distinct from `crate::domain::widget_kind::WidgetKind`,
/// which identifies a *render-side* widget. The two universes are
/// now isomorphic for GPUs (`SubsystemKind::Gpu(n)` ↔
/// `WidgetKind::Gpu(n)`); the distinction remains for the
/// non-GPU subsystems (e.g. one CPU subsystem produces data for
/// the single CPU widget; one process subsystem produces data for
/// the single process widget).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SubsystemKind {
    Cpu,
    Mem,
    Disk,
    Net,
    Gpu(u8),
    Proc,
    Statusbar,
}

/// Stable interned names for `SubsystemKind::Gpu(n)` where
/// `n < MAX_GPUS`. Indexed by the variant payload in
/// [`SubsystemKind::as_str`].
const GPU_SUBSYSTEM_NAMES: [&str; MAX_GPUS] = [
    "gpu0", "gpu1", "gpu2", "gpu3", "gpu4", "gpu5", "gpu6", "gpu7",
];

const _: () = {
    // Pin the table length to MAX_GPUS so a future MAX_GPUS bump
    // fails to compile here until the table is extended.
    assert!(GPU_SUBSYSTEM_NAMES.len() == MAX_GPUS);
};

impl SubsystemKind {
    /// Every subsystem variant in canonical order: the four base
    /// subsystems, then `Gpu(0..MAX_GPUS)`, then `Proc`, then
    /// `Statusbar`. Iterate this slice instead of repeating the
    /// match by hand.
    ///
    /// The GPU range is materialised at compile time via a `const`
    /// initialiser so the slice is a true `[SubsystemKind;
    /// MAX_GPUS + 6]` literal — same shape as the original
    /// `[SubsystemKind; 7]` ALL just sized for the per-device
    /// expansion.
    pub(crate) const ALL: [SubsystemKind; MAX_GPUS + 6] = {
        let mut arr = [SubsystemKind::Cpu; MAX_GPUS + 6];
        arr[0] = SubsystemKind::Cpu;
        arr[1] = SubsystemKind::Mem;
        arr[2] = SubsystemKind::Disk;
        arr[3] = SubsystemKind::Net;
        let mut i = 0;
        while i < MAX_GPUS {
            arr[4 + i] = SubsystemKind::Gpu(i as u8);
            i += 1;
        }
        arr[4 + MAX_GPUS] = SubsystemKind::Proc;
        arr[5 + MAX_GPUS] = SubsystemKind::Statusbar;
        arr
    };

    /// Stable short name for diagnostics and tracing fields.
    /// `Gpu(n)` returns the interned `"gpuN"` string from
    /// [`GPU_SUBSYSTEM_NAMES`]; an out-of-range payload (which
    /// can only occur if a caller constructs `Gpu(n)` directly
    /// with `n >= MAX_GPUS`) returns `"gpu?"` so the diagnostic
    /// stays printable.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SubsystemKind::Cpu => "cpu",
            SubsystemKind::Mem => "memory",
            SubsystemKind::Disk => "disk",
            SubsystemKind::Net => "network",
            SubsystemKind::Gpu(n) => GPU_SUBSYSTEM_NAMES
                .get(n as usize)
                .copied()
                .unwrap_or("gpu?"),
            SubsystemKind::Proc => "process",
            SubsystemKind::Statusbar => "statusbar",
        }
    }
}

/// Typed indexed container with one slot per [`SubsystemKind`].
///
/// Used where a value of uniform type `T` exists per subsystem —
/// today, the per-cycle "ready" bools and the per-collector
/// command channels. The exhaustive `match` in the accessors is
/// checked by the compiler: adding a variant to [`SubsystemKind`]
/// forces every `PerSubsystem<T>` accessor to be updated.
///
/// GPU slots are indexed by the `Gpu(u8)` payload via a
/// fixed-size `[T; MAX_GPUS]` array — same shape as
/// [`crate::domain::widget_kind::PerWidget`] uses for its GPU
/// widgets, giving constant-time lookup without runtime allocation
/// or hashing.
///
/// Heterogeneous per-subsystem data (e.g. typed snapshot slots
/// where each subsystem has a different concrete type) keeps
/// using explicit per-subsystem fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PerSubsystem<T> {
    cpu: T,
    mem: T,
    disk: T,
    net: T,
    gpu: [T; MAX_GPUS],
    process: T,
    statusbar: T,
}

impl<T> PerSubsystem<T> {
    /// Construct from one value per subsystem, in `SubsystemKind`
    /// canonical order. The `gpu` parameter is a fixed-size array
    /// indexed by `Gpu(n)` payload. Use this when `T` does not
    /// implement `Default` (e.g. `Sender<_>`).
    pub(crate) fn new(
        cpu: T,
        mem: T,
        disk: T,
        net: T,
        gpu: [T; MAX_GPUS],
        process: T,
        statusbar: T,
    ) -> Self {
        Self {
            cpu,
            mem,
            disk,
            net,
            gpu,
            process,
            statusbar,
        }
    }

    pub(crate) fn get(&self, kind: SubsystemKind) -> &T {
        match kind {
            SubsystemKind::Cpu => &self.cpu,
            SubsystemKind::Mem => &self.mem,
            SubsystemKind::Disk => &self.disk,
            SubsystemKind::Net => &self.net,
            SubsystemKind::Gpu(n) => &self.gpu[n as usize],
            SubsystemKind::Proc => &self.process,
            SubsystemKind::Statusbar => &self.statusbar,
        }
    }

    pub(crate) fn get_mut(&mut self, kind: SubsystemKind) -> &mut T {
        match kind {
            SubsystemKind::Cpu => &mut self.cpu,
            SubsystemKind::Mem => &mut self.mem,
            SubsystemKind::Disk => &mut self.disk,
            SubsystemKind::Net => &mut self.net,
            SubsystemKind::Gpu(n) => &mut self.gpu[n as usize],
            SubsystemKind::Proc => &mut self.process,
            SubsystemKind::Statusbar => &mut self.statusbar,
        }
    }
}

/// Events processed by the main event loop.
///
/// All event sources (input thread, collector threads) send through
/// a single `mpsc::Sender<AppEvent>` channel. The main loop blocks
/// on the receiver, processes events, and renders dirty widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppEvent {
    /// A key was pressed (from input thread).
    Key(input::Key),
    /// Terminal was resized (from input thread).
    Resize,
    /// New data is available in the named subsystem's publish slot.
    SubsystemReady(SubsystemKind),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn events_roundtrip_through_channel() {
        let (tx, rx) = mpsc::channel();

        tx.send(AppEvent::Key(input::Key::Char('q'))).unwrap();
        tx.send(AppEvent::Resize).unwrap();
        tx.send(AppEvent::SubsystemReady(SubsystemKind::Cpu))
            .unwrap();
        tx.send(AppEvent::SubsystemReady(SubsystemKind::Proc))
            .unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('q'))
        ));
        assert!(matches!(rx.recv().unwrap(), AppEvent::Resize));
        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::SubsystemReady(SubsystemKind::Cpu)
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::SubsystemReady(SubsystemKind::Proc)
        ));
    }

    #[test]
    fn sender_clone_allows_multiple_producers() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();

        tx.send(AppEvent::Key(input::Key::Char('a'))).unwrap();
        tx2.send(AppEvent::SubsystemReady(SubsystemKind::Mem))
            .unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('a'))
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::SubsystemReady(SubsystemKind::Mem)
        ));
    }

    #[test]
    fn try_recv_drains_queued_events() {
        let (tx, rx) = mpsc::channel();

        tx.send(AppEvent::Key(input::Key::Up)).unwrap();
        tx.send(AppEvent::Key(input::Key::Down)).unwrap();
        tx.send(AppEvent::SubsystemReady(SubsystemKind::Cpu))
            .unwrap();

        let first = rx.recv().unwrap();
        let rest: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();

        assert!(matches!(first, AppEvent::Key(input::Key::Up)));
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn app_event_is_copy() {
        let event = AppEvent::SubsystemReady(SubsystemKind::Cpu);
        let copy = event;
        assert_eq!(event, copy);
    }

    #[test]
    fn subsystem_kind_all_lists_every_variant_once() {
        let all = SubsystemKind::ALL;
        // 4 base subsystems (cpu/mem/disk/net) + MAX_GPUS GPU
        // variants (Gpu(0)..Gpu(MAX_GPUS-1)) + 2 trailing
        // (proc/statusbar) = MAX_GPUS + 6 entries.
        assert_eq!(all.len(), MAX_GPUS + 6);
        for kind in [
            SubsystemKind::Cpu,
            SubsystemKind::Mem,
            SubsystemKind::Disk,
            SubsystemKind::Net,
            SubsystemKind::Proc,
            SubsystemKind::Statusbar,
        ] {
            assert_eq!(all.iter().filter(|k| **k == kind).count(), 1);
        }
        for n in 0..MAX_GPUS as u8 {
            assert_eq!(
                all.iter().filter(|k| **k == SubsystemKind::Gpu(n)).count(),
                1,
                "Gpu({n}) must appear exactly once in ALL",
            );
        }
    }

    #[test]
    fn subsystem_kind_all_canonical_ordering() {
        // Pin the canonical order: cpu/mem/disk/net first, then
        // Gpu(0..MAX_GPUS) in index order, then proc/statusbar.
        let all = SubsystemKind::ALL;
        assert_eq!(all[0], SubsystemKind::Cpu);
        assert_eq!(all[1], SubsystemKind::Mem);
        assert_eq!(all[2], SubsystemKind::Disk);
        assert_eq!(all[3], SubsystemKind::Net);
        for n in 0..MAX_GPUS as u8 {
            assert_eq!(all[4 + n as usize], SubsystemKind::Gpu(n));
        }
        assert_eq!(all[4 + MAX_GPUS], SubsystemKind::Proc);
        assert_eq!(all[5 + MAX_GPUS], SubsystemKind::Statusbar);
    }

    #[test]
    fn per_subsystem_default_is_default_for_every_variant() {
        let p = PerSubsystem::<bool>::default();
        for kind in SubsystemKind::ALL {
            assert!(!*p.get(kind));
        }
    }

    #[test]
    fn per_subsystem_get_mut_writes_to_named_slot() {
        let mut p = PerSubsystem::<bool>::default();
        for kind in SubsystemKind::ALL {
            assert!(!*p.get(kind));
            *p.get_mut(kind) = true;
            assert!(*p.get(kind));
        }
        // Every slot is independently set.
        for kind in SubsystemKind::ALL {
            assert!(*p.get(kind));
        }
    }

    #[test]
    fn per_subsystem_slots_are_independent() {
        let mut p = PerSubsystem::<u32>::default();
        for (i, kind) in SubsystemKind::ALL.iter().enumerate() {
            *p.get_mut(*kind) = i as u32 + 1;
        }
        for (i, kind) in SubsystemKind::ALL.iter().enumerate() {
            assert_eq!(*p.get(*kind), i as u32 + 1);
        }
    }

    #[test]
    fn per_subsystem_gpu_slots_are_addressable_by_index() {
        // Mirrors the per-widget GPU indexing test in
        // domain::widget_kind: writing only one GPU index leaves
        // the others untouched.
        let mut p = PerSubsystem::<u32>::default();
        for n in 0..MAX_GPUS as u8 {
            *p.get_mut(SubsystemKind::Gpu(n)) = (100 + n) as u32;
        }
        for n in 0..MAX_GPUS as u8 {
            assert_eq!(*p.get(SubsystemKind::Gpu(n)), (100 + n) as u32);
        }
        // Sparse writes preserve identity: writing only Gpu(2)
        // leaves Gpu(0) and Gpu(1) untouched.
        let mut q = PerSubsystem::<u32>::default();
        *q.get_mut(SubsystemKind::Gpu(2)) = 42;
        assert_eq!(*q.get(SubsystemKind::Gpu(0)), 0);
        assert_eq!(*q.get(SubsystemKind::Gpu(1)), 0);
        assert_eq!(*q.get(SubsystemKind::Gpu(2)), 42);
    }

    #[test]
    fn per_subsystem_gpu_does_not_alias_base_slots() {
        let mut p = PerSubsystem::<u32>::default();
        *p.get_mut(SubsystemKind::Cpu) = 1;
        *p.get_mut(SubsystemKind::Gpu(0)) = 2;
        assert_eq!(*p.get(SubsystemKind::Cpu), 1);
        assert_eq!(*p.get(SubsystemKind::Gpu(0)), 2);
    }

    #[test]
    fn per_subsystem_new_assigns_each_slot_in_canonical_order() {
        // The gpu parameter is a fixed-size array indexed by
        // Gpu(n) payload. Build it with distinguishable values
        // so each slot is verifiable.
        let gpu_values: [&str; MAX_GPUS] = std::array::from_fn(|i| match i {
            0 => "gpu0",
            1 => "gpu1",
            2 => "gpu2",
            3 => "gpu3",
            4 => "gpu4",
            5 => "gpu5",
            6 => "gpu6",
            7 => "gpu7",
            _ => unreachable!("MAX_GPUS is 8 by const_assert at top of crate"),
        });
        let p = PerSubsystem::new(
            "cpu",
            "mem",
            "disk",
            "net",
            gpu_values,
            "process",
            "statusbar",
        );
        assert_eq!(*p.get(SubsystemKind::Cpu), "cpu");
        assert_eq!(*p.get(SubsystemKind::Mem), "mem");
        assert_eq!(*p.get(SubsystemKind::Disk), "disk");
        assert_eq!(*p.get(SubsystemKind::Net), "net");
        for n in 0..MAX_GPUS as u8 {
            assert_eq!(*p.get(SubsystemKind::Gpu(n)), gpu_values[n as usize]);
        }
        assert_eq!(*p.get(SubsystemKind::Proc), "process");
        assert_eq!(*p.get(SubsystemKind::Statusbar), "statusbar");
    }

    #[test]
    fn subsystem_kind_as_str_matches_tracing_targets() {
        assert_eq!(SubsystemKind::Cpu.as_str(), "cpu");
        assert_eq!(SubsystemKind::Mem.as_str(), "memory");
        assert_eq!(SubsystemKind::Disk.as_str(), "disk");
        assert_eq!(SubsystemKind::Net.as_str(), "network");
        assert_eq!(SubsystemKind::Proc.as_str(), "process");
        assert_eq!(SubsystemKind::Statusbar.as_str(), "statusbar");
        for n in 0..MAX_GPUS as u8 {
            let expected = match n {
                0 => "gpu0",
                1 => "gpu1",
                2 => "gpu2",
                3 => "gpu3",
                4 => "gpu4",
                5 => "gpu5",
                6 => "gpu6",
                7 => "gpu7",
                _ => unreachable!("MAX_GPUS is 8 by const_assert at top of crate"),
            };
            assert_eq!(SubsystemKind::Gpu(n).as_str(), expected);
        }
    }

    #[test]
    fn subsystem_kind_as_str_falls_back_for_out_of_range_gpu_payload() {
        // `SubsystemKind::Gpu(n)` with n >= MAX_GPUS should not
        // panic; it returns the printable fallback "gpu?" so a
        // misbehaving construction is debuggable rather than
        // process-fatal.
        assert_eq!(SubsystemKind::Gpu(MAX_GPUS as u8).as_str(), "gpu?");
        assert_eq!(SubsystemKind::Gpu(255).as_str(), "gpu?");
    }
}
