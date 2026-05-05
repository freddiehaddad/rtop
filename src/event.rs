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
}
