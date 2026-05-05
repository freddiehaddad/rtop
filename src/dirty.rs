use bitflags::bitflags;

bitflags! {
    /// Per-subsystem dirty tracking for the render loop.
    ///
    /// Each flag represents something that needs work on the current frame.
    /// The main loop checks which flags are set, performs only the required
    /// UI work (view-model rebuild, layout, per-widget render), then clears them.
    ///
    /// Key invariant: changing process data (sort, filter, tree mode) requires
    /// both `PROC_LIST` (rebuild the display list) and `PROC_WIDGET` (redraw).
    /// Setting `PROC_WIDGET` alone only redraws with the existing display list
    /// (e.g. selection movement).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Dirty: u32 {
        /// Recalculate widget layout (on resize, widget toggle, preset change).
        /// Implies a full-screen clear before rendering.
        const LAYOUT     = 1 << 0;
        /// Redraw the CPU widget.
        const CPU_WIDGET  = 1 << 1;
        /// Redraw the memory widget.
        const MEM_WIDGET  = 1 << 2;
        /// Redraw the network widget.
        const NET_WIDGET  = 1 << 3;
        /// Redraw the process widget.
        const PROC_WIDGET = 1 << 4;
        /// Redraw the GPU widget(s).
        const GPU_WIDGET  = 1 << 5;
        /// Rebuild the derived process display list (sort, filter, tree)
        /// from the raw collected process data.
        const PROC_LIST  = 1 << 6;
        /// Redraw the disk widget.
        const DISK_WIDGET = 1 << 7;

        /// All renderable widgets.
        const ALL_WIDGETS = Self::CPU_WIDGET.bits()
                          | Self::MEM_WIDGET.bits()
                          | Self::NET_WIDGET.bits()
                          | Self::PROC_WIDGET.bits()
                          | Self::GPU_WIDGET.bits()
                          | Self::DISK_WIDGET.bits();

        /// Everything render-side — layout + all widgets + proc list.
        const FULL = Self::LAYOUT.bits()
                   | Self::ALL_WIDGETS.bits()
                   | Self::PROC_LIST.bits();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_widgets_includes_every_widget() {
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::CPU_WIDGET));
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::MEM_WIDGET));
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::NET_WIDGET));
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::PROC_WIDGET));
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::GPU_WIDGET));
        assert!(Dirty::ALL_WIDGETS.contains(Dirty::DISK_WIDGET));
    }

    #[test]
    fn full_includes_everything() {
        assert!(Dirty::FULL.contains(Dirty::LAYOUT));
        assert!(Dirty::FULL.contains(Dirty::ALL_WIDGETS));
        assert!(Dirty::FULL.contains(Dirty::PROC_LIST));
    }

    #[test]
    fn flags_are_independent() {
        let mut d = Dirty::empty();
        assert!(!d.contains(Dirty::CPU_WIDGET));
        d |= Dirty::CPU_WIDGET;
        assert!(d.contains(Dirty::CPU_WIDGET));
        assert!(!d.contains(Dirty::MEM_WIDGET));
    }

    #[test]
    fn insert_and_remove() {
        let mut d = Dirty::CPU_WIDGET | Dirty::NET_WIDGET;
        d.remove(Dirty::CPU_WIDGET);
        assert!(!d.contains(Dirty::CPU_WIDGET));
        assert!(d.contains(Dirty::NET_WIDGET));
    }
}
