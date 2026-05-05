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
/// - Collector threads: per-subsystem ready notifications
///
/// The loop blocks on `rx.recv()` (zero CPU when idle), drains all
/// queued events, then renders any dirty boxes in one frame.
pub fn run(config: &mut config::Config, terminal: &mut term::Terminal, theme: &mut theme::Theme) {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut manager = runner::CollectorManager::start(config.update_ms as u64, event_tx.clone());
    spawn_input_thread(event_tx);
    let mut state = AppState::new(config, Instant::now());
    tracing::info!(subsystem = %crate::log::Subsystem::Startup, "ready");

    while let Ok(first) = event_rx.recv() {
        // Drain all queued events to batch work before rendering.
        let mut has_resize = matches!(first, AppEvent::Resize);
        let mut has_cpu = matches!(first, AppEvent::CpuReady);
        let mut has_mem = matches!(first, AppEvent::MemReady);
        let mut has_disk = matches!(first, AppEvent::DiskReady);
        let mut has_net = matches!(first, AppEvent::NetReady);
        let mut has_gpu = matches!(first, AppEvent::GpuReady);
        let mut has_proc = matches!(first, AppEvent::ProcReady);
        let mut keys: Vec<input::Key> = Vec::new();
        if let AppEvent::Key(k) = first {
            keys.push(k);
        }
        for event in std::iter::from_fn(|| event_rx.try_recv().ok()) {
            match event {
                AppEvent::Resize => has_resize = true,
                AppEvent::CpuReady => has_cpu = true,
                AppEvent::MemReady => has_mem = true,
                AppEvent::DiskReady => has_disk = true,
                AppEvent::NetReady => has_net = true,
                AppEvent::GpuReady => has_gpu = true,
                AppEvent::ProcReady => has_proc = true,
                AppEvent::Key(k) => keys.push(k),
            }
        }

        // Process resize before keys — keys may draw overlays that need current dimensions.
        if has_resize {
            let changed = terminal.refresh();
            if changed {
                let (w, h) = terminal.size();
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::Ui,
                    w,
                    h,
                    "terminal resized",
                );
            }
            state.render.mark_resize();
        }
        let size = terminal_size(terminal);

        // Always consume slot data into LiveData regardless of overlay state.
        let render_ui = config.background_update || state.overlay.render_ui();
        let ready = SubsystemReady {
            cpu: has_cpu,
            mem: has_mem,
            disk: has_disk,
            net: has_net,
            gpu: has_gpu,
            proc_data: has_proc,
        };
        pull_subsystem_data(&mut state, config, &manager, render_ui, &ready);

        // Handle too-small terminal: render message, only accept quit.
        if is_too_small(size) {
            render_if_dirty_small(&mut state, config, terminal, theme, size);
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Handle waiting for first data: render message, only accept quit.
        if !state.live.is_ready() {
            render_if_dirty_waiting(&mut state, config, terminal, theme, size);
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Process key events.
        for key in &keys {
            if handle_input_key(key, &mut state, config, terminal, theme, &manager, size)
                == AppCommand::Quit
            {
                tracing::info!(subsystem = %crate::log::Subsystem::Startup, "exiting");
                manager.shutdown();
                save_config_on_exit(config);
                return;
            }
        }

        // Render dirty boxes.
        if state.overlay.render_ui() && !state.render.dirty.is_empty() {
            execute_dirty_work(&mut state, config, size);
            write_dirty_frame(&mut state, config, terminal, theme);
        }
    }

    tracing::info!(subsystem = %crate::log::Subsystem::Startup, "exiting");
    manager.shutdown();
    save_config_on_exit(config);
}

struct AppState {
    runtime: RuntimeState,
    render: RenderState,
    live: LiveData,
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
            live: LiveData::new(),
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

pub(crate) struct LiveData {
    pub(crate) cpu: Option<Arc<runner::CpuSnapshot>>,
    pub(crate) mem: Option<Arc<runner::MemSnapshot>>,
    pub(crate) disk: Option<Arc<runner::DiskSnapshot>>,
    pub(crate) net: Option<Arc<runner::NetSnapshot>>,
    pub(crate) gpu: Option<Arc<runner::GpuSnapshot>>,
    pub(crate) proc_data: Option<Arc<runner::ProcSnapshot>>,
    /// Cached core count for proc box (stable hardware constant).
    pub(crate) core_count: usize,
    /// Cached total physical memory for proc box (stable hardware constant).
    pub(crate) total_mem: u64,
}

impl LiveData {
    fn new() -> Self {
        Self {
            cpu: None,
            mem: None,
            disk: None,
            net: None,
            gpu: None,
            proc_data: None,
            core_count: 0,
            total_mem: 0,
        }
    }

    /// True when all subsystems have provided at least one data point.
    fn is_ready(&self) -> bool {
        self.cpu.is_some()
            && self.mem.is_some()
            && self.disk.is_some()
            && self.net.is_some()
            && self.gpu.is_some()
            && self.proc_data.is_some()
    }

    fn layout_hints(&self, config: &config::Config) -> runner::LayoutHints {
        runner::LayoutHints {
            core_count: self.core_count,
            gpu_count: self.gpu.as_ref().map_or(0, |g| g.gpus.len()),
            disk_count: filtered_disk_count(self.disk.as_deref(), config),
            has_swap: self
                .mem
                .as_ref()
                .is_some_and(|m| m.info.stats.swap_total > 0),
            has_cpu_temp: self.cpu.as_ref().is_some_and(|c| !c.info.temp.is_empty()),
            has_cpu_watts: self
                .cpu
                .as_ref()
                .is_some_and(|c| c.info.cpu_watts.is_some()),
        }
    }
}

/// Count the disks that pass the user's `disks_filter`.
///
/// Used by both layout sizing (`calculate_layout` and `LayoutHints`) and
/// dirty-flag change detection so the disk box height tracks the
/// post-filter row count, not the raw drive count. Returns 0 when no
/// disk snapshot is available.
fn filtered_disk_count(disk: Option<&runner::DiskSnapshot>, config: &config::Config) -> usize {
    let filter = crate::domain::disk::DisksFilter::parse(&config.disks_filter);
    disk.map_or(0, |d| {
        d.info
            .disks
            .iter()
            .filter(|info| filter.matches(&info.name))
            .count()
    })
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

/// Tracks recently-dead processes for one additional display cycle.
///
/// When `keep_dead_proc_usage` is enabled, processes that disappear
/// between collection cycles are preserved with their last-known
/// CPU/memory values so they don't flicker out of the display.
struct StaleProcessTracker {
    /// Recently-dead process entries (PID → last known data).
    stale: HashMap<u32, crate::domain::process::ProcInfo>,
    /// PIDs from the previous collection cycle.
    prev_pids: HashSet<u32>,
    /// ProcInfo cache from the previous cycle for stale lookup.
    prev_cache: HashMap<u32, crate::domain::process::ProcInfo>,
}

impl StaleProcessTracker {
    fn new() -> Self {
        Self {
            stale: HashMap::new(),
            prev_pids: HashSet::new(),
            prev_cache: HashMap::new(),
        }
    }

    /// Update stale tracking with the current live process list.
    ///
    /// Detects PIDs that were in the previous cycle but are now gone,
    /// and preserves their last-known data for one display cycle.
    fn update(&mut self, live_procs: &[crate::domain::process::ProcInfo], keep_dead: bool) {
        let active_pids: HashSet<u32> = live_procs.iter().map(|p| p.pid).collect();

        self.stale.clear();
        if keep_dead {
            for &pid in &self.prev_pids {
                if !active_pids.contains(&pid)
                    && let Some(info) = self.prev_cache.get(&pid)
                {
                    self.stale.insert(pid, info.clone());
                }
            }
        }

        self.prev_pids = active_pids;
        self.prev_cache.clear();
        for proc in live_procs {
            self.prev_cache.insert(proc.pid, proc.clone());
        }
    }

    /// Returns the stale process entries from the previous cycle.
    fn stale_procs(&self) -> &HashMap<u32, crate::domain::process::ProcInfo> {
        &self.stale
    }
}

pub(crate) struct ProcessViewState {
    pub(crate) start: usize,
    pub(crate) selected: usize,
    pub(crate) detailed_pid: u32,
    pub(crate) selected_pid: u32,
    pub(crate) followed_pid: u32,
    pub(crate) filter_text: String,
    pub(crate) entries: Vec<ProcDisplayEntry>,
    /// Combined procs list (live + stale) that entries' `proc_index` refers to.
    /// Only populated when `keep_dead_proc_usage` adds stale entries.
    pub(crate) display_procs: Option<Vec<crate::domain::process::ProcInfo>>,
    /// Armed terminate state: (pid, process_name, force_kill).
    /// Set on first `t`/`T`, cleared on second press or any other key.
    pub(crate) armed_terminate: Option<(u32, String, bool)>,
    stale_tracker: StaleProcessTracker,
}

impl ProcessViewState {
    fn new() -> Self {
        Self {
            start: 0,
            selected: 0,
            detailed_pid: 0,
            selected_pid: 0,
            followed_pid: 0,
            filter_text: String::new(),
            entries: Vec::new(),
            display_procs: None,
            armed_terminate: None,
            stale_tracker: StaleProcessTracker::new(),
        }
    }

    fn update_stale_procs(&mut self, procs: &[crate::domain::process::ProcInfo], keep_dead: bool) {
        self.stale_tracker.update(procs, keep_dead);
    }

    fn rebuild_entries(
        &mut self,
        procs: Option<&[crate::domain::process::ProcInfo]>,
        config: &config::Config,
    ) {
        let Some(live_procs) = procs else {
            self.entries.clear();
            self.display_procs = None;
            return;
        };

        // Build combined procs list with stale entries if keep_dead_proc_usage.
        let stale = self.stale_tracker.stale_procs();
        let has_stale = config.keep_dead_proc_usage && !stale.is_empty();
        if has_stale {
            let combined: Vec<crate::domain::process::ProcInfo> = live_procs
                .iter()
                .cloned()
                .chain(stale.values().cloned())
                .collect();
            self.display_procs = Some(combined);
        } else {
            self.display_procs = None;
        }
        let procs: &[crate::domain::process::ProcInfo] =
            self.display_procs.as_deref().unwrap_or(live_procs);

        let sort_by = config.proc_sorting;
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
        self.selected_pid = self
            .entries
            .get(self.selected)
            .and_then(|e| procs.get(e.proc_index))
            .map_or(0, |p| p.pid);

        // Auto-scroll to followed process.
        let followed = self.followed_pid;
        if followed > 0 {
            if let Some(idx) = self
                .entries
                .iter()
                .position(|e| procs.get(e.proc_index).is_some_and(|p| p.pid == followed))
            {
                self.selected = idx;
                self.selected_pid = followed;
            } else {
                // Followed process died — unfollow.
                self.followed_pid = 0;
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
        nets: &[crate::domain::network::NetInfo],
        preferred: &str,
        dirty: &mut Dirty,
    ) {
        if nets.is_empty() {
            if !self.selected_iface.is_empty() {
                self.selected_iface.clear();
                *dirty |= Dirty::NET_BOX;
            }
            return;
        }

        // If we have no selection yet, try the preferred interface from config
        if self.selected_iface.is_empty()
            && preferred != "auto"
            && !preferred.is_empty()
            && nets.iter().any(|n| n.name == preferred)
        {
            self.selected_iface = preferred.to_string();
            *dirty |= Dirty::NET_BOX;
            return;
        }

        if self.selected_iface.is_empty() || !nets.iter().any(|n| n.name == self.selected_iface) {
            self.selected_iface = nets[0].name.clone();
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
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Input,
                        error = %e,
                        "crossterm::event::read failed",
                    );
                }
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

/// Tracks which per-subsystem ready events were received in this drain cycle.
struct SubsystemReady {
    cpu: bool,
    mem: bool,
    disk: bool,
    net: bool,
    gpu: bool,
    proc_data: bool,
}

fn pull_subsystem_data(
    state: &mut AppState,
    config: &mut config::Config,
    manager: &runner::CollectorManager,
    render_ui: bool,
    ready: &SubsystemReady,
) {
    // Always consume slot data into LiveData.
    if ready.cpu
        && let Some(snap) = manager.cpu_slot.latest()
    {
        state.live.core_count = snap.info.core_count;
        state.live.cpu = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::CPU_BOX;
        }
    }
    if ready.mem
        && let Some(snap) = manager.mem_slot.latest()
    {
        state.live.total_mem = snap
            .info
            .stats
            .used
            .saturating_add(snap.info.stats.available);
        state.live.mem = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::MEM_BOX;
        }
    }
    if ready.disk
        && let Some(snap) = manager.disk_slot.latest()
    {
        state.live.disk = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::DISK_BOX;
        }
    }
    if ready.net
        && let Some(snap) = manager.net_slot.latest()
    {
        state.live.net = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::NET_BOX;
        }
    }
    if ready.gpu
        && let Some(snap) = manager.gpu_slot.latest()
    {
        state.live.gpu = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::GPU_BOX;
        }
    }
    if ready.proc_data
        && let Some(snap) = manager.proc_slot.latest()
    {
        state
            .process
            .update_stale_procs(&snap.procs, config.keep_dead_proc_usage);
        state.live.proc_data = Some(snap);
        if render_ui {
            state.render.dirty |= Dirty::PROC_BOX | Dirty::PROC_LIST;
        }
    }

    // Check layout hints for changes.
    let new_hints = state.live.layout_hints(config);
    if state
        .render
        .last_layout_hints
        .is_none_or(|hints| hints != new_hints)
    {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }
    state.render.last_layout_hints = Some(new_hints);

    apply_startup_snapshot_config(state, config);
    reconcile_selected_iface(state, config);
}

fn apply_startup_snapshot_config(state: &mut AppState, config: &mut config::Config) {
    if state.startup.boxes_initialized {
        return;
    }
    let gpu_count = state.live.gpu.as_ref().map_or(0, |g| g.gpus.len());
    if gpu_count == 0 {
        return;
    }

    if auto_add_gpu_boxes(config, gpu_count) {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }
    state.startup.boxes_initialized = true;
}

fn auto_add_gpu_boxes(config: &mut config::Config, gpu_count: usize) -> bool {
    if gpu_count == 0 {
        return false;
    }

    let mut changed = false;
    for i in 0..gpu_count {
        let name = format!("gpu{i}");
        if !config.shown_boxes.iter().any(|b| b == &name) {
            config.shown_boxes.push(name);
            changed = true;
        }
    }
    changed
}

fn reconcile_selected_iface(state: &mut AppState, config: &config::Config) {
    let Some(net) = state.live.net.as_ref() else {
        return;
    };
    state
        .network
        .reconcile(&net.nets, &config.net_iface, &mut state.render.dirty);
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
        "{}\x1b[{msg_y};{msg_x}H{}{}{msg}{}",
        term::CLEAR_SCREEN,
        term::BOLD,
        theme.color(tc::HI_FG),
        term::RESET,
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
        if let Err(e) = terminal.write_synced(&output) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
        state.render.clear_dirty();
    }
}

fn render_waiting_for_snapshot(size: TerminalSize, theme: &theme::Theme) -> String {
    let msg = "Collecting data...";
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!(
        "{}\x1b[{msg_y};{msg_x}H{}{}{msg}{}",
        term::CLEAR_SCREEN,
        term::BOLD,
        theme.color(tc::HI_FG),
        term::RESET,
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
        if let Err(e) = terminal.write_synced(&output) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
        state.render.clear_dirty();
    }
}

fn execute_dirty_work(state: &mut AppState, config: &mut config::Config, size: TerminalSize) {
    if state.render.dirty.contains(Dirty::PROC_LIST) {
        rebuild_proc_list(state, config);
    }

    if state.render.dirty.contains(Dirty::LAYOUT) || state.render.cached_layout.is_none() {
        state.render.cached_layout = Some(calculate_layout(config, &state.live, size));
    }
}

fn rebuild_proc_list(state: &mut AppState, config: &config::Config) {
    let procs = state.live.proc_data.as_ref().map(|s| s.procs.as_slice());
    state.process.rebuild_entries(procs, config);
}

fn calculate_layout(
    config: &config::Config,
    live: &LiveData,
    size: TerminalSize,
) -> draw::layout::Layout {
    let cpu = live.cpu.as_ref();
    let has_temp = config.check_temp && cpu.is_some_and(|c| !c.info.temp.is_empty());
    let has_watts = config.show_cpu_watts && cpu.is_some_and(|c| c.info.cpu_watts.is_some());
    let stats_rows = ui::cpu_box::stats_row_count(has_temp, has_watts);
    let cpu_panel_overhead = stats_rows + 2; // stats + load detail row + section divider

    draw::layout::calc_sizes(&draw::layout::LayoutConfig {
        term_width: size.width,
        term_height: size.height,
        shown_boxes: &config.shown_boxes,
        cpu_bottom: config.cpu_bottom,
        mem_below_net: config.mem_below_net,
        proc_left: config.proc_left,
        core_count: live.core_count,
        gpu_count: live.gpu.as_ref().map_or(0, |g| g.gpus.len()),
        disk_count: filtered_disk_count(live.disk.as_deref(), config),
        has_swap: config.show_swap
            && live
                .mem
                .as_ref()
                .is_some_and(|m| m.info.stats.swap_total > 0),
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

    let params = RenderParams {
        dirty: state.render.dirty,
        layout,
        cpu: state.live.cpu.as_deref(),
        mem: state.live.mem.as_deref(),
        disk: state.live.disk.as_deref(),
        net: state.live.net.as_deref(),
        gpu: state.live.gpu.as_deref(),
        proc_data: state.live.proc_data.as_deref(),
        proc_entries: &state.process.entries,
        proc_display_procs: state.process.display_procs.as_deref(),
        selected_iface: &state.network.selected_iface,
        config,
        theme,
        rounded: state.runtime.rounded,
        update_ms: state.runtime.update_ms,
        is_filtering: state.overlay.menu_state == MenuState::Filter,
        core_count: state.live.core_count,
        total_mem: state.live.total_mem,
        detailed_pid: state.process.detailed_pid,
        followed_pid: state.process.followed_pid,
        armed_terminate: state
            .process
            .armed_terminate
            .as_ref()
            .map(|(_, name, force)| (name.as_str(), *force)),
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
    manager: &runner::CollectorManager,
    size: TerminalSize,
) -> AppCommand {
    let mut ctx = InputContext {
        config,
        theme,
        manager,
        live: &state.live,
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
        if let Err(e) = terminal.write_synced(&out) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
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
                if let Err(e) = terminal.write_raw(&styled) {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Terminal,
                        error = %e,
                        "terminal write failed",
                    );
                }
            }
            handlers::TerminalOp::Synced(_) => {
                if let Err(e) = terminal.write_synced(&styled) {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Terminal,
                        error = %e,
                        "terminal write failed",
                    );
                }
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
        match config.write(&conf_path) {
            Ok(()) => tracing::info!(
                subsystem = %crate::log::Subsystem::Config,
                path = %conf_path.display(),
                "config saved",
            ),
            Err(e) => tracing::warn!(
                subsystem = %crate::log::Subsystem::Config,
                error = %e,
                path = %conf_path.display(),
                "config save failed",
            ),
        }
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
    pub(crate) core_count: usize,
    pub(crate) total_mem: u64,
    pub(crate) detailed_pid: u32,
    pub(crate) followed_pid: u32,
    pub(crate) armed_terminate: Option<(&'a str, bool)>,
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
    let config = params.config;
    let theme = params.theme;
    let rounded = params.rounded;
    let update_ms = params.update_ms;
    let is_filtering = params.is_filtering;
    let mut output = String::new();

    if dirty.intersects(Dirty::CPU_BOX)
        && let Some(ref cpu_dim) = layout.cpu
        && let Some(cpu) = params.cpu
    {
        let area = ui::BoxArea::from_dim(cpu_dim, rounded);
        let cpu_settings = ui::cpu_box::CpuBoxSettings {
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                config.graph_symbol_cpu,
                config.graph_symbol,
            ),
            upper_source: config.cpu_graph_upper,
            lower_source: config.cpu_graph_lower,
            check_temp: config.check_temp,
            show_coretemp: config.show_coretemp,
            temp_scale: config.temp_scale,
            single_graph: config.cpu_single_graph,
            update_ms,
            current_preset: config.current_preset,
            invert_lower: config.cpu_invert_lower,
            show_cpu_freq: config.show_cpu_freq,
            show_uptime: config.show_uptime,
            cpu_name: &cpu.info.cpu_name,
            custom_cpu_name: &config.custom_cpu_name,
            show_cpu_watts: config.show_cpu_watts,
            cpu_watts: cpu.info.cpu_watts,
            cpu_max_watts: cpu.info.cpu_max_watts,
            clock_format: &config.clock_format,
        };
        output.push_str(&ui::cpu_box::draw(
            &cpu.info,
            &area,
            theme,
            &cpu_settings,
            &cpu.status,
        ));
    }

    if dirty.intersects(Dirty::GPU_BOX)
        && let Some(gpu) = params.gpu
    {
        for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
            if gi < gpu.gpus.len() {
                let area = ui::BoxArea::from_dim(gpu_dim, rounded);
                let custom_name = config
                    .custom_gpu_names
                    .get(gi)
                    .map(String::as_str)
                    .unwrap_or("");
                let gpu_settings = ui::gpu_box::GpuBoxSettings {
                    index: gi,
                    temp_scale: config.temp_scale,
                    custom_name,
                    base_10: config.base_10_sizes,
                };
                output.push_str(&ui::gpu_box::draw(
                    &gpu.gpus[gi],
                    &area,
                    theme,
                    &gpu_settings,
                    &gpu.status,
                ));
            }
        }
    }

    if dirty.intersects(Dirty::MEM_BOX)
        && let Some(ref mem_dim) = layout.mem
        && let Some(mem) = params.mem
    {
        let area = ui::BoxArea::from_dim(mem_dim, rounded);
        output.push_str(&ui::mem_box::draw(
            &mem.info,
            &area,
            theme,
            &ui::mem_box::MemBoxSettings {
                show_swap: config.show_swap,
                base_10: config.base_10_sizes,
            },
            &mem.status,
        ));
    }

    if dirty.intersects(Dirty::DISK_BOX)
        && let Some(ref disk_dim) = layout.disk
        && let Some(disk) = params.disk
    {
        let area = ui::BoxArea::from_dim(disk_dim, rounded);
        let disk_settings = ui::disk_box::DiskBoxSettings {
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                config.graph_symbol_disk,
                config.graph_symbol,
            ),
            base_10: config.base_10_sizes,
            show_io_stat: config.show_io_stat,
            io_mode: config.io_mode,
            disk_io_mode: config.disk_io_mode,
            io_graph_combined: config.io_graph_combined,
        };
        let filter = crate::domain::disk::DisksFilter::parse(&config.disks_filter);
        let visible = filter.apply(&disk.info.disks);
        output.push_str(&ui::disk_box::draw(
            &visible,
            &area,
            theme,
            &disk_settings,
            &disk.status,
        ));
    }

    if dirty.intersects(Dirty::NET_BOX)
        && let Some(ref net_dim) = layout.net
        && let Some(net) = params.net
    {
        let iface = params.selected_iface;
        let default_net = crate::domain::network::NetInfo::default();
        let net_info = net
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
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                config.graph_symbol_net,
                config.graph_symbol,
            ),
            swap_dl_ul: config.swap_upload_download,
            base_10: config.base_10_sizes,
        };
        output.push_str(&ui::net_box::draw(
            net_info,
            &area,
            theme,
            &net_settings,
            &net.status,
        ));
    }

    if dirty.intersects(Dirty::PROC_BOX)
        && let Some(ref proc_dim) = layout.proc_box
        && let Some(proc_snap) = params.proc_data
    {
        let procs = params.proc_display_procs.unwrap_or(&proc_snap.procs);
        let entries = params.proc_entries;
        let detailed_pid = params.detailed_pid;
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
        let sort_by = config.proc_sorting;
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
        let proc_settings = ui::proc_box::ProcBoxSettings {
            proc_per_core: config.proc_per_core,
            core_count: params.core_count,
            proc_mem_bytes: config.proc_mem_bytes,
            total_mem: params.total_mem,
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
            &proc_snap.status,
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
        assert!(!state.live.is_ready());
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
        config.shown_boxes = vec!["cpu".into(), "mem".into(), "proc".into()];

        assert!(auto_add_gpu_boxes(&mut config, 2));
        assert_eq!(
            config.shown_boxes,
            vec!["cpu", "mem", "proc", "gpu0", "gpu1"]
        );
    }

    #[test]
    fn auto_add_gpu_boxes_ignores_existing_boxes() {
        let mut config = config::Config::new();
        config.shown_boxes = vec!["cpu".into(), "mem".into(), "proc".into(), "gpu0".into()];

        assert!(!auto_add_gpu_boxes(&mut config, 1));
        assert_eq!(config.shown_boxes, vec!["cpu", "mem", "proc", "gpu0"]);
    }

    #[test]
    fn reconcile_selected_iface_selects_first_available_interface() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.render.clear_dirty();
        state.live.net = Some(Arc::new(runner::NetSnapshot {
            nets: vec![
                crate::domain::network::NetInfo {
                    name: "Ethernet".into(),
                    ..Default::default()
                },
                crate::domain::network::NetInfo {
                    name: "Wi-Fi".into(),
                    ..Default::default()
                },
            ],
            status: crate::collect::CollectStatus::Ok,
        }));

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

    fn make_proc(pid: u32) -> crate::domain::process::ProcInfo {
        crate::domain::process::ProcInfo {
            pid,
            name: format!("proc{pid}"),
            ..Default::default()
        }
    }

    #[test]
    fn stale_tracker_no_stale_on_first_update() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_detects_dead_process() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], true);
        assert_eq!(tracker.stale_procs().len(), 1);
        assert!(tracker.stale_procs().contains_key(&2));
    }

    #[test]
    fn stale_tracker_one_cycle_retention() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], true);
        assert!(tracker.stale_procs().contains_key(&2));
        // Third cycle: pid 2 is no longer in prev_pids, so it drops out.
        tracker.update(&[make_proc(1)], true);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_keep_dead_false_clears_stale() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], false);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_all_processes_die() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[], true);
        assert_eq!(tracker.stale_procs().len(), 2);
    }
}
