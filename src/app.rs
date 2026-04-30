use crate::domain::process::ProcDisplayEntry;
use crate::{
    config,
    dirty::Dirty,
    draw,
    event::AppEvent,
    handlers,
    handlers::{InputContext, MenuState},
    input, runner, term, theme, theme_keys as tc, tools, ui,
};
use crossterm::event::Event;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

/// Run the main event loop: collect data, render UI, and handle input.
///
/// Events arrive from three sources through a single channel:
/// - Input thread: key presses and terminal resize
/// - Collection worker: snapshot-ready notifications
///
/// The loop blocks on `rx.recv()` (zero CPU when idle), drains all
/// queued events, then renders any dirty boxes in one frame.
pub fn run(config: &mut config::Config, terminal: &mut term::Terminal, theme: &mut theme::Theme) {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut worker = runner::CollectionWorker::start(config.update_ms as u64, event_tx.clone());
    spawn_input_thread(event_tx);
    let mut state = AppState::new(config, Instant::now());

    while let Ok(first) = event_rx.recv() {
        // Drain all queued events to batch work before rendering.
        let mut has_resize = matches!(first, AppEvent::Resize);
        let mut has_snapshot = matches!(first, AppEvent::SnapshotReady);
        let mut keys: Vec<input::Key> = Vec::new();
        if let AppEvent::Key(k) = first {
            keys.push(k);
        }
        for event in std::iter::from_fn(|| event_rx.try_recv().ok()) {
            match event {
                AppEvent::Resize => has_resize = true,
                AppEvent::SnapshotReady => has_snapshot = true,
                AppEvent::Key(k) => keys.push(k),
            }
        }

        // Process resize before keys — keys may draw overlays that need current dimensions.
        if has_resize {
            terminal.refresh();
            state.render.mark_resize();
        }
        let size = terminal_size(terminal);

        // Pull latest snapshot if new data arrived and the UI should consume it.
        if has_snapshot && (config.background_update || state.overlay.render_ui()) {
            pull_latest_snapshot(&mut state, config, &worker);
        }

        // Handle too-small terminal: render message, only accept quit.
        if is_too_small(size) {
            render_if_dirty_small(&mut state, config, terminal, theme, size);
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Handle waiting for first snapshot: render message, only accept quit.
        if state.snapshot.current.is_none() {
            render_if_dirty_waiting(&mut state, config, terminal, theme, size);
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Process key events.
        for key in &keys {
            if handle_input_key(key, &mut state, config, terminal, theme, &worker, size)
                == AppCommand::Quit
            {
                worker.shutdown();
                save_config_on_exit(config);
                return;
            }
        }

        // If an overlay just closed and we skipped a snapshot, catch up now.
        if state.overlay.render_ui() && has_snapshot && !config.background_update {
            pull_latest_snapshot(&mut state, config, &worker);
        }

        // Render dirty boxes.
        if state.overlay.render_ui() && !state.render.dirty.is_empty() {
            execute_dirty_work(&mut state, config, size);
            write_dirty_frame(&mut state, config, terminal, theme);
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
}

pub(crate) struct RuntimeState {
    pub(crate) rounded: bool,
    pub(crate) update_ms: u64,
}

impl RuntimeState {
    fn new(config: &config::Config) -> Self {
        Self {
            rounded: config.rounded_corners,
            update_ms: config.update_ms as u64,
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
    /// Combined procs list (live + stale) that entries' `proc_index` refers to.
    /// Only populated when `keep_dead_proc_usage` adds stale entries.
    pub(crate) display_procs: Option<Vec<crate::domain::process::ProcInfo>>,
    /// Recently-dead process entries kept for one additional display cycle.
    stale_procs: HashMap<u32, crate::domain::process::ProcInfo>,
    /// PIDs from the previous snapshot, used to detect newly-dead processes.
    prev_pids: HashSet<u32>,
    /// ProcInfo cache from the previous snapshot for stale-process lookup.
    prev_proc_cache: HashMap<u32, crate::domain::process::ProcInfo>,
}

impl ProcessViewState {
    fn new() -> Self {
        Self {
            start: 0,
            selected: 0,
            filter_text: String::new(),
            entries: Vec::new(),
            display_procs: None,
            stale_procs: HashMap::new(),
            prev_pids: HashSet::new(),
            prev_proc_cache: HashMap::new(),
        }
    }

    fn update_stale_procs(&mut self, snapshot: &runner::CollectionSnapshot, keep_dead: bool) {
        let active_pids: HashSet<u32> = snapshot
            .proc_data
            .procs
            .iter()
            .map(|proc| proc.pid)
            .collect();

        // Detect recently-dead PIDs and preserve their last-known data
        // for one additional display cycle.
        self.stale_procs.clear();
        if keep_dead {
            for &pid in &self.prev_pids {
                if !active_pids.contains(&pid)
                    && let Some(info) = self.prev_proc_cache.get(&pid)
                {
                    self.stale_procs.insert(pid, info.clone());
                }
            }
        }

        // Update prev_pids and prev_proc_cache for next cycle.
        self.prev_pids = active_pids;
        self.prev_proc_cache.clear();
        for proc in &snapshot.proc_data.procs {
            self.prev_proc_cache.insert(proc.pid, proc.clone());
        }
    }

    fn rebuild_entries(
        &mut self,
        snapshot: Option<&runner::CollectionSnapshot>,
        config: &mut config::Config,
    ) {
        let Some(snapshot) = snapshot else {
            self.entries.clear();
            self.display_procs = None;
            return;
        };

        // Build combined procs list with stale entries if keep_dead_proc_usage.
        let live_procs = &snapshot.proc_data.procs;
        let has_stale = config.keep_dead_proc_usage && !self.stale_procs.is_empty();
        if has_stale {
            let combined: Vec<crate::domain::process::ProcInfo> = live_procs
                .iter()
                .cloned()
                .chain(self.stale_procs.values().cloned())
                .collect();
            self.display_procs = Some(combined);
        } else {
            self.display_procs = None;
        }
        let procs: &[crate::domain::process::ProcInfo] =
            self.display_procs.as_deref().unwrap_or(live_procs);

        let sort_by = &config.proc_sorting;
        let reversed = config.proc_reversed;
        let filter = &config.proc_filter;
        let tree_mode = config.proc_tree;
        let aggregate = config.proc_aggregate;
        self.entries = crate::collect::process_display::build_proc_display_entries(
            procs, sort_by, reversed, filter, tree_mode, aggregate,
        );

        // Keep selection on the same row index (btop behavior).
        // Only track by PID when Follow mode is active.
        if !self.entries.is_empty() {
            self.selected = self.selected.min(self.entries.len() - 1);
        } else {
            self.selected = 0;
        }
        // Update selected_pid to reflect whatever process is now on this row.
        config.selected_pid = self
            .entries
            .get(self.selected)
            .and_then(|e| procs.get(e.proc_index))
            .map_or(0, |p| p.pid as i64);

        // Auto-scroll to followed process.
        let followed = config.followed_pid as u32;
        if followed > 0 {
            if let Some(idx) = self
                .entries
                .iter()
                .position(|e| procs.get(e.proc_index).is_some_and(|p| p.pid == followed))
            {
                self.selected = idx;
                config.selected_pid = followed as i64;
            } else {
                // Followed process died — unfollow.
                config.followed_pid = 0;
            }
        }
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

    fn reconcile(
        &mut self,
        snapshot: &runner::CollectionSnapshot,
        preferred: &str,
        dirty: &mut Dirty,
    ) {
        if snapshot.net.nets.is_empty() {
            if !self.selected_iface.is_empty() {
                self.selected_iface.clear();
                *dirty |= Dirty::NET_BOX;
            }
            return;
        }

        // If we have no selection yet, try the preferred interface from config
        if self.selected_iface.is_empty()
            && preferred != "Auto"
            && !preferred.is_empty()
            && snapshot.net.nets.iter().any(|n| n.name == preferred)
        {
            self.selected_iface = preferred.to_string();
            *dirty |= Dirty::NET_BOX;
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

/// Spawn a thread that blocks on `crossterm::event::read()` and forwards
/// key presses and resize events through the event channel.
///
/// The thread is not joined — it exits when `tx` is dropped (all senders
/// gone) or when the process exits.
fn spawn_input_thread(tx: std::sync::mpsc::Sender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            match crossterm::event::read() {
                Ok(Event::Key(key)) => {
                    if let Some(k) = input::translate_key(key)
                        && tx.send(AppEvent::Key(k)).is_err()
                    {
                        break;
                    }
                }
                Ok(Event::Resize(_, _)) if tx.send(AppEvent::Resize).is_err() => {
                    break;
                }
                _ => {}
            }
        }
    });
}

fn terminal_size(terminal: &term::Terminal) -> TerminalSize {
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
    state
        .process
        .update_stale_procs(&snapshot, config.keep_dead_proc_usage);
    state.snapshot.current = Some(snapshot);

    apply_startup_snapshot_config(state, config);
    reconcile_selected_iface(state, config);
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
    config.initial_shown_boxes = config.shown_boxes.clone();
    state.startup.boxes_initialized = true;
}

fn auto_add_gpu_boxes(config: &mut config::Config, gpu_count: usize) -> bool {
    if gpu_count == 0 {
        return false;
    }

    let shown = config.shown_boxes.clone();
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
        config.shown_boxes = boxes.join(" ");
    }
    changed
}

fn reconcile_selected_iface(state: &mut AppState, config: &config::Config) {
    let Some(snapshot) = state.snapshot.current.as_ref() else {
        return;
    };
    state
        .network
        .reconcile(snapshot, &config.net_iface, &mut state.render.dirty);
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

/// Render the "too small" message if dirty flags indicate it's needed.
fn render_if_dirty_small(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) {
    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.dirty.intersects(Dirty::ALL_BOXES)
    {
        let output = style_terminal_output(&render_too_small(size, theme), config, theme);
        let _ = terminal.write_synced(&output);
        state.render.clear_dirty();
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

/// Render the "Collecting data..." message if dirty flags indicate it's needed.
fn render_if_dirty_waiting(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) {
    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.dirty.intersects(Dirty::ALL_BOXES)
    {
        let output =
            style_terminal_output(&render_waiting_for_snapshot(size, theme), config, theme);
        let _ = terminal.write_synced(&output);
        state.render.clear_dirty();
    }
}

fn execute_dirty_work(state: &mut AppState, config: &mut config::Config, size: TerminalSize) {
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

fn rebuild_proc_list(state: &mut AppState, config: &mut config::Config) {
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
        .shown_boxes
        .split_whitespace()
        .map(String::from)
        .collect();
    let has_temp = config.check_temp && !snapshot.cpu.info.temp.is_empty();
    let has_watts = config.show_cpu_watts && snapshot.cpu.info.cpu_watts.is_some();
    let stats_rows = ui::cpu_box::stats_row_count(has_temp, has_watts);
    let cpu_panel_overhead = stats_rows + 2; // stats + load detail row + section divider

    draw::layout::calc_sizes(&draw::layout::LayoutConfig {
        term_width: size.width,
        term_height: size.height,
        shown_boxes: &shown,
        cpu_bottom: config.cpu_bottom,
        mem_below_net: config.mem_below_net,
        proc_left: config.proc_left,
        core_count: snapshot.cpu.info.core_count,
        gpu_count: snapshot.gpu.gpus.len(),
        disk_count: snapshot.disk.info.disks.len(),
        has_swap: config.show_swap && snapshot.mem.info.stats.swap_total > 0,
        cpu_panel_overhead,
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
        proc_display_procs: state.process.display_procs.as_deref(),
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

fn handle_input_key(
    key: &input::Key,
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    worker: &runner::CollectionWorker,
    size: TerminalSize,
) -> AppCommand {
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
    terminal.set_sync(ctx.config.terminal_sync);
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
    theme.style_output(output, config.theme_background)
}

fn save_config_on_exit(config: &config::Config) {
    if config.save_config_on_exit {
        let conf_path = tools::config_dir().join("rtop.toml");
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
    pub(crate) proc_display_procs: Option<&'a [crate::domain::process::ProcInfo]>,
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

    if dirty.intersects(Dirty::CPU_BOX)
        && let Some(ref cpu_dim) = layout.cpu
    {
        let area = ui::BoxArea::from_dim(cpu_dim, rounded);
        let cpu_settings = ui::cpu_box::CpuBoxSettings {
            graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                &config.graph_symbol_cpu,
                &config.graph_symbol,
            ),
            upper_source: &config.cpu_graph_upper,
            lower_source: &config.cpu_graph_lower,
            check_temp: config.check_temp,
            show_coretemp: config.show_coretemp,
            temp_scale: &config.temp_scale,
            single_graph: config.cpu_single_graph,
            update_ms,
            current_preset: config.current_preset,
            invert_lower: config.cpu_invert_lower,
            show_cpu_freq: config.show_cpu_freq,
            show_uptime: config.show_uptime,
            cpu_name: &snapshot.cpu.info.cpu_name,
            custom_cpu_name: &config.custom_cpu_name,
            show_cpu_watts: config.show_cpu_watts,
            cpu_watts: snapshot.cpu.info.cpu_watts,
            cpu_max_watts: snapshot.cpu.info.cpu_max_watts,
            clock_format: &config.clock_format,
        };
        output.push_str(&ui::cpu_box::draw(
            &snapshot.cpu.info,
            &area,
            theme,
            &cpu_settings,
            &snapshot.cpu.status,
        ));
    }

    if dirty.intersects(Dirty::GPU_BOX) {
        for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
            if gi < snapshot.gpu.gpus.len() {
                let area = ui::BoxArea::from_dim(gpu_dim, rounded);
                let custom_name = match gi {
                    0 => &config.custom_gpu_name0,
                    1 => &config.custom_gpu_name1,
                    2 => &config.custom_gpu_name2,
                    3 => &config.custom_gpu_name3,
                    4 => &config.custom_gpu_name4,
                    5 => &config.custom_gpu_name5,
                    _ => "",
                };
                let gpu_settings = ui::gpu_box::GpuBoxSettings {
                    index: gi,
                    temp_scale: &config.temp_scale,
                    custom_name,
                    base_10: config.base_10_sizes,
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

    if dirty.intersects(Dirty::MEM_BOX)
        && let Some(ref mem_dim) = layout.mem
    {
        let area = ui::BoxArea::from_dim(mem_dim, rounded);
        output.push_str(&ui::mem_box::draw(
            &snapshot.mem.info,
            &area,
            theme,
            &ui::mem_box::MemBoxSettings {
                show_swap: config.show_swap,
                base_10: config.base_10_sizes,
            },
            &snapshot.mem.status,
        ));
    }

    if dirty.intersects(Dirty::DISK_BOX)
        && let Some(ref disk_dim) = layout.disk
    {
        let area = ui::BoxArea::from_dim(disk_dim, rounded);
        let disk_settings = ui::disk_box::DiskBoxSettings {
            graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                &config.graph_symbol_disk,
                &config.graph_symbol,
            ),
            base_10: config.base_10_sizes,
            show_io_stat: config.show_io_stat,
            io_mode: config.io_mode,
            disk_io_mode: config.disk_io_mode,
            io_graph_combined: config.io_graph_combined,
        };
        output.push_str(&ui::disk_box::draw(
            &snapshot.disk.info,
            &area,
            theme,
            &disk_settings,
            &snapshot.disk.status,
        ));
    }

    if dirty.intersects(Dirty::NET_BOX)
        && let Some(ref net_dim) = layout.net
    {
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
            auto_scale: config.net_auto,
            sync_scale: config.net_sync,
            max_download: config.net_download,
            max_upload: config.net_upload,
            graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                &config.graph_symbol_net,
                &config.graph_symbol,
            ),
            swap_dl_ul: config.swap_upload_download,
            base_10: config.base_10_sizes,
        };
        output.push_str(&ui::net_box::draw(
            net_info,
            &area,
            theme,
            &net_settings,
            &snapshot.net.status,
        ));
    }

    if dirty.intersects(Dirty::PROC_BOX)
        && let Some(ref proc_dim) = layout.proc_box
    {
        let procs = params
            .proc_display_procs
            .unwrap_or(&snapshot.proc_data.procs);
        let entries = params.proc_entries;
        let detailed_pid = config.detailed_pid as u32;
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
        let sort_by = &config.proc_sorting;
        let reversed = config.proc_reversed;
        let tree_mode = config.proc_tree;
        let pf = &config.proc_filter;
        let area = ui::BoxArea::from_dim(proc_dim, rounded);
        let view = ui::ProcView {
            start: *proc_start,
            selected: *proc_selected,
            sort_by,
            sort_reversed: reversed,
            tree_mode,
            detailed_pid,
            followed_pid: config.followed_pid as u32,
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
            proc_per_core: config.proc_per_core,
            core_count: snapshot.cpu.info.core_count,
            proc_mem_bytes: config.proc_mem_bytes,
            total_mem,
            proc_colors: config.proc_colors,
            proc_gradient: config.proc_gradient,
            base_10: config.base_10_sizes,
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

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initializes_from_config() {
        let mut config = config::Config::new();
        config.rounded_corners = false;
        config.update_ms = 1_500;
        let now = Instant::now();

        let state = AppState::new(&config, now);

        assert!(!state.runtime.rounded);
        assert_eq!(state.runtime.update_ms, 1_500);
        assert!(state.overlay.menu_state == MenuState::None);
        assert_eq!(state.render.dirty, Dirty::FULL);
        assert!(state.render.cached_layout.is_none());
        assert!(state.snapshot.current.is_none());
        assert!(state.process.entries.is_empty());
        assert!(state.network.selected_iface.is_empty());
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
        config.shown_boxes = "cpu mem proc".to_string();

        assert!(auto_add_gpu_boxes(&mut config, 2));
        assert_eq!(&config.shown_boxes, "cpu mem proc gpu0 gpu1");
    }

    #[test]
    fn auto_add_gpu_boxes_ignores_existing_boxes() {
        let mut config = config::Config::new();
        config.shown_boxes = "cpu mem proc gpu0".to_string();

        assert!(!auto_add_gpu_boxes(&mut config, 1));
        assert_eq!(&config.shown_boxes, "cpu mem proc gpu0");
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

        reconcile_selected_iface(&mut state, &config);

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
