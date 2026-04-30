use crate::input;

/// Events processed by the main event loop.
///
/// All event sources (input thread, collection workers) send through
/// a single `mpsc::Sender<AppEvent>` channel. The main loop blocks
/// on the receiver, processes events, and renders dirty boxes.
pub(crate) enum AppEvent {
    /// A key was pressed (from input thread).
    Key(input::Key),
    /// Terminal was resized (from input thread).
    Resize,
    /// New collection data is available in the shared snapshot slot.
    ///
    /// The main loop reads the latest snapshot via `CollectionWorker::latest_if_new()`.
    /// Multiple `SnapshotReady` events coalesce — only the latest data is used.
    SnapshotReady,
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
        tx.send(AppEvent::SnapshotReady).unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('q'))
        ));
        assert!(matches!(rx.recv().unwrap(), AppEvent::Resize));
        assert!(matches!(rx.recv().unwrap(), AppEvent::SnapshotReady));
    }

    #[test]
    fn sender_clone_allows_multiple_producers() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();

        tx.send(AppEvent::Key(input::Key::Char('a'))).unwrap();
        tx2.send(AppEvent::SnapshotReady).unwrap();

        assert!(matches!(
            rx.recv().unwrap(),
            AppEvent::Key(input::Key::Char('a'))
        ));
        assert!(matches!(rx.recv().unwrap(), AppEvent::SnapshotReady));
    }

    #[test]
    fn try_recv_drains_queued_events() {
        let (tx, rx) = mpsc::channel();

        tx.send(AppEvent::Key(input::Key::Up)).unwrap();
        tx.send(AppEvent::Key(input::Key::Down)).unwrap();
        tx.send(AppEvent::SnapshotReady).unwrap();

        let first = rx.recv().unwrap();
        let rest: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();

        assert!(matches!(first, AppEvent::Key(input::Key::Up)));
        assert_eq!(rest.len(), 2);
    }
}
