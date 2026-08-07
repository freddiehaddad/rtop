use crate::collect::CollectStatus;
use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;

/// Toggle key digits shown as superscripts in widget titles.
///
/// Each constant is the digit key that toggles the corresponding widget.
/// Used by both the renderers (superscript label) and the input handler
/// (keybind dispatch) to keep them in sync.
pub const CPU_KEY: u8 = 1;
pub const MEM_KEY: u8 = 2;
pub const NET_KEY: u8 = 3;
pub const PROC_KEY: u8 = 4;
pub const DISK_KEY: u8 = 5;
/// GPU widget toggle key. The cycling-GPU widget is a singleton —
/// one toggle key suffices, joining the existing `1`-`5` array on
/// [`crate::handlers::normal::toggle_widget_main_action`].
pub const GPU_KEY: u8 = 6;

/// Shared area description for UI widget draw functions.
pub struct WidgetArea {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub rounded: bool,
}

impl WidgetArea {
    pub fn from_dim(dim: &crate::draw::layout::WidgetDimensions, rounded: bool) -> Self {
        Self {
            x: dim.x,
            y: dim.y,
            width: dim.width,
            height: dim.height,
            rounded,
        }
    }
}

/// Draw a status indicator inset on the top border when the collector is
/// degraded or failed. Placed after the widget title (left side).
///
/// `title` is the widget title text (e.g. "cpu", "mem") used to calculate
/// the offset past the existing title inset.
pub fn draw_status_inset(
    buf: &mut AnsiBuffer,
    status: &CollectStatus,
    title: &str,
    x: usize,
    y: usize,
    border_color: &str,
    title_color: &str,
) {
    if *status == CollectStatus::Ok {
        return;
    }
    let icon = match status {
        CollectStatus::Degraded(_) => "\u{26a0}",
        CollectStatus::Failed(_) => "\u{2717}",
        CollectStatus::Ok => unreachable!(),
    };
    let inset = box_drawing::title_inset(icon, border_color, title_color, false);
    // Position after the widget title: title_left(1) + bold_superscript(~1) + title_text + title_right(1)
    // create_box places the title at x+3, so the end of the title region is approximately:
    let title_end_x = x + 3 + box_drawing::inset_width(title) + 1;
    buf.mv(title_end_x, y + 1).text(&inset);
}

/// Resolved input for the proc widget's detail panel.
///
/// When `Some`, the proc widget reserves space for the panel and
/// renders it from `proc`. When `None`, the panel is closed and the
/// layout reclaims the rows.
///
/// `dead` is computed upstream as
/// `!live_snapshot.contains(open_pid)`. When `true`, the renderer
/// inserts the `✗ Process exited` status row. The flag is computed
/// the same way in both paused and live modes — see the
/// `DetailPanel` enum on `ProcessViewState` and the resolver in
/// `RenderInputs::build`.
#[derive(Clone, Copy)]
pub struct DetailView<'a> {
    pub proc: &'a ProcInfo,
    pub dead: bool,
}

/// Display state for the process list view.
pub struct ProcView<'a> {
    pub start: usize,
    pub selected: usize,
    pub sort_by: crate::collect::process_display::ProcSort,
    pub sort_reversed: bool,
    pub tree_mode: bool,
    /// Resolved detail panel input. `None` when closed.
    pub detail: Option<DetailView<'a>>,
    pub followed_pid: u32,
    pub filter: &'a str,
    pub filtering: bool,
    pub armed_name: &'a str,
    pub armed_force: bool,
    /// `true` when the proc list is paused. Drives the top-border
    /// `paused` chip and gates the dead-row styling rule.
    pub paused: bool,
    /// PIDs from the paused snapshot whose live counterpart has
    /// disappeared. Empty when not paused. The row renderer
    /// applies the exited gradient color + `✗ ` prefix to rows whose PID is
    /// in this set; the bottom-border `terminate` chip dims when
    /// the selected PID is in this set.
    pub dead_pids: &'a std::collections::HashSet<u32>,
    /// PID currently under the cursor. Used by the bottom-border
    /// renderer to decide whether to dim the `terminate` chip.
    pub selected_pid: u32,
}

/// One widget renderer.
///
/// Implemented by [`cpu_widget::CpuWidget`], [`mem_widget::MemWidget`],
/// [`net_widget::NetWidget`], [`proc_widget::ProcWidget`],
/// [`disk_widget::DiskWidget`], and [`gpu_widget::GpuWidget`]. Adding
/// a new widget kind is a one-file change: define the type, impl
/// `Widget`, register it in the static `WIDGETS` table consumed by
/// the dispatchers in [`crate::app::dirty_exec::render_all`] and
/// [`crate::draw::layout`].
///
/// **Why a trait** — six concrete implementers share an identical
/// shape (dirty-flag + per-kind sizing + render). The previous
/// design replicated the dispatch in three central match-on-
/// `WidgetKind` chains (`render_all`, `widget_preferred_height`,
/// `widget_min_width`/`min_height`); adding a new widget kind
/// required touching all of them with no compiler help. The trait
/// makes adding a widget a one-file change that compiles or
/// doesn't.
///
/// **`Sync` bound** — every implementer is a stateless unit struct,
/// so `Sync` is trivially satisfied. The bound is required because
/// the dispatchers hold the registered widgets as
/// `&'static [&'static dyn Widget]`, which Rust requires to be
/// `Sync` for cross-thread sharing.
///
/// **Object-safe** — all methods take `&self` and primitive /
/// borrowed types so the dispatchers can hold a
/// `&'static [&'static dyn Widget]`. For per-instance widgets
/// (today: GPU), the implementing type carries the instance index
/// (e.g., `GpuWidget { index: u8 }`) and `kinds(&self)` returns
/// the per-instance `WidgetKind`.
///
/// **No `draw` on the trait** — per-widget snapshot/frame types
/// don't generalise cleanly (each widget pulls a different
/// snapshot field, builds a different `*Frame`). Instead,
/// [`Widget::render`] takes the entire
/// [`crate::app::RenderParams`] and the implementer pulls what it
/// needs. The render method is the one place per widget where the
/// snapshot lookup, layout-slot lookup, frame construction, and
/// per-instance iteration live.
pub trait Widget: Sync {
    /// The widget kinds this renderer handles. Most widgets return
    /// a single-element slice; per-instance widgets (today: GPU)
    /// return one entry per instance index.
    ///
    /// Used by the layout engine to look up the right widget when
    /// it has a [`crate::domain::widget_kind::WidgetKind`] and
    /// needs that widget's intrinsic sizing (preferred / min).
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind];

    /// Whether this widget needs to be re-rendered this frame.
    /// Default impl returns `true` if any of the widget's claimed
    /// [`Self::kinds`] is dirty in `dirty`.
    ///
    /// **Multi-kind widgets** (today: [`gpu_widget::GpuWidget`])
    /// must additionally filter inside [`Self::render`] — the
    /// dispatcher only gates on "any kind dirty", so a single
    /// dirty GPU instance still calls `render` for the whole
    /// widget. The widget's render loop is responsible for
    /// skipping clean instances.
    fn is_dirty(&self, dirty: &crate::dirty::RenderDirty) -> bool {
        self.kinds().iter().any(|k| dirty.is_widget_dirty(*k))
    }

    /// Preferred intrinsic height in rows (including borders).
    /// The layout engine clamps this against per-widget `Preferred`
    /// slot rules and the terminal-relative caps (e.g. CPU's
    /// `term_height/3`).
    fn preferred_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize;

    /// Minimum width in columns (including borders). The layout
    /// engine uses this as the floor when a widget's allocation
    /// would otherwise be smaller.
    fn min_width(&self, hints: &crate::draw::layout::LayoutHints) -> usize;

    /// Minimum height in rows (including borders). Used by the
    /// `min_terminal_size` calculation to compute the smallest
    /// terminal at which the active layout fits.
    fn min_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize;

    /// Render this widget for every instance present in
    /// `params.layout`. The implementer pulls its snapshot from
    /// `params.<subsystem>`, looks up its layout slot via
    /// `params.layout.dims_for(...)`, builds the per-frame view,
    /// and calls its draw function. No-op (empty append) if any
    /// required data is missing.
    ///
    /// Appends to `output` rather than returning a string so the
    /// dispatcher doesn't pay an allocation per widget per frame
    /// for the common "nothing to render" case.
    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String);
}

pub mod cpu_widget;
pub mod disk_widget;
pub mod gpu_widget;
pub mod mem_widget;
pub mod net_widget;
pub mod proc_widget;
pub mod statusbar_widget;

/// Every widget renderer registered with the central dispatchers.
///
/// [`crate::app::dirty_exec::render_all`] iterates this slice and
/// calls [`Widget::render`] on each entry whose
/// [`Widget::is_dirty`] returns `true`. The layout engine's
/// per-kind sizing helpers ([`widget_for_kind`]) look up the
/// matching widget by [`Widget::kinds`].
///
/// Adding a new widget kind is a one-file change at the new
/// widget's call site (define the type, impl `Widget`) plus
/// adding it to this list.
pub static WIDGETS: &[&dyn Widget] = &[
    &cpu_widget::CpuWidget,
    &mem_widget::MemWidget,
    &net_widget::NetWidget,
    &proc_widget::ProcWidget,
    &disk_widget::DiskWidget,
    &gpu_widget::GpuWidget,
    &statusbar_widget::StatusbarWidget,
];

/// Look up the widget renderer responsible for a given
/// [`crate::domain::widget_kind::WidgetKind`].
///
/// Returns `None` if no registered widget claims the kind (which
/// is a programmer error — every kind in the schema must be
/// claimed by exactly one widget). The layout engine treats
/// `None` as "not laid out" (zero size, skip placement) so a
/// future schema mismatch degrades gracefully rather than
/// panicking.
pub fn widget_for_kind(
    kind: crate::domain::widget_kind::WidgetKind,
) -> Option<&'static dyn Widget> {
    for widget in WIDGETS {
        if widget.kinds().contains(&kind) {
            return Some(*widget);
        }
    }
    None
}
