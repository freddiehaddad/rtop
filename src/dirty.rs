use bitflags::bitflags;

bitflags! {
    /// Per-subsystem dirty tracking for the render loop.
    ///
    /// Each flag represents something that needs work on the current frame.
    /// The main loop checks which flags are set, performs only the required
    /// UI work (view-model rebuild, layout, per-box render), then clears them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Dirty: u32 {
        /// Recalculate box layout (on resize, box toggle, preset change).
        /// Implies a full-screen clear before rendering.
        const LAYOUT    = 1 << 0;
        /// Redraw the CPU box.
        const CPU_BOX   = 1 << 1;
        /// Redraw the memory box.
        const MEM_BOX   = 1 << 2;
        /// Redraw the network box.
        const NET_BOX   = 1 << 3;
        /// Redraw the process box.
        const PROC_BOX  = 1 << 4;
        /// Redraw GPU box(es).
        const GPU_BOX   = 1 << 5;
        /// Rebuild the derived process display list (sort, filter, tree)
        /// from the raw collected process data.
        const PROC_LIST = 1 << 6;
        /// Redraw the disk box.
        const DISK_BOX  = 1 << 7;

        /// All renderable boxes.
        const ALL_BOXES = Self::CPU_BOX.bits()
                        | Self::MEM_BOX.bits()
                        | Self::NET_BOX.bits()
                        | Self::PROC_BOX.bits()
                        | Self::GPU_BOX.bits()
                        | Self::DISK_BOX.bits();

        /// Everything render-side — layout + all boxes + proc list.
        const FULL = Self::LAYOUT.bits()
                   | Self::ALL_BOXES.bits()
                   | Self::PROC_LIST.bits();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_boxes_includes_every_box() {
        assert!(Dirty::ALL_BOXES.contains(Dirty::CPU_BOX));
        assert!(Dirty::ALL_BOXES.contains(Dirty::MEM_BOX));
        assert!(Dirty::ALL_BOXES.contains(Dirty::NET_BOX));
        assert!(Dirty::ALL_BOXES.contains(Dirty::PROC_BOX));
        assert!(Dirty::ALL_BOXES.contains(Dirty::GPU_BOX));
        assert!(Dirty::ALL_BOXES.contains(Dirty::DISK_BOX));
    }

    #[test]
    fn full_includes_everything() {
        assert!(Dirty::FULL.contains(Dirty::LAYOUT));
        assert!(Dirty::FULL.contains(Dirty::ALL_BOXES));
        assert!(Dirty::FULL.contains(Dirty::PROC_LIST));
    }

    #[test]
    fn flags_are_independent() {
        let mut d = Dirty::empty();
        assert!(!d.contains(Dirty::CPU_BOX));
        d |= Dirty::CPU_BOX;
        assert!(d.contains(Dirty::CPU_BOX));
        assert!(!d.contains(Dirty::MEM_BOX));
    }

    #[test]
    fn insert_and_remove() {
        let mut d = Dirty::CPU_BOX | Dirty::NET_BOX;
        d.remove(Dirty::CPU_BOX);
        assert!(!d.contains(Dirty::CPU_BOX));
        assert!(d.contains(Dirty::NET_BOX));
    }
}
