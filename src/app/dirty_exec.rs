//! Dirty-flag execution: rebuild derived state, recompute the
//! cached layout, and produce the per-frame ANSI output.
//!
//! `execute_dirty_work` does the pre-render normalisation
//! (rebuild proc list, recompute layout, clamp proc selection).
//! `render_dirty_frame` and `render_all` produce the output string;
//! `write_dirty_frame` ships it to the terminal.
//!
//! `render_all` is a **pure function** of (state, dirty); all
//! mutation happens upstream so this module can stay easy to
//! reason about.

use crate::app::TerminalSize;
use crate::app::lifecycle::style_terminal_output;
use crate::app::state::{AppState, LiveData, NetworkViewState, ProcessViewState, RuntimeView};
use crate::config;
use crate::dirty::RenderDirty;
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::draw;
use crate::overlay::ActiveModal;
use crate::runner;
use crate::term;
use crate::theme;
use crate::ui;
use std::collections::HashSet;
use std::sync::OnceLock;

pub(crate) fn execute_dirty_work(
    state: &mut AppState,
    config: &mut config::Config,
    size: TerminalSize,
) {
    if state.render.dirty.needs_proc_list() {
        rebuild_proc_list(state, config);
    }

    if state.render.dirty.needs_layout() || state.render.cached_layout().is_none() {
        let layout = calculate_layout(state, config, size);
        state.render.set_cached_layout(layout);
    }

    // Pre-render normalisation: clamp the proc widget's view-state to
    // the current entry count and widget dimensions. Done here (not
    // inside render_all) so that the render path stays a pure
    // function of (state, dirty).
    if let Some(layout) = state.render.cached_layout()
        && let Some(proc_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Proc)
    {
        let detail_rows = if state.process.detailed_pid > 0 {
            8_usize.min(proc_dim.height.saturating_sub(6))
        } else {
            0
        };
        clamp_proc_selection(
            state.process.entries.len(),
            proc_dim.height,
            detail_rows,
            &mut state.process.selected,
            &mut state.process.start,
        );
    }
}

fn rebuild_proc_list(state: &mut AppState, config: &config::Config) {
    state
        .process
        .rebuild_entries(config, &state.view, &state.live);
}

fn calculate_layout(
    state: &AppState,
    config: &config::Config,
    size: TerminalSize,
) -> draw::layout::Layout {
    draw::layout::calc_sizes(&draw::layout::LayoutConfig {
        term_width: size.width,
        term_height: size.height,
        root: config.layout_spec().clone(),
        hints: state.live.layout_hints(config, &state.view),
        hidden: state.compose_hidden(config),
    })
}

pub(crate) fn write_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) {
    // Clone to break the borrow on `state.overlay` so we can also
    // mutate `state.render` below. `ActiveModal` is cheap to
    // clone — variants are unit-like or carry small state.
    let active = state.overlay.active.clone();
    // Reconcile the dim cache against the current modal boundary.
    // If the boundary moved since the previous frame the cached
    // snapshot is invalidated atomically — no separate "set last
    // boundary" step is possible to forget.
    let dims_now = state.render.reconcile_dim_boundary(active.dims_underlay());
    let output = if dims_now {
        compose_modal_frame(state, config, theme, &active, size)
    } else {
        let raw = render_dirty_frame(state, config, theme);
        style_terminal_output(&raw, config, theme)
    };
    if let Err(e) = terminal.write_synced(&output) {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Terminal,
            error = %e,
            "terminal write failed",
        );
    }
    state.render.clear_dirty();
}

/// Compose a single atomic frame consisting of a dimmed snapshot of
/// the widget layer plus the active modal painted on top at full
/// brightness.
///
/// The dimmed underlay snapshot is held inside [`crate::app::RenderState`]
/// for the lifetime of the modal — modal-internal navigation
/// repaints only the modal layer. The cache is invalidated by
/// modal open/close transitions (via
/// [`crate::app::RenderState::reconcile_dim_boundary`]) and by
/// terminal resize (via `mark_resize`).
fn compose_modal_frame(
    state: &mut AppState,
    config: &config::Config,
    theme: &theme::Theme,
    active: &crate::overlay::ActiveModal,
    size: TerminalSize,
) -> String {
    if state.render.cached_dimmed_underlay().is_none() {
        // Build a fresh underlay: render every widget, theme-style
        // the result, then run the dim transform. Order matters —
        // theme styling runs before dim so the base style
        // (including theme bg) is dimmed too; otherwise every
        // `\x1b[0m` reset in the underlay would re-apply the
        // full-brightness base style and leave bright halos.
        let raw = render_widget_layer_full(state, config, theme);
        let styled = style_terminal_output(&raw, config, theme);
        state
            .render
            .store_dimmed_underlay(crate::draw::dim::dim_truecolor(&styled));
    }
    let dimmed = state
        .render
        .cached_dimmed_underlay()
        .expect("cached_dimmed_underlay just populated above");

    let raw_modal = crate::overlay::render(active, size, config, theme);
    let styled_modal = style_terminal_output(&raw_modal, config, theme);

    format!("{}{}{}", term::CLEAR_SCREEN, dimmed, styled_modal)
}

/// Render every widget at full intensity, ignoring the per-widget
/// dirty bits (the dim cache must be a complete snapshot, not a
/// partial repaint of only the dirty widgets).
fn render_widget_layer_full(
    state: &AppState,
    config: &config::Config,
    theme: &theme::Theme,
) -> String {
    let layout = state
        .render
        .cached_layout()
        .expect("layout must be initialized before rendering");
    let params = RenderInputs {
        layout,
        live: &state.live,
        process: &state.process,
        network: &state.network,
        view: &state.view,
        filter: &state.filter,
        config,
        theme,
        dirty: RenderDirty::all_widgets(),
        is_filtering: false,
    }
    .build();
    let mut output = String::new();
    output.push_str(term::CLEAR_SCREEN);
    output.push_str(&render_all(&params));
    output
}

fn render_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    theme: &theme::Theme,
) -> String {
    let layout = state
        .render
        .cached_layout()
        .expect("layout must be initialized before rendering");
    let mut output = String::new();

    if state.render.dirty.needs_layout() {
        output.push_str(term::CLEAR_SCREEN);
    }

    let params = RenderInputs {
        layout,
        live: &state.live,
        process: &state.process,
        network: &state.network,
        view: &state.view,
        filter: &state.filter,
        config,
        theme,
        dirty: state.render.dirty,
        is_filtering: matches!(state.overlay.active, ActiveModal::Filter(_)),
    }
    .build();
    output.push_str(&render_all(&params));
    output
}

fn clamp_proc_selection(
    count: usize,
    widget_height: usize,
    detail_rows: usize,
    selected: &mut usize,
    start: &mut usize,
) {
    let max_visible = ui::proc_widget::visible_row_count(widget_height, detail_rows);
    if count == 0 {
        *selected = 0;
        *start = 0;
        return;
    }
    if *selected >= count {
        *selected = count - 1;
    }
    if max_visible == 0 {
        *start = *selected;
        return;
    }
    if *selected >= *start + max_visible {
        *start = *selected - max_visible + 1;
    }
    if *selected < *start {
        *start = *selected;
    }
}

/// Parameters for rendering the UI widgets.
pub(crate) struct RenderParams<'a> {
    pub(crate) dirty: RenderDirty,
    pub(crate) layout: &'a draw::layout::Layout,
    pub(crate) cpu: Option<&'a runner::CpuSnapshot>,
    pub(crate) mem: Option<&'a runner::MemSnapshot>,
    pub(crate) disk: Option<&'a runner::DiskSnapshot>,
    pub(crate) net: Option<&'a runner::NetSnapshot>,
    pub(crate) gpu: Option<&'a runner::GpuSnapshot>,
    pub(crate) proc_data: Option<&'a runner::ProcSnapshot>,
    pub(crate) proc_entries: &'a [ProcDisplayEntry],
    /// Process slice the proc widget renders rows from. Equals
    /// `pause.snapshot.procs` when paused; otherwise
    /// `proc_data.procs`. Borrowed for the lifetime of the frame so
    /// the renderer never has to repeat the paused/live choice.
    pub(crate) proc_source: &'a [ProcInfo],
    /// `true` when the proc-list pause is active. Drives the
    /// top-border `paused` chip and the dead-row layering rule.
    pub(crate) proc_paused: bool,
    /// PIDs from the paused snapshot that are no longer in the live
    /// snapshot. Empty when not paused. The proc widget consults
    /// this to render dead-row styling and the bottom-border
    /// `terminate` chip dim treatment.
    pub(crate) dead_pids: &'a HashSet<u32>,
    pub(crate) selected_iface: &'a str,
    pub(crate) config: &'a config::Config,
    pub(crate) view: &'a RuntimeView,
    pub(crate) theme: &'a theme::Theme,
    pub(crate) rounded: bool,
    pub(crate) update_ms: u64,
    pub(crate) is_filtering: bool,
    /// `true` when the runtime view filter has at least one widget
    /// in it. Threaded into the CPU widget settings so the bottom
    /// preset hint can render a `*` suffix on the preset name.
    pub(crate) filter_active: bool,
    pub(crate) core_count: usize,
    pub(crate) total_mem: u64,
    pub(crate) detailed_pid: u32,
    pub(crate) followed_pid: u32,
    pub(crate) armed_terminate: Option<(&'a str, bool)>,
    pub(crate) proc_selected: usize,
    pub(crate) proc_start: usize,
}

/// Inputs required to build a [`RenderParams`].
///
/// Bundles the per-frame state borrows so the builder takes one
/// argument instead of nine. Used by [`RenderInputs::build`].
pub(crate) struct RenderInputs<'a> {
    pub(crate) layout: &'a draw::layout::Layout,
    pub(crate) live: &'a LiveData,
    pub(crate) process: &'a ProcessViewState,
    pub(crate) network: &'a NetworkViewState,
    pub(crate) view: &'a RuntimeView,
    pub(crate) filter: &'a crate::app::WidgetFilter,
    pub(crate) config: &'a config::Config,
    pub(crate) theme: &'a theme::Theme,
    /// Which widgets to render this frame.
    pub(crate) dirty: RenderDirty,
    /// `true` while the user is in the filter overlay so the proc
    /// widget can show its inline filter prompt.
    pub(crate) is_filtering: bool,
}

/// Empty dead-PID set used as the borrow target when no pause is
/// active. `OnceLock` so the renderer can always borrow a real
/// `&HashSet<u32>` from `RenderParams::dead_pids` without dealing
/// with `Option` indirection.
fn empty_dead_pids() -> &'static HashSet<u32> {
    static EMPTY: OnceLock<HashSet<u32>> = OnceLock::new();
    EMPTY.get_or_init(HashSet::new)
}

impl<'a> RenderInputs<'a> {
    /// Materialise a [`RenderParams`] view of these inputs.
    ///
    /// Used by both `render_dirty_frame` (the main per-frame render
    /// path) and `handlers::redraw_after_overlay` (the post-overlay
    /// redraw path) so the per-widget field wiring lives in one
    /// place. Adding a new widget setting or per-frame state field
    /// touches one call site.
    pub(crate) fn build(self) -> RenderParams<'a> {
        let proc_source = self.process.procs_source(self.live).unwrap_or(&[]);
        let dead_pids = match self.process.pause.as_ref() {
            Some(p) => &p.dead_pids,
            None => empty_dead_pids(),
        };
        RenderParams {
            dirty: self.dirty,
            layout: self.layout,
            cpu: self.live.cpu.as_deref(),
            mem: self.live.mem.as_deref(),
            disk: self.live.disk.as_deref(),
            net: self.live.net.as_deref(),
            gpu: self.live.gpu.as_deref(),
            proc_data: self.live.proc_data.as_deref(),
            proc_entries: &self.process.entries,
            proc_source,
            proc_paused: self.process.pause.is_some(),
            dead_pids,
            selected_iface: self.network.selected_iface.as_str(),
            config: self.config,
            view: self.view,
            theme: self.theme,
            rounded: self.config.ui.rounded_corners,
            update_ms: self.config.refresh.update_ms as u64,
            is_filtering: self.is_filtering,
            filter_active: !self.filter.hidden.is_empty(),
            core_count: self.live.core_count,
            total_mem: self.live.total_mem,
            detailed_pid: self.process.detailed_pid,
            followed_pid: self.process.followed_pid,
            armed_terminate: self
                .process
                .armed_terminate
                .as_ref()
                .map(|(_, name, force)| (name.as_str(), *force)),
            proc_selected: self.process.selected,
            proc_start: self.process.start,
        }
    }
}

/// Render UI widgets into an ANSI output string.
///
/// **Pure function** of `(params, params.layout, params.dirty)`.
/// All view-state normalisation (proc-list rebuild, selection
/// clamping, network-interface reconciliation) happens before this
/// is called, in `pull::pull_subsystem_data` or `execute_dirty_work`.
/// No state mutation, no I/O.
///
/// Only renders widgets whose corresponding dirty flag is set.
/// Pass `Dirty::ALL_WIDGETS` to render everything.
pub(crate) fn render_all(params: &RenderParams) -> String {
    let mut output = String::new();
    for widget in ui::WIDGETS {
        if widget.is_dirty(&params.dirty) {
            widget.render(params, &mut output);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_proc_selection_allows_last_visible_row() {
        let mut selected = 3;
        let mut start = 0;

        clamp_proc_selection(10, 8, 0, &mut selected, &mut start);

        assert_eq!(selected, 3);
        assert_eq!(start, 0);
    }
}
