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
use crate::app::state::{AppState, LiveData, NetworkViewState, ProcessViewState, RuntimeState};
use crate::config;
use crate::dirty::Dirty;
use crate::domain::process::ProcDisplayEntry;
use crate::draw;
use crate::handlers::MenuState;
use crate::runner;
use crate::term;
use crate::theme;
use crate::ui;

pub(crate) fn execute_dirty_work(
    state: &mut AppState,
    config: &mut config::Config,
    size: TerminalSize,
) {
    if state.render.dirty.contains(Dirty::PROC_LIST) {
        rebuild_proc_list(state, config);
    }

    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.cached_layout.is_none() {
        state.render.cached_layout = Some(calculate_layout(state, config, size));
    }

    // Pre-render normalisation: clamp the proc widget's view-state to
    // the current entry count and widget dimensions. Done here (not
    // inside render_all) so that the render path stays a pure
    // function of (state, dirty).
    if let Some(layout) = state.render.cached_layout.as_ref()
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
    let procs = state.live.proc_data.as_ref().map(|s| s.procs.as_slice());
    state.process.rebuild_entries(procs, config);
}

fn calculate_layout(
    state: &AppState,
    config: &config::Config,
    size: TerminalSize,
) -> draw::layout::Layout {
    draw::layout::calc_sizes(&draw::layout::LayoutConfig {
        term_width: size.width,
        term_height: size.height,
        root: config.layout_spec(),
        hints: state.live.layout_hints(config),
        hidden: state.compose_hidden(config),
    })
}

pub(crate) fn write_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
) {
    let output = render_dirty_frame(state, config, theme);
    let output = style_terminal_output(&output, config, theme);
    if let Err(e) = terminal.write_synced(&output) {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Terminal,
            error = %e,
            "terminal write failed",
        );
    }
    state.render.clear_dirty();
}

fn render_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    theme: &theme::Theme,
) -> String {
    let layout = state
        .render
        .cached_layout
        .as_ref()
        .expect("layout must be initialized before rendering");
    let mut output = String::new();

    if state.render.dirty.contains(Dirty::LAYOUT) {
        output.push_str(term::CLEAR_SCREEN);
    }

    let params = RenderInputs {
        layout,
        live: &state.live,
        process: &state.process,
        network: &state.network,
        runtime: &state.runtime,
        filter: &state.filter,
        config,
        theme,
        dirty: state.render.dirty,
        is_filtering: state.overlay.menu_state == MenuState::Filter,
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
    pub(crate) dirty: Dirty,
    pub(crate) layout: &'a draw::layout::Layout,
    pub(crate) cpu: Option<&'a runner::CpuSnapshot>,
    pub(crate) mem: Option<&'a runner::MemSnapshot>,
    pub(crate) disk: Option<&'a runner::DiskSnapshot>,
    pub(crate) net: Option<&'a runner::NetSnapshot>,
    pub(crate) gpu: Option<&'a runner::GpuSnapshot>,
    pub(crate) proc_data: Option<&'a runner::ProcSnapshot>,
    pub(crate) proc_entries: &'a [ProcDisplayEntry],
    pub(crate) proc_display_procs: Option<&'a [crate::domain::process::ProcInfo]>,
    pub(crate) selected_iface: &'a str,
    pub(crate) config: &'a config::Config,
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
    pub(crate) runtime: &'a RuntimeState,
    pub(crate) filter: &'a crate::app::WidgetFilter,
    pub(crate) config: &'a config::Config,
    pub(crate) theme: &'a theme::Theme,
    /// Which widgets to render this frame.
    pub(crate) dirty: Dirty,
    /// `true` while the user is in `MenuState::Filter` so the proc
    /// widget can show its inline filter prompt.
    pub(crate) is_filtering: bool,
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
            proc_display_procs: self.process.display_procs.as_deref(),
            selected_iface: self.network.selected_iface.as_str(),
            config: self.config,
            theme: self.theme,
            rounded: self.runtime.rounded,
            update_ms: self.runtime.update_ms,
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
    let dirty = params.dirty;
    let layout = params.layout;
    let config = params.config;
    let theme = params.theme;
    let rounded = params.rounded;
    let update_ms = params.update_ms;
    let is_filtering = params.is_filtering;
    let mut output = String::new();

    if dirty.intersects(Dirty::CPU_WIDGET)
        && let Some(cpu_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Cpu)
        && let Some(cpu) = params.cpu
    {
        let area = ui::WidgetArea::from_dim(cpu_dim, rounded);
        let cpu_settings =
            ui::cpu_widget::build_settings(config, &cpu.info, update_ms, params.filter_active);
        output.push_str(&ui::cpu_widget::draw(
            &cpu.info,
            &area,
            theme,
            &cpu_settings,
            &cpu.status,
        ));
    }

    if dirty.intersects(Dirty::GPU_WIDGET)
        && let Some(gpu) = params.gpu
    {
        // Iterate by actual GPU index n. Layout slots are keyed by
        // WidgetKind::Gpu(n), so a sparse selection (e.g. only
        // gpu1) renders gpu.gpus[1] with the correct title and
        // toggle key. The defensive bounds check on gpu.gpus
        // covers the narrow window where layout was computed
        // against an older device count.
        for n in 0..config::MAX_GPUS {
            let kind = crate::domain::widget_kind::WidgetKind::Gpu(n as u8);
            let Some(gpu_dim) = layout.dims_for(kind) else {
                continue;
            };
            let Some(gpu_info) = gpu.gpus.get(n) else {
                continue;
            };
            let area = ui::WidgetArea::from_dim(gpu_dim, rounded);
            let gpu_settings = ui::gpu_widget::build_settings(config, n);
            output.push_str(&ui::gpu_widget::draw(
                gpu_info,
                &area,
                theme,
                &gpu_settings,
                &gpu.status,
            ));
        }
    }

    if dirty.intersects(Dirty::MEM_WIDGET)
        && let Some(mem_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Mem)
        && let Some(mem) = params.mem
    {
        let area = ui::WidgetArea::from_dim(mem_dim, rounded);
        output.push_str(&ui::mem_widget::draw(
            &mem.info,
            &area,
            theme,
            &ui::mem_widget::build_settings(config),
            &mem.status,
        ));
    }

    if dirty.intersects(Dirty::DISK_WIDGET)
        && let Some(disk_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Disk)
        && let Some(disk) = params.disk
    {
        let area = ui::WidgetArea::from_dim(disk_dim, rounded);
        let disk_settings = ui::disk_widget::build_settings(config);
        let filter = crate::domain::disk::DisksFilter::parse(&config.disks_filter);
        let visible = filter.apply(&disk.info.disks);
        output.push_str(&ui::disk_widget::draw(
            &visible,
            &area,
            theme,
            &disk_settings,
            &disk.status,
        ));
    }

    if dirty.intersects(Dirty::NET_WIDGET)
        && let Some(net_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Net)
        && let Some(net) = params.net
    {
        let iface = params.selected_iface;
        let default_net = crate::domain::network::NetInfo::default();
        let net_info = net
            .nets
            .iter()
            .find(|n| n.name == iface)
            .unwrap_or(&default_net);
        let area = ui::WidgetArea::from_dim(net_dim, rounded);
        let net_settings = ui::net_widget::build_settings(config, iface);
        output.push_str(&ui::net_widget::draw(
            net_info,
            &area,
            theme,
            &net_settings,
            &net.status,
        ));
    }

    if dirty.intersects(Dirty::PROC_WIDGET)
        && let Some(proc_dim) = layout.dims_for(crate::domain::widget_kind::WidgetKind::Proc)
        && let Some(proc_snap) = params.proc_data
    {
        let procs = params.proc_display_procs.unwrap_or(&proc_snap.procs);
        let entries = params.proc_entries;
        let detailed_pid = params.detailed_pid;
        let sort_by = config.proc_sorting;
        let reversed = config.proc_reversed;
        let tree_mode = config.proc_tree;
        let pf = &config.proc_filter;
        let area = ui::WidgetArea::from_dim(proc_dim, rounded);
        let view = ui::ProcView {
            start: params.proc_start,
            selected: params.proc_selected,
            sort_by,
            sort_reversed: reversed,
            tree_mode,
            detailed_pid,
            followed_pid: params.followed_pid,
            filter: pf,
            filtering: is_filtering,
            armed_name: params
                .armed_terminate
                .as_ref()
                .map(|(n, _)| *n)
                .unwrap_or(""),
            armed_force: params.armed_terminate.as_ref().is_some_and(|(_, f)| *f),
        };
        let proc_settings =
            ui::proc_widget::build_settings(config, params.core_count, params.total_mem);
        output.push_str(&ui::proc_widget::draw(
            procs,
            entries,
            &area,
            theme,
            &proc_settings,
            &view,
            &proc_snap.status,
        ));
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
