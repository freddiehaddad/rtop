use crate::input;
use std::fmt;

/// Identifier for a data-collection subsystem.
///
/// Each subsystem corresponds to one collector thread and one
/// publish slot. `SubsystemKind` is the dispatch key used by
/// [`AppEvent::SubsystemReady`] and by per-subsystem plumbing in
/// `runner` / `app` so the same subsystems are no longer
/// enumerated by hand at every site.
///
/// The `Gpu(u8)` variant carries the device index — one collector
/// thread per discovered GPU. The index is purely an internal
/// addressing handle (collector wakeup, command channel routing);
/// the user-facing widget is a single cycling [`WidgetKind::Gpu`]
/// that selects which device's snapshot to render via stable
/// device IDs (see [`crate::app::GpuViewState`]).
///
/// [`WidgetKind::Gpu`]: crate::domain::widget_kind::WidgetKind::Gpu
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

impl SubsystemKind {
    /// Every subsystem variant in canonical order for a system
    /// with `gpu_count` discovered GPUs: the four base subsystems,
    /// then `Gpu(0..gpu_count)`, then `Proc`, then `Statusbar`.
    /// Iterate this instead of repeating the match by hand.
    pub(crate) fn all_for(gpu_count: u8) -> impl Iterator<Item = SubsystemKind> {
        const SCALAR_PREFIX: [SubsystemKind; 4] = [
            SubsystemKind::Cpu,
            SubsystemKind::Mem,
            SubsystemKind::Disk,
            SubsystemKind::Net,
        ];
        const SCALAR_SUFFIX: [SubsystemKind; 2] = [SubsystemKind::Proc, SubsystemKind::Statusbar];
        SCALAR_PREFIX
            .into_iter()
            .chain((0..gpu_count).map(SubsystemKind::Gpu))
            .chain(SCALAR_SUFFIX)
    }
}

/// Stable short name for diagnostics and tracing fields. The
/// scalar variants format as their interned constant; `Gpu(n)`
/// formats as `"gpu{n}"` (no allocation — `write!` streams the
/// integer directly into the formatter).
impl fmt::Display for SubsystemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubsystemKind::Cpu => f.write_str("cpu"),
            SubsystemKind::Mem => f.write_str("memory"),
            SubsystemKind::Disk => f.write_str("disk"),
            SubsystemKind::Net => f.write_str("network"),
            SubsystemKind::Gpu(n) => write!(f, "gpu{n}"),
            SubsystemKind::Proc => f.write_str("process"),
            SubsystemKind::Statusbar => f.write_str("statusbar"),
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
/// GPU slots are stored in a `Vec<T>` sized at construction by the
/// discovered device count; there is no compile-time cap on GPU
/// slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PerSubsystem<T> {
    cpu: T,
    mem: T,
    disk: T,
    net: T,
    gpu: Vec<T>,
    process: T,
    statusbar: T,
}

impl<T: Default> PerSubsystem<T> {
    /// Construct a `PerSubsystem<T>` with `T::default()` in every
    /// slot, including `gpu_count` GPU slots.
    pub(crate) fn with_default(gpu_count: u8) -> Self {
        Self {
            cpu: T::default(),
            mem: T::default(),
            disk: T::default(),
            net: T::default(),
            gpu: (0..gpu_count).map(|_| T::default()).collect(),
            process: T::default(),
            statusbar: T::default(),
        }
    }

    /// Overwrite every slot with `T::default()` in place. Used by
    /// the per-event-loop "ready" bitmap to drop the previous
    /// cycle's state without reallocating the GPU `Vec`.
    pub(crate) fn reset(&mut self) {
        self.cpu = T::default();
        self.mem = T::default();
        self.disk = T::default();
        self.net = T::default();
        for slot in &mut self.gpu {
            *slot = T::default();
        }
        self.process = T::default();
        self.statusbar = T::default();
    }
}

impl<T> PerSubsystem<T> {
    /// Construct from one value per subsystem, in `SubsystemKind`
    /// canonical order. The `gpu` parameter is a `Vec<T>` of
    /// length matching the discovered GPU count. Use this when
    /// `T` does not implement `Default` (e.g. `Sender<_>`).
    pub(crate) fn new(
        cpu: T,
        mem: T,
        disk: T,
        net: T,
        gpu: Vec<T>,
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

    /// Borrow the slot for `kind`. Panics if `kind` is `Gpu(n)`
    /// with `n >= gpu_count` — callers must respect the
    /// discovered device count.
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

    /// Mutably borrow the slot for `kind`. Panics if `kind` is
    /// `Gpu(n)` with `n >= gpu_count`.
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

    /// Test fixture: representative non-zero GPU count.
    const TEST_GPU_COUNT: u8 = 4;

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
    fn all_for_lists_every_variant_once_in_canonical_order() {
        let all: Vec<SubsystemKind> = SubsystemKind::all_for(TEST_GPU_COUNT).collect();
        assert_eq!(all.len(), TEST_GPU_COUNT as usize + 6);
        assert_eq!(all[0], SubsystemKind::Cpu);
        assert_eq!(all[1], SubsystemKind::Mem);
        assert_eq!(all[2], SubsystemKind::Disk);
        assert_eq!(all[3], SubsystemKind::Net);
        for n in 0..TEST_GPU_COUNT {
            assert_eq!(all[4 + n as usize], SubsystemKind::Gpu(n));
        }
        assert_eq!(all[4 + TEST_GPU_COUNT as usize], SubsystemKind::Proc);
        assert_eq!(all[5 + TEST_GPU_COUNT as usize], SubsystemKind::Statusbar);
    }

    #[test]
    fn all_for_zero_gpus_skips_gpu_variants() {
        let all: Vec<SubsystemKind> = SubsystemKind::all_for(0).collect();
        assert_eq!(all.len(), 6);
        assert!(!all.iter().any(|k| matches!(k, SubsystemKind::Gpu(_))));
    }

    #[test]
    fn per_subsystem_with_default_initialises_every_slot() {
        let p = PerSubsystem::<bool>::with_default(TEST_GPU_COUNT);
        for kind in SubsystemKind::all_for(TEST_GPU_COUNT) {
            assert!(!*p.get(kind));
        }
    }

    #[test]
    fn per_subsystem_reset_clears_all_slots_in_place() {
        let mut p = PerSubsystem::<bool>::with_default(TEST_GPU_COUNT);
        for kind in SubsystemKind::all_for(TEST_GPU_COUNT) {
            *p.get_mut(kind) = true;
        }
        p.reset();
        for kind in SubsystemKind::all_for(TEST_GPU_COUNT) {
            assert!(!*p.get(kind), "{kind:?} must be reset to default");
        }
    }

    #[test]
    fn per_subsystem_get_mut_writes_to_named_slot() {
        let mut p = PerSubsystem::<bool>::with_default(TEST_GPU_COUNT);
        for kind in SubsystemKind::all_for(TEST_GPU_COUNT) {
            assert!(!*p.get(kind));
            *p.get_mut(kind) = true;
            assert!(*p.get(kind));
        }
    }

    #[test]
    fn per_subsystem_gpu_slots_are_independent() {
        let mut p = PerSubsystem::<u32>::with_default(TEST_GPU_COUNT);
        for n in 0..TEST_GPU_COUNT {
            *p.get_mut(SubsystemKind::Gpu(n)) = 100 + n as u32;
        }
        for n in 0..TEST_GPU_COUNT {
            assert_eq!(*p.get(SubsystemKind::Gpu(n)), 100 + n as u32);
        }
    }

    #[test]
    fn per_subsystem_new_assigns_each_slot_in_canonical_order() {
        let gpu_values: Vec<&str> = (0..TEST_GPU_COUNT as usize)
            .map(|i| match i {
                0 => "gpu0",
                1 => "gpu1",
                2 => "gpu2",
                3 => "gpu3",
                _ => unreachable!("TEST_GPU_COUNT is 4"),
            })
            .collect();
        let p = PerSubsystem::new(
            "cpu",
            "mem",
            "disk",
            "net",
            gpu_values.clone(),
            "process",
            "statusbar",
        );
        assert_eq!(*p.get(SubsystemKind::Cpu), "cpu");
        assert_eq!(*p.get(SubsystemKind::Mem), "mem");
        assert_eq!(*p.get(SubsystemKind::Disk), "disk");
        assert_eq!(*p.get(SubsystemKind::Net), "net");
        for n in 0..TEST_GPU_COUNT {
            assert_eq!(*p.get(SubsystemKind::Gpu(n)), gpu_values[n as usize]);
        }
        assert_eq!(*p.get(SubsystemKind::Proc), "process");
        assert_eq!(*p.get(SubsystemKind::Statusbar), "statusbar");
    }

    #[test]
    fn subsystem_kind_display_matches_tracing_targets() {
        assert_eq!(SubsystemKind::Cpu.to_string(), "cpu");
        assert_eq!(SubsystemKind::Mem.to_string(), "memory");
        assert_eq!(SubsystemKind::Disk.to_string(), "disk");
        assert_eq!(SubsystemKind::Net.to_string(), "network");
        assert_eq!(SubsystemKind::Proc.to_string(), "process");
        assert_eq!(SubsystemKind::Statusbar.to_string(), "statusbar");
        for n in [0_u8, 1, 7, 31, 255] {
            assert_eq!(SubsystemKind::Gpu(n).to_string(), format!("gpu{n}"));
        }
    }
}
