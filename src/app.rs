use crate::domain::process::ProcDisplayEntry;
use crate::{
    config,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    draw, handlers,
    handlers::{InputContext, MenuState},
    input, runner, term, theme, theme_keys as tc, tools, ui,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

const SNAPSHOT_POLL_MS: u64 = 50;
const PROC_CPU_HISTORY_LIMIT: usize = 300;

/// Run the main event loop: collect data, render UI, and handle input.
pub fn run(config: &mut config::Config, terminal: &mut term::Terminal, theme: &mut theme::Theme) {
    let mut worker = runner::CollectionWorker::start(config.get_int(ik::UPDATE_MS) as u64);
    let mut state = AppState::new(config, Instant::now());

    loop {
        let size = refresh_terminal(&mut state, terminal);
        pull_latest_snapshot(&mut state, config, &worker);

        if is_too_small(size) {
            if handle_small_terminal(&mut state, config, terminal, theme, size) == AppCommand::Quit
            {
                break;
            }
            continue;
        }

        if state.snapshot.current.is_none() {
            if handle_waiting_for_snapshot(&mut state, config, terminal, theme, size)
                == AppCommand::Quit
            {
                break;
            }
            continue;
        }

        if state.overlay.render_ui() && !state.render.dirty.is_empty() {
            execute_dirty_work(&mut state, config, size);
            write_dirty_frame(&mut state, config, terminal, theme);
        }

        if poll_and_handle_input(&mut state, config, terminal, theme, &worker, size)
            == AppCommand::Quit
        {
            break;
        }
    }

    worker.shutdown();
    save_config_on_exit(config);
}

struct AppState {
    runtime: RuntimeState,
    render: RenderState,
    snapshot: SnapshotState,
    overlay: OverlayState,
    process: ProcessViewState,
    network: NetworkViewState,
    startup: StartupState,
}

impl AppState {
    fn new(config: &config::Config, _now: Instant) -> Self {
        Self {
            runtime: RuntimeState::new(config),
            render: RenderState::new(),
            snapshot: SnapshotState::new(),
            overlay: OverlayState::new(),
            process: ProcessViewState::new(),
            network: NetworkViewState::new(),
            startup: StartupState::new(),
        }
    }

    fn poll_timeout_ms(&self) -> u64 {
        SNAPSHOT_POLL_MS
    }
}

pub(crate) struct RuntimeState {
    pub(crate) rounded: bool,
    pub(crate) update_ms: u64,
}

impl RuntimeState {
    fn new(config: &config::Config) -> Self {
        Self {
            rounded: config.get_bool(bk::ROUNDED_CORNERS),
            update_ms: config.get_int(ik::UPDATE_MS) as u64,
        }
    }
}

pub(crate) struct RenderState {
    pub(crate) dirty: Dirty,
    pub(crate) cached_layout: Option<draw::layout::Layout>,
    pub(crate) last_layout_hints: Option<runner::LayoutHints>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            dirty: Dirty::FULL,
            cached_layout: None,
            last_layout_hints: None,
        }
    }

    pub(crate) fn mark_resize(&mut self) {
        self.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = Dirty::empty();
    }
}

struct SnapshotState {
    current: Option<Arc<runner::CollectionSnapshot>>,
}

impl SnapshotState {
    fn new() -> Self {
        Self { current: None }
    }
}

pub(crate) struct OverlayState {
    pub(crate) menu_state: MenuState,
    pub(crate) menu_return_to: MenuState,
    pub(crate) main_menu_selected: usize,
    pub(crate) options_cat: usize,
    pub(crate) options_selected: usize,
    pub(crate) options_page: usize,
}

impl OverlayState {
    fn new() -> Self {
        Self {
            menu_state: MenuState::None,
            menu_return_to: MenuState::None,
            main_menu_selected: 0,
            options_cat: 0,
            options_selected: 0,
            options_page: 0,
        }
    }

    pub(crate) fn set_menu_state(&mut self, new: MenuState) {
        debug_assert!(
            self.menu_state.can_transition_to(new),
            "invalid menu transition: {:?} → {:?}",
            self.menu_state,
            new,
        );
        self.menu_state = new;
    }

    fn render_ui(&self) -> bool {
        self.menu_state == MenuState::None || self.menu_state == MenuState::Filter
    }
}

pub(crate) struct ProcessViewState {
    pub(crate) start: usize,
    pub(crate) selected: usize,
    pub(crate) filter_text: String,
    pub(crate) entries: Vec<ProcDisplayEntry>,
    pub(crate) cpu_histories: HashMap<u32, VecDeque<i64>>,
}

impl ProcessViewState {
    fn new() -> Self {
        Self {
            start: 0,
            selected: 0,
            filter_text: String::new(),
            entries: Vec::new(),
            cpu_histories: HashMap::new(),
        }
    }

    fn update_cpu_histories(&mut self, snapshot: &runner::CollectionSnapshot) {
        let active_pids: HashSet<u32> = snapshot
            .proc_data
            .procs
            .iter()
            .map(|proc| proc.pid)
            .collect();
        self.cpu_histories
            .retain(|pid, _| active_pids.contains(pid));

        let max_cpu = 100.0 * snapshot.cpu.info.core_count.max(1) as f64;
        for proc in &snapshot.proc_data.procs {
            let cpu = if proc.cpu_p.is_finite() {
                proc.cpu_p.clamp(0.0, max_cpu).round() as i64
            } else {
                0
            };
            let history = self.cpu_histories.entry(proc.pid).or_default();
            history.push_back(cpu);
            while history.len() > PROC_CPU_HISTORY_LIMIT {
                history.pop_front();
            }
        }
    }

    fn rebuild_entries(
        &mut self,
        snapshot: Option<&runner::CollectionSnapshot>,
        config: &config::Config,
    ) {
        let Some(snapshot) = snapshot else {
            self.entries.clear();
            return;
        };
        let sort_by = config.get_string(sk::PROC_SORTING);
        let reversed = config.get_bool(bk::PROC_REVERSED);
        let filter = config.get_string(sk::PROC_FILTER);
        let tree_mode = config.get_bool(bk::PROC_TREE);
        self.entries = crate::collect::process_display::build_proc_display_entries(
            &snapshot.proc_data.procs,
            sort_by,
            reversed,
            filter,
            tree_mode,
        );
    }
}

pub(crate) struct NetworkViewState {
    pub(crate) selected_iface: String,
}

impl NetworkViewState {
    fn new() -> Self {
        Self {
            selected_iface: String::new(),
        }
    }

    fn reconcile(&mut self, snapshot: &runner::CollectionSnapshot, dirty: &mut Dirty) {
        if snapshot.net.nets.is_empty() {
            if !self.selected_iface.is_empty() {
                self.selected_iface.clear();
                *dirty |= Dirty::NET_BOX;
            }
            return;
        }

        if self.selected_iface.is_empty()
            || !snapshot
                .net
                .nets
                .iter()
                .any(|n| n.name == self.selected_iface)
        {
            self.selected_iface = snapshot.net.nets[0].name.clone();
            *dirty |= Dirty::NET_BOX;
        }
    }
}

struct StartupState {
    boxes_initialized: bool,
}

impl StartupState {
    fn new() -> Self {
        Self {
            boxes_initialized: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSize {
    width: usize,
    height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppCommand {
    Continue,
    Quit,
}

fn refresh_terminal(state: &mut AppState, terminal: &mut term::Terminal) -> TerminalSize {
    if terminal.refresh() {
        state.render.mark_resize();
    }
    let (width, height) = terminal.size();
    TerminalSize {
        width: width as usize,
        height: height as usize,
    }
}

fn pull_latest_snapshot(
    state: &mut AppState,
    config: &mut config::Config,
    worker: &runner::CollectionWorker,
) {
    let last_seen = state.snapshot.current.as_ref().map_or(0, |s| s.seq);
    let Some(snapshot) = worker.latest_if_new(last_seen) else {
        return;
    };

    let new_hints = snapshot.layout_hints();
    if state
        .render
        .last_layout_hints
        .is_none_or(|hints| hints != new_hints)
    {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }
    state.render.last_layout_hints = Some(new_hints);
    state.process.update_cpu_histories(&snapshot);
    state.snapshot.current = Some(snapshot);

    apply_startup_snapshot_config(state, config);
    reconcile_selected_iface(state);
    state.render.dirty |= Dirty::ALL_BOXES | Dirty::PROC_LIST;
}

fn apply_startup_snapshot_config(state: &mut AppState, config: &mut config::Config) {
    if state.startup.boxes_initialized {
        return;
    }
    let Some(snapshot) = state.snapshot.current.as_ref() else {
        return;
    };

    if auto_add_gpu_boxes(config, snapshot.gpu.gpus.len()) {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }
    let initial = config.get_string(sk::SHOWN_BOXES).to_string();
    config.set_string(sk::INITIAL_SHOWN_BOXES, &initial);
    state.startup.boxes_initialized = true;
}

fn auto_add_gpu_boxes(config: &mut config::Config, gpu_count: usize) -> bool {
    if gpu_count == 0 {
        return false;
    }

    let shown = config.get_string(sk::SHOWN_BOXES).to_string();
    let mut boxes: Vec<String> = shown.split_whitespace().map(String::from).collect();
    let mut changed = false;
    for i in 0..gpu_count {
        let name = format!("gpu{i}");
        if !boxes.iter().any(|b| b == &name) {
            boxes.push(name);
            changed = true;
        }
    }
    if changed {
        config.set_string(sk::SHOWN_BOXES, &boxes.join(" "));
    }
    changed
}

fn reconcile_selected_iface(state: &mut AppState) {
    let Some(snapshot) = state.snapshot.current.as_ref() else {
        return;
    };
    state.network.reconcile(snapshot, &mut state.render.dirty);
}

fn is_too_small(size: TerminalSize) -> bool {
    size.width < draw::layout::MIN_TERM_WIDTH || size.height < draw::layout::MIN_TERM_HEIGHT
}

fn render_too_small(size: TerminalSize, theme: &theme::Theme) -> String {
    let min_w = draw::layout::MIN_TERM_WIDTH;
    let min_h = draw::layout::MIN_TERM_HEIGHT;
    let msg = format!(
        "Terminal too small ({}x{}). Need {}x{}.",
        size.width, size.height, min_w, min_h
    );
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!(
        "\x1b[2J\x1b[{msg_y};{msg_x}H\x1b[1m{}{msg}\x1b[0m",
        theme.color(tc::HI_FG)
    )
}

fn handle_small_terminal(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) -> AppCommand {
    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.dirty.intersects(Dirty::ALL_BOXES)
    {
        let output = style_terminal_output(&render_too_small(size, theme), config, theme);
        let _ = terminal.write_synced(&output);
        state.render.clear_dirty();
    }

    if input::poll(state.poll_timeout_ms())
        && input::get().is_some_and(|key| key == input::Key::Char('q'))
    {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn render_waiting_for_snapshot(size: TerminalSize, theme: &theme::Theme) -> String {
    let msg = "Collecting data...";
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!(
        "\x1b[2J\x1b[{msg_y};{msg_x}H\x1b[1m{}{msg}\x1b[0m",
        theme.color(tc::HI_FG)
    )
}

fn handle_waiting_for_snapshot(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) -> AppCommand {
    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.dirty.intersects(Dirty::ALL_BOXES)
    {
        let output =
            style_terminal_output(&render_waiting_for_snapshot(size, theme), config, theme);
        let _ = terminal.write_synced(&output);
        state.render.clear_dirty();
    }

    if input::poll(state.poll_timeout_ms())
        && input::get().is_some_and(|key| key == input::Key::Char('q'))
    {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn execute_dirty_work(state: &mut AppState, config: &config::Config, size: TerminalSize) {
    if state.render.dirty.contains(Dirty::PROC_LIST) {
        rebuild_proc_list(state, config);
    }

    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.cached_layout.is_none() {
        state.render.cached_layout = state
            .snapshot
            .current
            .as_ref()
            .map(|snapshot| calculate_layout(config, snapshot, size));
    }
}

fn rebuild_proc_list(state: &mut AppState, config: &config::Config) {
    state
        .process
        .rebuild_entries(state.snapshot.current.as_deref(), config);
}

fn calculate_layout(
    config: &config::Config,
    snapshot: &runner::CollectionSnapshot,
    size: TerminalSize,
) -> draw::layout::Layout {
    let shown: Vec<String> = config
        .get_string(sk::SHOWN_BOXES)
        .split_whitespace()
        .map(String::from)
        .collect();
    draw::layout::calc_sizes(&draw::layout::LayoutConfig {
        term_width: size.width,
        term_height: size.height,
        shown_boxes: &shown,
        cpu_bottom: config.get_bool(bk::CPU_BOTTOM),
        mem_below_net: config.get_bool(bk::MEM_BELOW_NET),
        proc_left: config.get_bool(bk::PROC_LEFT),
        core_count: snapshot.cpu.info.core_count,
        gpu_count: snapshot.gpu.gpus.len(),
        disk_count: snapshot.disk.info.disks.len(),
        has_swap: snapshot.mem.info.stats.swap_total > 0,
    })
}

fn write_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
) {
    let output = render_dirty_frame(state, config, theme);
    let output = style_terminal_output(&output, config, theme);
    if let Err(e) = terminal.write_synced(&output) {
        tracing::debug!("terminal write failed: {e}");
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
    let snapshot = state
        .snapshot
        .current
        .as_ref()
        .expect("snapshot must be initialized before rendering");
    let mut output = String::new();

    if state.render.dirty.contains(Dirty::LAYOUT) {
        output.push_str("\x1b[2J");
    }

    let params = RenderParams {
        dirty: state.render.dirty,
        layout,
        snapshot,
        proc_entries: &state.process.entries,
        proc_cpu_histories: &state.process.cpu_histories,
        selected_iface: &state.network.selected_iface,
        config,
        theme,
        rounded: state.runtime.rounded,
        update_ms: state.runtime.update_ms,
        is_filtering: state.overlay.menu_state == MenuState::Filter,
    };
    output.push_str(&render_all(
        &params,
        &mut state.process.selected,
        &mut state.process.start,
    ));
    output
}

fn poll_and_handle_input(
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    worker: &runner::CollectionWorker,
    size: TerminalSize,
) -> AppCommand {
    if !input::poll(state.poll_timeout_ms()) {
        return AppCommand::Continue;
    }

    let Some(key) = input::get() else {
        return AppCommand::Continue;
    };

    handle_input_key(&key, state, config, terminal, theme, worker, size)
}

fn handle_input_key(
    key: &input::Key,
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    worker: &runner::CollectionWorker,
    size: TerminalSize,
) -> AppCommand {
    if key.is_mouse() || *key == input::Key::Resize {
        if *key == input::Key::Resize {
            state.render.mark_resize();
        }
        return AppCommand::Continue;
    }

    let mut ctx = InputContext {
        config,
        theme,
        worker,
        snapshot: state.snapshot.current.as_deref(),
        runtime: &mut state.runtime,
        render: &mut state.render,
        overlay: &mut state.overlay,
        process: &mut state.process,
        network: &mut state.network,
        tw: size.width,
        th: size.height,
    };
    let result = dispatch_handler(key, &mut ctx);
    execute_terminal_ops(terminal, ctx.config, ctx.theme, &result);
    if result.redraw_overlay {
        let out = handlers::redraw_after_overlay(&mut ctx);
        let out = style_terminal_output(&out, ctx.config, ctx.theme);
        let _ = terminal.write_synced(&out);
    }

    if result.quit {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn dispatch_handler(key: &input::Key, ctx: &mut InputContext) -> handlers::HandleResult {
    match ctx.overlay.menu_state {
        MenuState::Main => handlers::main_menu::handle(key, ctx),
        MenuState::Help => handlers::help::handle(key, ctx),
        MenuState::Options => handlers::options::handle(key, ctx),
        MenuState::Filter => handlers::filter::handle(key, ctx),
        MenuState::None => handlers::normal::handle(key, ctx),
    }
}

fn execute_terminal_ops(
    terminal: &mut term::Terminal,
    config: &config::Config,
    theme: &theme::Theme,
    result: &handlers::HandleResult,
) {
    for op in &result.ops {
        let styled = match op {
            handlers::TerminalOp::Raw(s) | handlers::TerminalOp::Synced(s) => {
                style_terminal_output(s, config, theme)
            }
        };
        match op {
            handlers::TerminalOp::Raw(_) => {
                let _ = terminal.write_raw(&styled);
            }
            handlers::TerminalOp::Synced(_) => {
                let _ = terminal.write_synced(&styled);
            }
        }
    }
}

fn style_terminal_output(output: &str, config: &config::Config, theme: &theme::Theme) -> String {
    theme.style_output(output, config.get_bool(bk::THEME_BACKGROUND))
}

fn save_config_on_exit(config: &config::Config) {
    if config.get_bool(bk::SAVE_CONFIG_ON_EXIT) {
        let conf_path = tools::config_dir().join("rtop.conf");
        let _ = config.write(&conf_path);
    }
}

fn clamp_proc_selection(
    count: usize,
    box_height: usize,
    detail_rows: usize,
    selected: &mut usize,
    start: &mut usize,
) {
    let max_visible = ui::proc_box::visible_row_count(box_height, detail_rows);
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

/// Parameters for rendering the UI boxes.
pub(crate) struct RenderParams<'a> {
    pub(crate) dirty: Dirty,
    pub(crate) layout: &'a draw::layout::Layout,
    pub(crate) snapshot: &'a runner::CollectionSnapshot,
    pub(crate) proc_entries: &'a [ProcDisplayEntry],
    pub(crate) proc_cpu_histories: &'a HashMap<u32, VecDeque<i64>>,
    pub(crate) selected_iface: &'a str,
    pub(crate) config: &'a config::Config,
    pub(crate) theme: &'a theme::Theme,
    pub(crate) rounded: bool,
    pub(crate) update_ms: u64,
    pub(crate) is_filtering: bool,
}

/// Render UI boxes into an ANSI output string.
///
/// Only renders boxes whose corresponding dirty flag is set.
/// Pass `Dirty::ALL_BOXES` to render everything.
pub(crate) fn render_all(
    params: &RenderParams,
    proc_selected: &mut usize,
    proc_start: &mut usize,
) -> String {
    let dirty = params.dirty;
    let layout = params.layout;
    let snapshot = params.snapshot;
    let config = params.config;
    let theme = params.theme;
    let rounded = params.rounded;
    let update_ms = params.update_ms;
    let is_filtering = params.is_filtering;
    let mut output = String::new();

    if dirty.intersects(Dirty::CPU_BOX) {
        if let Some(ref cpu_dim) = layout.cpu {
            let area = ui::BoxArea::from_dim(cpu_dim, rounded);
            let cpu_settings = ui::cpu_box::CpuBoxSettings {
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_CPU),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
                upper_source: config.get_string(sk::CPU_GRAPH_UPPER),
                lower_source: config.get_string(sk::CPU_GRAPH_LOWER),
                check_temp: config.get_bool(bk::CHECK_TEMP),
                show_coretemp: config.get_bool(bk::SHOW_CORETEMP),
                temp_scale: config.get_string(sk::TEMP_SCALE),
                single_graph: config.get_bool(bk::CPU_SINGLE_GRAPH),
                update_ms,
                current_preset: config.get_int(ik::CURRENT_PRESET),
            };
            output.push_str(&ui::cpu_box::draw(
                &snapshot.cpu.info,
                &area,
                theme,
                &cpu_settings,
                &snapshot.cpu.status,
            ));
        }
    }

    if dirty.intersects(Dirty::GPU_BOX) {
        for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
            if gi < snapshot.gpu.gpus.len() {
                let area = ui::BoxArea::from_dim(gpu_dim, rounded);
                let gpu_settings = ui::gpu_box::GpuBoxSettings {
                    index: gi,
                    temp_scale: config.get_string(sk::TEMP_SCALE),
                };
                output.push_str(&ui::gpu_box::draw(
                    &snapshot.gpu.gpus[gi],
                    &area,
                    theme,
                    &gpu_settings,
                    &snapshot.gpu.status,
                ));
            }
        }
    }

    if dirty.intersects(Dirty::MEM_BOX) {
        if let Some(ref mem_dim) = layout.mem {
            let area = ui::BoxArea::from_dim(mem_dim, rounded);
            output.push_str(&ui::mem_box::draw(
                &snapshot.mem.info,
                &area,
                theme,
                &ui::mem_box::MemBoxSettings {
                    show_swap: config.get_bool(bk::SHOW_SWAP),
                },
                &snapshot.mem.status,
            ));
        }
    }

    if dirty.intersects(Dirty::DISK_BOX) {
        if let Some(ref disk_dim) = layout.disk {
            let area = ui::BoxArea::from_dim(disk_dim, rounded);
            let disk_settings = ui::disk_box::DiskBoxSettings {
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_DISK),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
            };
            output.push_str(&ui::disk_box::draw(
                &snapshot.disk.info,
                &area,
                theme,
                &disk_settings,
                &snapshot.disk.status,
            ));
        }
    }

    if dirty.intersects(Dirty::NET_BOX) {
        if let Some(ref net_dim) = layout.net {
            let iface = params.selected_iface;
            let default_net = crate::domain::network::NetInfo::default();
            let net_info = snapshot
                .net
                .nets
                .iter()
                .find(|n| n.name == iface)
                .unwrap_or(&default_net);
            let area = ui::BoxArea::from_dim(net_dim, rounded);
            let net_settings = ui::net_box::NetBoxSettings {
                iface,
                auto_scale: config.get_bool(bk::NET_AUTO),
                sync_scale: config.get_bool(bk::NET_SYNC),
                max_download: config.get_int(ik::NET_DOWNLOAD),
                max_upload: config.get_int(ik::NET_UPLOAD),
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_NET),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
            };
            output.push_str(&ui::net_box::draw(
                net_info,
                &area,
                theme,
                &net_settings,
                &snapshot.net.status,
            ));
        }
    }

    if dirty.intersects(Dirty::PROC_BOX) {
        if let Some(ref proc_dim) = layout.proc_box {
            let procs = &snapshot.proc_data.procs;
            let entries = params.proc_entries;
            let detailed_pid = config.get_int(ik::DETAILED_PID) as u32;
            let detail_rows = if detailed_pid > 0 {
                8_usize.min(proc_dim.height.saturating_sub(6))
            } else {
                0
            };
            clamp_proc_selection(
                entries.len(),
                proc_dim.height,
                detail_rows,
                proc_selected,
                proc_start,
            );
            let sort_by = config.get_string(sk::PROC_SORTING);
            let reversed = config.get_bool(bk::PROC_REVERSED);
            let tree_mode = config.get_bool(bk::PROC_TREE);
            let pf = config.get_string(sk::PROC_FILTER);
            let area = ui::BoxArea::from_dim(proc_dim, rounded);
            let view = ui::ProcView {
                start: *proc_start,
                selected: *proc_selected,
                sort_by,
                sort_reversed: reversed,
                tree_mode,
                detailed_pid,
                filter: pf,
                filtering: is_filtering,
            };
            let total_mem = snapshot
                .mem
                .info
                .stats
                .used
                .saturating_add(snapshot.mem.info.stats.available);
            let proc_settings = ui::proc_box::ProcBoxSettings {
                proc_per_core: config.get_bool(bk::PROC_PER_CORE),
                core_count: snapshot.cpu.info.core_count,
                proc_mem_bytes: config.get_bool(bk::PROC_MEM_BYTES),
                total_mem,
                proc_colors: config.get_bool(bk::PROC_COLORS),
                proc_gradient: config.get_bool(bk::PROC_GRADIENT),
                proc_cpu_graphs: config.get_bool(bk::PROC_CPU_GRAPHS),
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_PROC),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
                cpu_histories: params.proc_cpu_histories,
            };
            output.push_str(&ui::proc_box::draw(
                procs,
                entries,
                &area,
                theme,
                &proc_settings,
                &view,
                &snapshot.proc_data.status,
            ));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initializes_from_config() {
        let mut config = config::Config::new();
        config.set_bool(bk::ROUNDED_CORNERS, false);
        config.set_int(ik::UPDATE_MS, 1_500);
        let now = Instant::now();

        let state = AppState::new(&config, now);

        assert!(!state.runtime.rounded);
        assert_eq!(state.runtime.update_ms, 1_500);
        assert!(state.overlay.menu_state == MenuState::None);
        assert_eq!(state.render.dirty, Dirty::FULL);
        assert!(state.render.cached_layout.is_none());
        assert!(state.snapshot.current.is_none());
        assert!(state.process.entries.is_empty());
        assert!(state.process.cpu_histories.is_empty());
        assert!(state.network.selected_iface.is_empty());
    }

    #[test]
    fn update_proc_cpu_histories_tracks_prunes_and_caps() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        let mut runner = runner::Runner {
            cpu: crate::collect::cpu::CpuCollector::default(),
            disk: crate::collect::disk::DiskCollector::default(),
            gpu: crate::collect::gpu::GpuCollector::default(),
            mem: crate::collect::memory::MemCollector::default(),
            net: crate::collect::network::NetCollector::default(),
            proc_collector: crate::collect::process::ProcCollector::default(),
        };
        runner.cpu.info.core_count = 2;
        runner.proc_collector.procs = vec![
            crate::domain::process::ProcInfo {
                pid: 1,
                cpu_p: 50.0,
                ..Default::default()
            },
            crate::domain::process::ProcInfo {
                pid: 2,
                cpu_p: f64::NAN,
                ..Default::default()
            },
        ];

        state.process.update_cpu_histories(&runner.snapshot(1));

        assert_eq!(
            state.process.cpu_histories.get(&1).unwrap().back().copied(),
            Some(50)
        );
        assert_eq!(
            state.process.cpu_histories.get(&2).unwrap().back().copied(),
            Some(0)
        );

        runner.proc_collector.procs = vec![crate::domain::process::ProcInfo {
            pid: 1,
            cpu_p: 500.0,
            ..Default::default()
        }];
        for seq in 2..(PROC_CPU_HISTORY_LIMIT + 4) as u64 {
            state.process.update_cpu_histories(&runner.snapshot(seq));
        }

        assert!(!state.process.cpu_histories.contains_key(&2));
        let history = state.process.cpu_histories.get(&1).unwrap();
        assert_eq!(history.len(), PROC_CPU_HISTORY_LIMIT);
        assert_eq!(history.back().copied(), Some(200));
    }

    #[test]
    fn app_state_render_ui_only_for_normal_and_filter() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        state.overlay.menu_state = MenuState::None;
        assert!(state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Filter;
        assert!(state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Main;
        assert!(!state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Help;
        assert!(!state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Options;
        assert!(!state.overlay.render_ui());
    }

    #[test]
    fn poll_timeout_uses_snapshot_poll_interval() {
        let config = config::Config::new();
        let state = AppState::new(&config, Instant::now());

        assert_eq!(state.poll_timeout_ms(), SNAPSHOT_POLL_MS);
    }

    #[test]
    fn mark_resize_sets_layout_and_all_boxes() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.render.clear_dirty();

        state.render.mark_resize();

        assert!(state.render.dirty.contains(Dirty::LAYOUT));
        assert!(state.render.dirty.contains(Dirty::ALL_BOXES));
    }

    #[test]
    fn auto_add_gpu_boxes_adds_missing_boxes() {
        let mut config = config::Config::new();
        config.set_string(sk::SHOWN_BOXES, "cpu mem proc");

        assert!(auto_add_gpu_boxes(&mut config, 2));
        assert_eq!(config.get_string(sk::SHOWN_BOXES), "cpu mem proc gpu0 gpu1");
    }

    #[test]
    fn auto_add_gpu_boxes_ignores_existing_boxes() {
        let mut config = config::Config::new();
        config.set_string(sk::SHOWN_BOXES, "cpu mem proc gpu0");

        assert!(!auto_add_gpu_boxes(&mut config, 1));
        assert_eq!(config.get_string(sk::SHOWN_BOXES), "cpu mem proc gpu0");
    }

    #[test]
    fn reconcile_selected_iface_selects_first_available_interface() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.render.clear_dirty();
        let mut runner = runner::Runner {
            cpu: crate::collect::cpu::CpuCollector::default(),
            disk: crate::collect::disk::DiskCollector::default(),
            gpu: crate::collect::gpu::GpuCollector::default(),
            mem: crate::collect::memory::MemCollector::default(),
            net: crate::collect::network::NetCollector::default(),
            proc_collector: crate::collect::process::ProcCollector::default(),
        };
        runner.net.nets = vec![
            crate::domain::network::NetInfo {
                name: "Ethernet".into(),
                ..Default::default()
            },
            crate::domain::network::NetInfo {
                name: "Wi-Fi".into(),
                ..Default::default()
            },
        ];
        state.snapshot.current = Some(Arc::new(runner.snapshot(1)));

        reconcile_selected_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Ethernet");
        assert!(state.render.dirty.contains(Dirty::NET_BOX));
    }

    #[test]
    fn clamp_proc_selection_allows_last_visible_row() {
        let mut selected = 3;
        let mut start = 0;

        clamp_proc_selection(10, 8, 0, &mut selected, &mut start);

        assert_eq!(selected, 3);
        assert_eq!(start, 0);
    }

    #[test]
    fn terminal_size_checks_minimum_dimensions() {
        assert!(is_too_small(TerminalSize {
            width: draw::layout::MIN_TERM_WIDTH - 1,
            height: draw::layout::MIN_TERM_HEIGHT,
        }));
        assert!(is_too_small(TerminalSize {
            width: draw::layout::MIN_TERM_WIDTH,
            height: draw::layout::MIN_TERM_HEIGHT - 1,
        }));
        assert!(!is_too_small(TerminalSize {
            width: draw::layout::MIN_TERM_WIDTH,
            height: draw::layout::MIN_TERM_HEIGHT,
        }));
    }

    #[test]
    fn too_small_message_includes_actual_and_required_size() {
        let out = render_too_small(
            TerminalSize {
                width: 40,
                height: 10,
            },
            &theme::Theme::new(),
        );

        assert!(out.contains("Terminal too small (40x10)."));
        assert!(out.contains(&format!(
            "Need {}x{}.",
            draw::layout::MIN_TERM_WIDTH,
            draw::layout::MIN_TERM_HEIGHT
        )));
    }
}
