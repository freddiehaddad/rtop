use crate::input;

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
    /// New CPU data available in the CPU slot.
    CpuReady,
    /// New memory data available in the memory slot.
    MemReady,
    /// New disk data available in the disk slot.
    DiskReady,
    /// New network data available in the network slot.
    NetReady,
    /// New GPU data available in the GPU slot.
    GpuReady,
    /// New process data available in the process slot.
    ProcReady,
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
        tx.send(AppEvent::CpuReady).unwrap();
        tx.send(AppEvent::ProcReady).unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('q'))
        ));
        assert!(matches!(rx.recv().unwrap(), AppEvent::Resize));
        assert!(matches!(rx.recv().unwrap(), AppEvent::CpuReady));
        assert!(matches!(rx.recv().unwrap(), AppEvent::ProcReady));
    }

    #[test]
    fn sender_clone_allows_multiple_producers() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();

        tx.send(AppEvent::Key(input::Key::Char('a'))).unwrap();
        tx2.send(AppEvent::MemReady).unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('a'))
        ));
        assert!(matches!(rx.recv().unwrap(), AppEvent::MemReady));
    }

    #[test]
    fn try_recv_drains_queued_events() {
        let (tx, rx) = mpsc::channel();

        tx.send(AppEvent::Key(input::Key::Up)).unwrap();
        tx.send(AppEvent::Key(input::Key::Down)).unwrap();
        tx.send(AppEvent::CpuReady).unwrap();

        let first = rx.recv().unwrap();
        let rest: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();

        assert!(matches!(first, AppEvent::Key(input::Key::Up)));
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn app_event_is_copy() {
        let event = AppEvent::CpuReady;
        let copy = event;
        assert_eq!(event, copy);
    }
}
