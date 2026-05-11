//! Per-frame "what needs work this frame" tracker.
//!
//! `RenderDirty` replaces the previous `bitflags!`-based `Dirty`
//! type. It carries three independent groups of work:
//!
//! * `layout` — recompute the widget layout. Implies a full screen
//!   clear before re-render (and forces every visible widget to
//!   redraw, since their positions may have changed).
//! * `proc_list` — rebuild the derived process display list (sort
//!   + filter + tree) from the raw collected process data.
//! * `widgets` — per-instance dirty bits (one per [`WidgetKind`],
//!   including each GPU index). A single flag per widget kind
//!   replaces the previous shared `GPU_WIDGET` bit, so the GPU
//!   collector can mark only the GPUs that actually changed since
//!   the previous snapshot.
//!
//! Setting `proc_list` without also marking the proc widget would
//! draw the old list with the new sort applied — see
//! [`Self::mark_proc_data_changed`] for the combined helper.
//!
//! All fields are private. Mutators are intent-named so the
//! invariants documented above (proc_list ⇒ proc_widget;
//! layout ⇒ all widgets) cannot be violated by the caller.
//!
//! `RenderDirty: Copy` so [`crate::app::RenderParams`] can keep
//! it by value without churning the renderer signatures.
//!
//! All fields are private. Mutators are intent-named so the
//! invariants documented above (proc_list ⇒ proc_widget;
//! layout ⇒ all widgets) cannot be violated by the caller.
//!
//! `RenderDirty: Copy` so [`crate::app::RenderParams`] can keep
//! it by value without churning the renderer signatures.

use crate::domain::widget_kind::WidgetKind;
use crate::domain::widget_set::WidgetSet;

/// Per-frame dirty state: layout, proc-list rebuild, per-widget
/// render bits, plus overlay tracking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderDirty {
    layout: bool,
    proc_list: bool,
    widgets: WidgetSet,
    /// Overlay layer needs to be repainted this frame. Set on
    /// overlay open/close and on overlay-internal navigation
    /// (selection, page change, edit buffer mutation).
    overlay: bool,
}

impl RenderDirty {
    /// Empty dirty state — nothing needs work. Equivalent to
    /// [`Self::default`] but reads more clearly at call sites.
    #[cfg(test)]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Everything dirty: layout + proc_list + every widget.
    /// Equivalent to the previous `Dirty::FULL`.
    pub fn full() -> Self {
        let mut d = Self::default();
        d.mark_layout_and_all_widgets();
        d.proc_list = true;
        d
    }

    /// Construct a `RenderDirty` with every widget bit set, but
    /// neither `layout` nor `proc_list`. Used by paths that want
    /// to redraw every visible widget without forcing a layout
    /// recompute or a proc-list rebuild — e.g. the post-overlay
    /// redraw (`handlers::redraw_after_overlay`) where the
    /// underlying layout is unchanged.
    pub fn all_widgets() -> Self {
        let mut d = Self::default();
        d.mark_all_widgets();
        d
    }

    // ---- mutators ----------------------------------------------------

    /// Drop every dirty bit.
    pub fn clear(&mut self) {
        self.layout = false;
        self.proc_list = false;
        self.widgets.clear();
        self.overlay = false;
    }

    /// Mark the layout dirty. Always sets every widget dirty too,
    /// because a layout recompute may move every widget — they
    /// must all redraw at their new positions.
    pub fn mark_layout(&mut self) {
        self.mark_layout_and_all_widgets();
    }

    /// Mark layout + every widget dirty in one call. Used by
    /// resize, preset cycle, widget-toggle (`1`-`9`/`0`/Shift+R),
    /// menu-close paths, and the post-overlay redraw helper.
    pub fn mark_layout_and_all_widgets(&mut self) {
        self.layout = true;
        self.mark_all_widgets();
    }

    /// Mark a single widget dirty.
    pub fn mark_widget(&mut self, kind: WidgetKind) {
        self.widgets.insert(kind);
    }

    /// Mark every widget kind dirty (without touching `layout` or
    /// `proc_list`). Equivalent to the previous `Dirty::ALL_WIDGETS`.
    pub fn mark_all_widgets(&mut self) {
        for kind in WidgetKind::all() {
            self.widgets.insert(kind);
        }
    }

    /// Mark the process display list dirty AND the proc widget
    /// dirty in one call. The proc widget renders the post-sort,
    /// post-filter list; setting `proc_list` without `proc_widget`
    /// would update the data but leave the previous frame on
    /// screen until something else triggered a redraw.
    pub fn mark_proc_data_changed(&mut self) {
        self.proc_list = true;
        self.widgets.insert(WidgetKind::Proc);
    }

    /// Mark only the proc widget dirty (selection movement,
    /// armed-terminate state change, follow toggle — anything that
    /// changes how the existing list is rendered, not the list
    /// itself).
    pub fn mark_proc_widget(&mut self) {
        self.widgets.insert(WidgetKind::Proc);
    }

    /// Mark the overlay layer dirty: the next frame should
    /// recompose the modal/dimmed-underlay layer. Set on overlay
    /// open/close transitions and (in handler code) on
    /// overlay-internal navigation.
    pub fn mark_overlay(&mut self) {
        self.overlay = true;
    }

    /// `true` if no dirty state is set.
    pub fn is_empty(&self) -> bool {
        !self.layout && !self.proc_list && self.widgets.is_empty() && !self.overlay
    }

    /// `true` if the layout needs to be recomputed this frame.
    pub fn needs_layout(&self) -> bool {
        self.layout
    }

    /// `true` if the proc display list needs to be rebuilt this
    /// frame.
    pub fn needs_proc_list(&self) -> bool {
        self.proc_list
    }

    /// `true` if `kind` needs to be redrawn this frame.
    pub fn is_widget_dirty(&self, kind: WidgetKind) -> bool {
        self.widgets.contains(kind)
    }

    /// `true` if at least one widget needs to be redrawn this
    /// frame. Used by render gates that decide "should we even
    /// flush a frame at all?".
    pub fn is_any_widget_dirty(&self) -> bool {
        !self.widgets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_nothing_set() {
        let d = RenderDirty::empty();
        assert!(d.is_empty());
        assert!(!d.needs_layout());
        assert!(!d.needs_proc_list());
        assert!(!d.is_any_widget_dirty());
        for kind in WidgetKind::all() {
            assert!(!d.is_widget_dirty(kind));
        }
    }

    #[test]
    fn full_has_everything_set() {
        let d = RenderDirty::full();
        assert!(d.needs_layout());
        assert!(d.needs_proc_list());
        for kind in WidgetKind::all() {
            assert!(d.is_widget_dirty(kind), "{kind:?} must be dirty in full()");
        }
    }

    #[test]
    fn mark_widget_only_marks_that_widget() {
        let mut d = RenderDirty::empty();
        d.mark_widget(WidgetKind::Cpu);
        assert!(d.is_widget_dirty(WidgetKind::Cpu));
        assert!(!d.is_widget_dirty(WidgetKind::Mem));
        assert!(!d.needs_layout());
        assert!(!d.needs_proc_list());
    }

    #[test]
    fn mark_layout_marks_every_widget_too() {
        // Layout invariant: a recompute may move every widget, so
        // every widget's render slot is now stale. Marking only
        // `layout` would leave widgets at their new positions
        // un-redrawn.
        let mut d = RenderDirty::empty();
        d.mark_layout();
        assert!(d.needs_layout());
        for kind in WidgetKind::all() {
            assert!(
                d.is_widget_dirty(kind),
                "layout dirty must imply {kind:?} dirty",
            );
        }
    }

    #[test]
    fn mark_proc_data_changed_marks_both_proc_list_and_proc_widget() {
        // Proc-data invariant: changing the underlying list (sort,
        // filter, tree, raw process collection) without re-rendering
        // would leave the previous frame's rows on screen until
        // some other dirty bit triggered a redraw.
        let mut d = RenderDirty::empty();
        d.mark_proc_data_changed();
        assert!(d.needs_proc_list());
        assert!(d.is_widget_dirty(WidgetKind::Proc));
        // Other widgets stay clean.
        assert!(!d.is_widget_dirty(WidgetKind::Cpu));
    }

    #[test]
    fn mark_proc_widget_does_not_set_proc_list() {
        // Selection movement / armed-terminate / follow toggle:
        // the *list* hasn't changed, only the rendering. We must
        // not pay for a list rebuild.
        let mut d = RenderDirty::empty();
        d.mark_proc_widget();
        assert!(d.is_widget_dirty(WidgetKind::Proc));
        assert!(!d.needs_proc_list());
    }

    #[test]
    fn clear_resets_every_field() {
        let mut d = RenderDirty::full();
        d.clear();
        assert!(d.is_empty());
    }

    #[test]
    fn gpu_dirty_marks_singleton() {
        // The cycling-GPU widget is a singleton — there is only
        // one `WidgetKind::Gpu` slot in the dirty map, regardless
        // of how many physical GPUs are present.
        let mut d = RenderDirty::empty();
        assert!(!d.is_widget_dirty(WidgetKind::Gpu));
        d.mark_widget(WidgetKind::Gpu);
        assert!(d.is_widget_dirty(WidgetKind::Gpu));
        // Other widget kinds untouched.
        assert!(!d.is_widget_dirty(WidgetKind::Cpu));
    }

    #[test]
    fn render_dirty_is_copy() {
        // Compile-time check: RenderDirty must remain Copy so it
        // can be threaded through RenderParams by value.
        fn assert_copy<T: Copy>() {}
        assert_copy::<RenderDirty>();
    }
}
