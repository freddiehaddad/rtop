use crate::input;

/// Identifier for a data-collection subsystem.
///
/// Each subsystem corresponds to one collector thread and one
/// publish slot. `SubsystemKind` is the dispatch key used by
/// [`AppEvent::SubsystemReady`] and by per-subsystem plumbing in
/// `runner` / `app` so the same six subsystems are no longer
/// enumerated by hand at every site.
///
/// This is distinct from `crate::domain::widget_kind::WidgetKind`,
/// which identifies a *render-side* widget. One subsystem can drive
/// many widgets — notably `SubsystemKind::Gpu` produces data for
/// every `WidgetKind::Gpu(n)` instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SubsystemKind {
    Cpu,
    Mem,
    Disk,
    Net,
    Gpu,
    Proc,
}

impl SubsystemKind {
    /// Every subsystem variant in declaration order. Iterate this
    /// slice instead of repeating the six-way match by hand.
    pub(crate) const ALL: [SubsystemKind; 6] = [
        SubsystemKind::Cpu,
        SubsystemKind::Mem,
        SubsystemKind::Disk,
        SubsystemKind::Net,
        SubsystemKind::Gpu,
        SubsystemKind::Proc,
    ];
}

/// Typed indexed container with one slot per [`SubsystemKind`].
///
/// Used where a value of uniform type `T` exists per subsystem —
/// today, the per-cycle "ready" bools and (in upcoming work)
/// command channels and dirty flags. The exhaustive `match` in
/// the accessors is checked by the compiler: adding a variant to
/// [`SubsystemKind`] forces every `PerSubsystem<T>` accessor to
/// be updated.
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
    gpu: T,
    process: T,
}

impl<T> PerSubsystem<T> {
    pub(crate) fn get(&self, kind: SubsystemKind) -> &T {
        match kind {
            SubsystemKind::Cpu => &self.cpu,
            SubsystemKind::Mem => &self.mem,
            SubsystemKind::Disk => &self.disk,
            SubsystemKind::Net => &self.net,
            SubsystemKind::Gpu => &self.gpu,
            SubsystemKind::Proc => &self.process,
        }
    }

    pub(crate) fn get_mut(&mut self, kind: SubsystemKind) -> &mut T {
        match kind {
            SubsystemKind::Cpu => &mut self.cpu,
            SubsystemKind::Mem => &mut self.mem,
            SubsystemKind::Disk => &mut self.disk,
            SubsystemKind::Net => &mut self.net,
            SubsystemKind::Gpu => &mut self.gpu,
            SubsystemKind::Proc => &mut self.process,
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
        assert_eq!(all.len(), 6);
        for kind in [
            SubsystemKind::Cpu,
            SubsystemKind::Mem,
            SubsystemKind::Disk,
            SubsystemKind::Net,
            SubsystemKind::Gpu,
            SubsystemKind::Proc,
        ] {
            assert_eq!(all.iter().filter(|k| **k == kind).count(), 1);
        }
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
}
