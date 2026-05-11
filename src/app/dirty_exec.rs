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
use crate::app::state::{
    AppState, DetailPanel, DimComposeMode, GpuViewState, LiveData, ModalFootprint,
    NetworkViewState, ProcessViewState, RuntimeView, WidgetFilter,
};
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
use std::sync::{Arc, OnceLock};

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
        let detail_rows = if state.process.detail_info().is_some() {
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
        hints: state.live.layout_hints(config, &state.view, &state.filter),
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
    // Reconcile the dim cache against the current modal footprint
    // (Main, Help, Options, NoModal). The cache encodes whether
    // the underlay snapshot is still valid AND whether the
    // previous modal's pixels need wiping; the returned mode
    // tells the compose path exactly what to emit this frame.
    let footprint = ModalFootprint::from_active(&active);
    let mode = state.render.reconcile_dim_compose(footprint);
    let output = match mode {
        DimComposeMode::Skip => {
            let raw = render_dirty_frame(state, config, theme);
            style_terminal_output(&raw, config, theme)
        }
        DimComposeMode::BuildAndEmit
        | DimComposeMode::EmitFromCache
        | DimComposeMode::ModalOnly => {
            compose_modal_frame(state, config, theme, &active, size, mode)
        }
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
/// repaints only the modal layer ([`DimComposeMode::ModalOnly`]).
/// The cache is invalidated by modal open/close transitions and
/// by terminal resize.
///
/// `mode` is the [`DimComposeMode`] returned by
/// [`crate::app::RenderState::reconcile_dim_compose`]; it carries
/// the per-frame decision (build vs. re-emit vs. modal-only).
///
/// **CLEAR_SCREEN gating.** Both [`DimComposeMode::BuildAndEmit`]
/// (cache miss — first paint after open, or after resize) and
/// [`DimComposeMode::EmitFromCache`] (modal kind changed mid-flight,
/// cache still valid) emit `CLEAR_SCREEN` as a frame prefix so the
/// transition starts from a clean canvas. The `EmitFromCache`
/// branch is what wipes the previous modal's footprint on a
/// kind-change transition (e.g. closing Options back to Main from
/// the main menu); without it, the larger previous modal's outer
/// ring would remain visible behind the smaller new modal.
///
/// [`DimComposeMode::ModalOnly`] (modal-internal navigation: same
/// modal kind, cache present) emits only the modal layer painted
/// over its own rectangle — no `CLEAR_SCREEN`, no re-emit of the
/// underlay. This is what lets a user keep an active mouse text-
/// selection on the dimmed area while navigating within a single
/// menu.
fn compose_modal_frame(
    state: &mut AppState,
    config: &config::Config,
    theme: &theme::Theme,
    active: &crate::overlay::ActiveModal,
    size: TerminalSize,
    mode: DimComposeMode,
) -> String {
    let raw_modal = crate::overlay::render(active, size, config, theme);
    let styled_modal = style_terminal_output(&raw_modal, config, theme);

    match mode {
        DimComposeMode::BuildAndEmit => {
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
            let dimmed = state
                .render
                .cached_dimmed_underlay()
                .expect("cached_dimmed_underlay just populated above");
            format!("{}{dimmed}{styled_modal}", term::CLEAR_SCREEN)
        }
        DimComposeMode::EmitFromCache => {
            // Modal kind changed but the dimmed underlay is still
            // valid (widgets unchanged while a modal was on top).
            // Re-emit the cached underlay to wipe the previous
            // modal's footprint, then paint the new modal on top.
            let dimmed = state
                .render
                .cached_dimmed_underlay()
                .expect("EmitFromCache is only returned by reconcile when the snapshot is present");
            format!("{}{dimmed}{styled_modal}", term::CLEAR_SCREEN)
        }
        DimComposeMode::ModalOnly => {
            // Cache hit, footprint unchanged: the dimmed underlay
            // is already on screen from a previous frame and the
            // modal box bounds haven't moved. Paint only the modal
            // layer over its own rectangle — cells outside the
            // modal are untouched.
            styled_modal
        }
        DimComposeMode::Skip => unreachable!(
            "compose_modal_frame is only entered for the three dim-modal modes; \
             write_dirty_frame routes Skip to the widget render path"
        ),
    }
}

/// Render every widget at full intensity, ignoring the per-widget
/// dirty bits (the dim cache must be a complete snapshot, not a
/// partial repaint of only the dirty widgets).
///
/// The output begins with `CLEAR_SCREEN` so the rendered frame is
/// a self-contained "draw me from scratch" string. Important: this
/// function is the sole producer of the dim cache, and that cache
/// is what gets emitted on the modal-open transition. The leading
/// clear is the right place for the canvas wipe — it travels with
/// the underlay through the dim transform and into the cache.
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
        gpu: &state.gpu,
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
        gpu: &state.gpu,
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
    pub(crate) gpu: &'a [Option<Arc<runner::GpuSnapshot>>],
    pub(crate) proc_data: Option<&'a runner::ProcSnapshot>,
    pub(crate) statusbar: Option<&'a runner::StatusbarSnapshot>,
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
    pub(crate) selected_net_iface: &'a str,
    pub(crate) selected_gpu_iface: &'a str,
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
    /// Resolved detail panel input passed straight to the proc
    /// widget. `Some` when the panel is open; the `proc` reference
    /// points either into `proc_source` (when the source still
    /// contains the open PID) or into the cached `last_seen` on
    /// `state.process.detail` (when the watched process has
    /// exited). `dead = true` iff the live snapshot does not
    /// contain the open PID — the same rule for both paused and
    /// live modes.
    pub(crate) detail_view: Option<ui::DetailView<'a>>,
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
    pub(crate) gpu: &'a GpuViewState,
    pub(crate) view: &'a RuntimeView,
    pub(crate) filter: &'a WidgetFilter,
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
        let detail_view = resolve_detail_view(self.process, self.live, proc_source);
        RenderParams {
            dirty: self.dirty,
            layout: self.layout,
            cpu: self.live.cpu.as_deref(),
            mem: self.live.mem.as_deref(),
            disk: self.live.disk.as_deref(),
            net: self.live.net.as_deref(),
            gpu: &self.live.gpu,
            proc_data: self.live.proc_data.as_deref(),
            statusbar: self.live.statusbar.as_deref(),
            proc_entries: &self.process.entries,
            proc_source,
            proc_paused: self.process.pause.is_some(),
            dead_pids,
            selected_net_iface: self.network.selected_iface.as_str(),
            selected_gpu_iface: self.gpu.selected_iface.as_str(),
            config: self.config,
            view: self.view,
            theme: self.theme,
            rounded: self.config.ui.rounded_corners,
            update_ms: self.config.refresh.update_ms as u64,
            is_filtering: self.is_filtering,
            filter_active: !self.filter.hidden.is_empty(),
            core_count: self.live.core_count,
            total_mem: self.live.total_mem,
            detail_view,
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

/// Resolve the proc widget's detail-panel render input.
///
/// Returns `None` when the panel is closed. Otherwise returns the
/// `ProcInfo` reference the renderer should display together with
/// the `dead` flag:
///
/// * `proc` borrows from `proc_source` when the source still
///   contains the open PID, falling back to the cached `last_seen`
///   on `process.detail` when it does not. This keeps the panel
///   functional after the watched process exits in live mode (the
///   row vanishes from `proc_source` but the cache survives) and
///   correctly mirrors the snapshot value in paused mode.
/// * `dead` is `true` iff the live snapshot does not contain the
///   open PID. The same rule applies in both paused and live modes
///   — in paused mode it reduces to "PID is in `dead_pids`" because
///   `dead_pids` is exactly the snapshot-minus-live diff intersected
///   with the snapshot's PID set.
fn resolve_detail_view<'a>(
    process: &'a ProcessViewState,
    live: &LiveData,
    proc_source: &'a [ProcInfo],
) -> Option<ui::DetailView<'a>> {
    let open_pid = match &process.detail {
        DetailPanel::Closed => return None,
        DetailPanel::Open(d) => d.pid,
    };
    let proc = proc_source
        .iter()
        .find(|p| p.pid == open_pid)
        .or_else(|| process.detail_info())?;
    let dead = live
        .proc_data
        .as_ref()
        .is_none_or(|s| !s.procs.iter().any(|p| p.pid == open_pid));
    Some(ui::DetailView { proc, dead })
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
    use crate::collect::CollectStatus;
    use crate::domain::process::ProcInfo;
    use std::sync::Arc;

    #[test]
    fn clamp_proc_selection_allows_last_visible_row() {
        let mut selected = 3;
        let mut start = 0;

        clamp_proc_selection(10, 8, 0, &mut selected, &mut start);

        assert_eq!(selected, 3);
        assert_eq!(start, 0);
    }

    // ─────────────────────────────────────────────────────────────
    // resolve_detail_view — pure function exercising the panel
    // resolution rule used by RenderInputs::build.
    // ─────────────────────────────────────────────────────────────

    fn proc_with(pid: u32, name: &str) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.into(),
            ..Default::default()
        }
    }

    fn snap(pids: &[u32]) -> Arc<runner::ProcSnapshot> {
        Arc::new(runner::ProcSnapshot {
            procs: pids.iter().map(|&pid| proc_with(pid, "p")).collect(),
            status: CollectStatus::Ok,
        })
    }

    /// Build a fresh `AppState` for resolver tests, then return
    /// `(process, live)` borrows for the call. Centralising the
    /// construction means the resolver tests do not depend on
    /// the privacy of `ProcessViewState::new` / `LiveData::new`.
    fn make_state() -> AppState {
        let config = config::Config::new();
        AppState::new(&config, 0)
    }

    #[test]
    fn resolve_detail_view_returns_none_when_panel_closed() {
        let state = make_state();
        let proc_source: Vec<ProcInfo> = vec![];
        assert!(matches!(state.process.detail, DetailPanel::Closed));
        assert!(resolve_detail_view(&state.process, &state.live, &proc_source).is_none());
    }

    #[test]
    fn resolve_detail_view_uses_proc_source_when_pid_present() {
        // Live mode, process alive: the detail proc reference points
        // at the live snapshot row, NOT the cached last_seen.
        let mut state = make_state();
        state.process.open_detail(proc_with(100, "stale-cache"));
        state.live.proc_data = Some(snap(&[100, 200]));
        let proc_source = vec![proc_with(100, "fresh-live"), proc_with(200, "other")];

        let view = resolve_detail_view(&state.process, &state.live, &proc_source)
            .expect("panel resolution should succeed");
        assert_eq!(view.proc.name, "fresh-live");
        assert!(
            !view.dead,
            "PID 100 is in live snapshot, dead must be false"
        );
    }

    #[test]
    fn resolve_detail_view_falls_back_to_cache_when_pid_absent_from_source() {
        // Live mode, process exited: source no longer contains the
        // PID. resolve falls back to the cached last_seen and marks
        // dead = true.
        let mut state = make_state();
        state.process.open_detail(proc_with(100, "cached-name"));
        state.live.proc_data = Some(snap(&[200, 300]));
        let proc_source = vec![proc_with(200, "other"), proc_with(300, "another")];

        let view = resolve_detail_view(&state.process, &state.live, &proc_source)
            .expect("cache fallback succeeds");
        assert_eq!(view.proc.name, "cached-name");
        assert!(
            view.dead,
            "PID 100 is not in live snapshot, dead must be true"
        );
    }

    #[test]
    fn resolve_detail_view_marks_dead_when_no_live_data() {
        // First-frame race: panel was somehow opened (e.g. via
        // initial keystroke) before the first live snapshot
        // arrived. dead = true is the correct conservative answer
        // because there is no evidence the PID is alive.
        let mut state = make_state();
        state.process.open_detail(proc_with(100, "alpha"));
        let proc_source = vec![proc_with(100, "alpha")];

        let view = resolve_detail_view(&state.process, &state.live, &proc_source)
            .expect("source still resolves the proc");
        assert!(
            view.dead,
            "no live data means dead must default to true (no proof of life)"
        );
    }

    #[test]
    fn resolve_detail_view_dead_flag_unifies_paused_and_live_modes() {
        // The dead rule (`!live.contains(open_pid)`) gives the same
        // answer in paused mode that the existing `dead_pids`
        // machinery produces. Construct the paused-mode shape and
        // verify the rule fires.
        let mut state = make_state();
        state.process.open_detail(proc_with(100, "alpha"));

        // Paused mode: proc_source comes from the snapshot, which
        // still contains the open PID.
        let proc_source = vec![proc_with(100, "alpha"), proc_with(200, "beta")];

        // Live snapshot lost PID 100 (process exited after pause).
        state.live.proc_data = Some(snap(&[200]));

        let view = resolve_detail_view(&state.process, &state.live, &proc_source)
            .expect("snapshot still has PID 100");
        assert_eq!(view.proc.pid, 100);
        assert!(
            view.dead,
            "live lost PID 100, dead must be true even when source still has it"
        );
    }

    #[test]
    fn resolve_detail_view_dead_flag_clears_on_resurrection() {
        // PID dies, then a process with the same PID reappears in
        // the live snapshot. resolve must report dead = false
        // again. (The panel will then show the new live values; the
        // cached `last_seen` is overwritten by the next
        // `refresh_detail_cache` call in the pull path.)
        let mut state = make_state();
        state.process.open_detail(proc_with(100, "old"));

        state.live.proc_data = Some(snap(&[100, 200]));
        let proc_source = vec![proc_with(100, "resurrected"), proc_with(200, "other")];

        let view = resolve_detail_view(&state.process, &state.live, &proc_source).unwrap();
        assert!(!view.dead);
        assert_eq!(view.proc.name, "resurrected");
    }

    // ─────────────────────────────────────────────────────────────
    // Dim-cache compose path — CLEAR_SCREEN gating contract +
    // modal-kind-change regression coverage.
    //
    // `compose_modal_frame` now takes an explicit `DimComposeMode`.
    // The mode is produced by `RenderState::reconcile_dim_compose`
    // from the current `ModalFootprint`; these tests drive both the
    // reconcile and the compose to lock in the end-to-end behaviour.
    // ─────────────────────────────────────────────────────────────

    /// Build a fresh `AppState` ready for modal compose: empty layout
    /// cached, the requested modal active, and the dim-cache footprint
    /// reset to `NoModal` so the first reconcile sees an open
    /// transition.
    fn modal_state(active: crate::overlay::ActiveModal) -> AppState {
        let mut state = make_state();
        state
            .render
            .set_cached_layout(crate::draw::layout::Layout::default());
        state.overlay.active = active;
        state
    }

    fn fixture_size() -> TerminalSize {
        TerminalSize {
            width: 80,
            height: 30,
        }
    }

    /// Drive one full pass through the dim-cache pipeline:
    /// reconcile → compose. Returns the produced frame. Mirrors
    /// what `write_dirty_frame` does, minus the terminal write.
    fn compose_one_frame(
        state: &mut AppState,
        config: &config::Config,
        theme: &theme::Theme,
        size: TerminalSize,
    ) -> String {
        let active = state.overlay.active.clone();
        let footprint = ModalFootprint::from_active(&active);
        let mode = state.render.reconcile_dim_compose(footprint);
        compose_modal_frame(state, config, theme, &active, size, mode)
    }

    #[test]
    fn first_modal_frame_emits_clear_screen_and_underlay() {
        let config = config::Config::new();
        let theme = theme::Theme::new();
        let mut state = modal_state(crate::overlay::ActiveModal::Main(
            crate::overlay::main_menu::MainMenuState::new(),
        ));

        let out = compose_one_frame(&mut state, &config, &theme, fixture_size());
        assert!(
            out.starts_with(term::CLEAR_SCREEN),
            "first modal frame (cache miss) must begin with CLEAR_SCREEN; got prefix: {:?}",
            &out.chars().take(10).collect::<String>(),
        );
        assert!(
            state.render.cached_dimmed_underlay().is_some(),
            "first modal frame must populate the dim cache",
        );
    }

    #[test]
    fn same_modal_navigation_emits_modal_only_no_clear_screen() {
        // Regression guard for the mouse-text-selection invariant:
        // navigating *within* a single modal must not re-emit
        // CLEAR_SCREEN or the underlay.
        let config = config::Config::new();
        let theme = theme::Theme::new();
        let mut state = modal_state(crate::overlay::ActiveModal::Main(
            crate::overlay::main_menu::MainMenuState::new(),
        ));

        let _first = compose_one_frame(&mut state, &config, &theme, fixture_size());
        let second = compose_one_frame(&mut state, &config, &theme, fixture_size());
        assert!(
            !second.contains(term::CLEAR_SCREEN),
            "same-modal repaint must NOT emit CLEAR_SCREEN anywhere; got: {second:?}",
        );
    }

    #[test]
    fn modal_kind_change_re_emits_underlay_with_clear_screen() {
        // The bug this fix targets: pressing m → Enter (open Help
        // or Options from Main) → Esc (back to Main) used to leave
        // the previous modal's outer ring visible because the
        // cache-hit branch only emitted the new (smaller) modal.
        // After the fix, a footprint change re-emits the cached
        // dimmed underlay (wiping the previous modal's pixels)
        // before painting the new modal.
        let config = config::Config::new();
        let theme = theme::Theme::new();
        let mut state = modal_state(crate::overlay::ActiveModal::Main(
            crate::overlay::main_menu::MainMenuState::new(),
        ));

        // Frame 1: open Main (BuildAndEmit — populates the cache).
        let _main_frame = compose_one_frame(&mut state, &config, &theme, fixture_size());
        let cached_underlay = state
            .render
            .cached_dimmed_underlay()
            .expect("Main frame populates the cache")
            .to_string();

        // Frame 2: switch to Options. Footprint changes from
        // Main → Options while dim stays true → EmitFromCache.
        // The cached underlay must appear in the output prefixed
        // by CLEAR_SCREEN, NOT just the Options modal alone.
        state.overlay.active = crate::overlay::ActiveModal::Options(
            crate::overlay::options::OptionsState::new(crate::overlay::ReturnTarget::Main),
        );
        let kind_change_frame = compose_one_frame(&mut state, &config, &theme, fixture_size());
        assert!(
            kind_change_frame.starts_with(term::CLEAR_SCREEN),
            "modal-kind-change frame must begin with CLEAR_SCREEN; got prefix: {:?}",
            &kind_change_frame.chars().take(10).collect::<String>(),
        );
        assert!(
            kind_change_frame.contains(&cached_underlay),
            "modal-kind-change frame must re-emit the cached dimmed underlay so the \
             previous modal's pixels are wiped",
        );
    }

    #[test]
    fn modal_kind_change_does_not_rebuild_underlay() {
        // The cached underlay must be reused (not regenerated)
        // across a kind change — widgets did not change while a
        // modal was on top, so the snapshot is still valid; we
        // only need to re-emit it. Asserting snapshot identity by
        // value: the cached String content is identical before
        // and after the kind change.
        let config = config::Config::new();
        let theme = theme::Theme::new();
        let mut state = modal_state(crate::overlay::ActiveModal::Main(
            crate::overlay::main_menu::MainMenuState::new(),
        ));

        let _main_frame = compose_one_frame(&mut state, &config, &theme, fixture_size());
        let before = state
            .render
            .cached_dimmed_underlay()
            .expect("Main frame populates the cache")
            .to_string();

        state.overlay.active = crate::overlay::ActiveModal::Help(
            crate::overlay::help::HelpState::new(crate::overlay::ReturnTarget::Main),
        );
        let _help_frame = compose_one_frame(&mut state, &config, &theme, fixture_size());
        let after = state
            .render
            .cached_dimmed_underlay()
            .expect("cache must survive the kind change")
            .to_string();
        assert_eq!(
            before, after,
            "kind-change re-emit must not rebuild the dimmed underlay",
        );
    }

    #[test]
    fn options_to_options_edit_is_same_footprint() {
        // Options ↔ Options-with-edit-buffer share a footprint
        // (same modal box, only the box's interior content
        // changes). The dim-cache reconcile must treat this
        // transition as same-modal, returning ModalOnly so no
        // CLEAR_SCREEN is emitted.
        let config = config::Config::new();
        let theme = theme::Theme::new();
        let mut opts =
            crate::overlay::options::OptionsState::new(crate::overlay::ReturnTarget::Normal);
        let mut state = modal_state(crate::overlay::ActiveModal::Options(opts.clone()));

        let _first = compose_one_frame(&mut state, &config, &theme, fixture_size());

        // Enter the inline-edit sub-state. ActiveModal::Options
        // discriminant is unchanged; OverlayKind would flip from
        // Options to OptionsEdit but ModalFootprint stays Options.
        opts.enter_edit(crate::overlay::options::edit::OptionEditState::placeholder());
        state.overlay.active = crate::overlay::ActiveModal::Options(opts);
        let edit_frame = compose_one_frame(&mut state, &config, &theme, fixture_size());
        assert!(
            !edit_frame.contains(term::CLEAR_SCREEN),
            "Options ↔ OptionsEdit shares a footprint; transition must NOT emit \
             CLEAR_SCREEN; got: {edit_frame:?}",
        );
    }

    #[test]
    fn modal_close_drops_cache_and_skips_compose() {
        // Closing the modal entirely (dim_now → false) drops the
        // cached underlay so the next modal-open transition
        // rebuilds. We can observe this via reconcile's return
        // value: after close, the next compose call for a fresh
        // modal must be BuildAndEmit (cache empty), not
        // EmitFromCache.
        let mut state = make_state();

        // Open Main → BuildAndEmit (populates the cache).
        assert_eq!(
            state.render.reconcile_dim_compose(ModalFootprint::Main),
            DimComposeMode::BuildAndEmit,
        );
        state.render.store_dimmed_underlay("dummy".to_string());

        // Close → Skip, snapshot dropped.
        assert_eq!(
            state.render.reconcile_dim_compose(ModalFootprint::NoModal),
            DimComposeMode::Skip,
        );
        assert!(
            state.render.cached_dimmed_underlay().is_none(),
            "modal close must drop the dim-cache snapshot",
        );

        // Re-open Main → BuildAndEmit again (not EmitFromCache).
        assert_eq!(
            state.render.reconcile_dim_compose(ModalFootprint::Main),
            DimComposeMode::BuildAndEmit,
        );
    }
}
