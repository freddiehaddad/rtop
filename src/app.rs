use crate::{
    config,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    draw, handlers,
    handlers::{InputContext, MenuState},
    input, runner, term, theme, tools, ui,
};
use std::time::{Duration, Instant};

/// Run the main event loop: collect data, render UI, and handle input.
pub fn run(
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    runner: &mut runner::Runner,
) {
    let mut state = AppState::new(config, Instant::now());

    loop {
        let size = refresh_terminal(&mut state, terminal);

        if is_too_small(size) {
            if handle_small_terminal(&mut state, terminal, size) == AppCommand::Quit {
                break;
            }
            continue;
        }

        state.mark_update_due(Instant::now());

        if state.render_ui() && !state.dirty.is_empty() {
            execute_dirty_work(&mut state, config, runner, size);
            write_dirty_frame(&mut state, config, terminal, theme, runner);
        }

        if poll_and_handle_input(&mut state, config, terminal, theme, runner, size)
            == AppCommand::Quit
        {
            break;
        }
    }

    save_config_on_exit(config);
}

struct AppState {
    rounded: bool,
    update_ms: u64,
    menu_state: MenuState,
    options_cat: usize,
    options_selected: usize,
    options_page: usize,
    main_menu_selected: usize,
    proc_start: usize,
    proc_selected: usize,
    filter_text: String,
    menu_return_to: MenuState,
    dirty: Dirty,
    cached_layout: Option<draw::layout::Layout>,
    next_update: Instant,
}

impl AppState {
    fn new(config: &config::Config, now: Instant) -> Self {
        Self {
            rounded: config.get_bool(bk::ROUNDED_CORNERS),
            update_ms: config.get_int(ik::UPDATE_MS) as u64,
            menu_state: MenuState::None,
            options_cat: 0,
            options_selected: 0,
            options_page: 0,
            main_menu_selected: 0,
            proc_start: 0,
            proc_selected: 0,
            filter_text: String::new(),
            menu_return_to: MenuState::None,
            dirty: Dirty::FULL,
            cached_layout: None,
            next_update: now,
        }
    }

    fn render_ui(&self) -> bool {
        self.menu_state == MenuState::None || self.menu_state == MenuState::Filter
    }

    fn poll_timeout_ms(&self, now: Instant) -> u64 {
        let remaining = self.next_update.saturating_duration_since(now).as_millis() as u64;
        remaining.clamp(10, 1000)
    }

    fn mark_resize(&mut self) {
        self.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
    }

    fn mark_update_due(&mut self, now: Instant) {
        if now >= self.next_update {
            self.dirty |= Dirty::COLLECT | Dirty::ALL_BOXES | Dirty::PROC_LIST;
            self.next_update = now + Duration::from_millis(self.update_ms);
        }
    }

    fn clear_dirty(&mut self) {
        self.dirty = Dirty::empty();
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
        state.mark_resize();
    }
    let (width, height) = terminal.size();
    TerminalSize {
        width: width as usize,
        height: height as usize,
    }
}

fn is_too_small(size: TerminalSize) -> bool {
    size.width < draw::layout::MIN_TERM_WIDTH || size.height < draw::layout::MIN_TERM_HEIGHT
}

fn render_too_small(size: TerminalSize) -> String {
    let min_w = draw::layout::MIN_TERM_WIDTH;
    let min_h = draw::layout::MIN_TERM_HEIGHT;
    let msg = format!(
        "Terminal too small ({}x{}). Need {}x{}.",
        size.width, size.height, min_w, min_h
    );
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!("\x1b[2J\x1b[{msg_y};{msg_x}H\x1b[1;33m{msg}\x1b[0m")
}

fn handle_small_terminal(
    state: &mut AppState,
    terminal: &mut term::Terminal,
    size: TerminalSize,
) -> AppCommand {
    if state.dirty.contains(Dirty::LAYOUT) || state.dirty.intersects(Dirty::ALL_BOXES) {
        let _ = terminal.write_synced(&render_too_small(size));
        state.clear_dirty();
    }

    if input::poll(state.poll_timeout_ms(Instant::now()))
        && input::get().is_some_and(|key| key == "q")
    {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn execute_dirty_work(
    state: &mut AppState,
    config: &config::Config,
    runner: &mut runner::Runner,
    size: TerminalSize,
) {
    if state.dirty.contains(Dirty::COLLECT) {
        runner.collect_all();
    }

    if state.dirty.contains(Dirty::PROC_LIST) {
        rebuild_proc_list(config, runner);
    }

    if state.dirty.contains(Dirty::LAYOUT) || state.cached_layout.is_none() {
        state.cached_layout = Some(calculate_layout(config, runner, size));
    }
}

fn rebuild_proc_list(config: &config::Config, runner: &mut runner::Runner) {
    let sort_by = config.get_string(sk::PROC_SORTING);
    let reversed = config.get_bool(bk::PROC_REVERSED);
    let filter = config.get_string(sk::PROC_FILTER);
    let tree_mode = config.get_bool(bk::PROC_TREE);
    runner
        .proc_collector
        .rebuild_display(sort_by, reversed, filter, tree_mode);
}

fn calculate_layout(
    config: &config::Config,
    runner: &runner::Runner,
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
        core_count: runner.cpu.info.core_count,
        gpu_count: runner.gpu.gpu_count(),
        disk_count: runner.disk.data.disks.len(),
        has_swap: runner.mem.info.stats.swap_total > 0,
    })
}

fn write_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    runner: &runner::Runner,
) {
    let output = render_dirty_frame(state, config, runner, theme);
    if let Err(e) = terminal.write_synced(&output) {
        tracing::debug!("terminal write failed: {e}");
    }
    state.clear_dirty();
}

fn render_dirty_frame(
    state: &mut AppState,
    config: &config::Config,
    runner: &runner::Runner,
    theme: &theme::Theme,
) -> String {
    let layout = state
        .cached_layout
        .as_ref()
        .expect("layout must be initialized before rendering");
    let mut output = String::new();

    if state.dirty.contains(Dirty::LAYOUT) {
        output.push_str("\x1b[2J");
    }

    let params = RenderParams {
        dirty: state.dirty,
        layout,
        runner,
        config,
        theme,
        rounded: state.rounded,
        update_ms: state.update_ms,
        is_filtering: state.menu_state == MenuState::Filter,
    };
    output.push_str(&render_all(
        &params,
        &mut state.proc_selected,
        &mut state.proc_start,
    ));
    output
}

fn poll_and_handle_input(
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    runner: &mut runner::Runner,
    size: TerminalSize,
) -> AppCommand {
    if !input::poll(state.poll_timeout_ms(Instant::now())) {
        return AppCommand::Continue;
    }

    let Some(key) = input::get() else {
        return AppCommand::Continue;
    };

    handle_input_key(key.as_ref(), state, config, terminal, theme, runner, size)
}

fn handle_input_key(
    key: &str,
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    runner: &mut runner::Runner,
    size: TerminalSize,
) -> AppCommand {
    if key.is_empty() || key.starts_with("mouse_") || key == "resize" {
        if key == "resize" {
            state.mark_resize();
        }
        return AppCommand::Continue;
    }

    let mut ctx = InputContext {
        config,
        theme,
        runner,
        menu_state: &mut state.menu_state,
        dirty: &mut state.dirty,
        rounded: &mut state.rounded,
        update_ms: &mut state.update_ms,
        main_menu_selected: &mut state.main_menu_selected,
        options_cat: &mut state.options_cat,
        options_selected: &mut state.options_selected,
        options_page: &mut state.options_page,
        proc_selected: &mut state.proc_selected,
        proc_start: &mut state.proc_start,
        filter_text: &mut state.filter_text,
        cached_layout: &state.cached_layout,
        menu_return_to: &mut state.menu_return_to,
        tw: size.width,
        th: size.height,
    };
    let result = dispatch_handler(key, &mut ctx);
    execute_terminal_ops(terminal, &result);
    if result.redraw_overlay {
        let out = handlers::redraw_after_overlay(&mut ctx);
        let _ = terminal.write_synced(&out);
    }

    if result.quit {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn dispatch_handler(key: &str, ctx: &mut InputContext) -> handlers::HandleResult {
    match *ctx.menu_state {
        MenuState::Main => handlers::main_menu::handle(key, ctx),
        MenuState::Help => handlers::help::handle(key, ctx),
        MenuState::Options => handlers::options::handle(key, ctx),
        MenuState::Filter => handlers::filter::handle(key, ctx),
        MenuState::None => handlers::normal::handle(key, ctx),
    }
}

fn execute_terminal_ops(terminal: &mut term::Terminal, result: &handlers::HandleResult) {
    for op in &result.ops {
        match op {
            handlers::TerminalOp::Raw(s) => {
                let _ = terminal.write_raw(s);
            }
            handlers::TerminalOp::Synced(s) => {
                let _ = terminal.write_synced(s);
            }
        }
    }
}

fn save_config_on_exit(config: &config::Config) {
    if config.get_bool(bk::SAVE_CONFIG_ON_EXIT) {
        let conf_path = tools::config_dir().join("rtop.conf");
        let _ = config.write(&conf_path);
    }
}

fn clamp_proc_selection(
    procs: &[crate::domain::process::ProcInfo],
    box_height: usize,
    detail_rows: usize,
    selected: &mut usize,
    start: &mut usize,
) {
    let count = procs.len();
    let max_visible = box_height.saturating_sub(5 + detail_rows);
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
    pub(crate) runner: &'a runner::Runner,
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
    let runner = params.runner;
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
                &runner.cpu.info,
                &area,
                theme,
                &cpu_settings,
                &runner.cpu.status,
            ));
        }
    }

    if dirty.intersects(Dirty::GPU_BOX) {
        let gpu_settings = ui::gpu_box::GpuBoxSettings {
            temp_scale: config.get_string(sk::TEMP_SCALE),
        };
        for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
            if gi < runner.gpu.gpus.len() {
                let area = ui::BoxArea::from_dim(gpu_dim, rounded);
                output.push_str(&ui::gpu_box::draw(
                    &runner.gpu.gpus[gi],
                    gi,
                    &area,
                    theme,
                    &gpu_settings,
                    &runner.gpu.status,
                ));
            }
        }
    }

    if dirty.intersects(Dirty::MEM_BOX) {
        if let Some(ref mem_dim) = layout.mem {
            let area = ui::BoxArea::from_dim(mem_dim, rounded);
            output.push_str(&ui::mem_box::draw(
                &runner.mem.info,
                &area,
                theme,
                config.get_bool(bk::SHOW_SWAP),
                &runner.mem.status,
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
                &runner.disk.data,
                &area,
                theme,
                &disk_settings,
                &runner.disk.status,
            ));
        }
    }

    if dirty.intersects(Dirty::NET_BOX) {
        if let Some(ref net_dim) = layout.net {
            let iface = &runner.net.selected_iface;
            let default_net = crate::domain::network::NetInfo::default();
            let net_info = runner.net.current_net.get(iface).unwrap_or(&default_net);
            let area = ui::BoxArea::from_dim(net_dim, rounded);
            let net_settings = ui::net_box::NetBoxSettings {
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
                iface,
                &area,
                theme,
                &net_settings,
                &runner.net.status,
            ));
        }
    }

    if dirty.intersects(Dirty::PROC_BOX) {
        if let Some(ref proc_dim) = layout.proc_box {
            let procs = &runner.proc_collector.display_procs;
            let detailed_pid = config.get_int(ik::DETAILED_PID) as u32;
            let detail_rows = if detailed_pid > 0 {
                8_usize.min(proc_dim.height.saturating_sub(6))
            } else {
                0
            };
            clamp_proc_selection(
                procs,
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
            output.push_str(&ui::proc_box::draw_with_sort(
                procs,
                &area,
                &view,
                theme,
                &runner.proc_collector.status,
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

        assert!(!state.rounded);
        assert_eq!(state.update_ms, 1_500);
        assert!(state.menu_state == MenuState::None);
        assert_eq!(state.dirty, Dirty::FULL);
        assert!(state.cached_layout.is_none());
        assert_eq!(state.next_update, now);
    }

    #[test]
    fn app_state_render_ui_only_for_normal_and_filter() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        state.menu_state = MenuState::None;
        assert!(state.render_ui());

        state.menu_state = MenuState::Filter;
        assert!(state.render_ui());

        state.menu_state = MenuState::Main;
        assert!(!state.render_ui());

        state.menu_state = MenuState::Help;
        assert!(!state.render_ui());

        state.menu_state = MenuState::Options;
        assert!(!state.render_ui());
    }

    #[test]
    fn poll_timeout_is_clamped() {
        let config = config::Config::new();
        let base = Instant::now();
        let mut state = AppState::new(&config, base);

        state.next_update = base + Duration::from_millis(5_000);
        assert_eq!(state.poll_timeout_ms(base), 1_000);

        state.next_update = base + Duration::from_millis(500);
        assert_eq!(state.poll_timeout_ms(base), 500);

        state.next_update = base + Duration::from_millis(5);
        assert_eq!(state.poll_timeout_ms(base), 10);

        state.next_update = base;
        assert_eq!(state.poll_timeout_ms(base + Duration::from_millis(1)), 10);
    }

    #[test]
    fn mark_resize_sets_layout_and_all_boxes() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.clear_dirty();

        state.mark_resize();

        assert!(state.dirty.contains(Dirty::LAYOUT));
        assert!(state.dirty.contains(Dirty::ALL_BOXES));
    }

    #[test]
    fn mark_update_due_sets_collection_flags_and_advances_deadline() {
        let mut config = config::Config::new();
        config.set_int(ik::UPDATE_MS, 750);
        let base = Instant::now();
        let mut state = AppState::new(&config, base);
        state.clear_dirty();
        let due = base + Duration::from_millis(1);

        state.mark_update_due(due);

        assert!(state.dirty.contains(Dirty::COLLECT));
        assert!(state.dirty.contains(Dirty::ALL_BOXES));
        assert!(state.dirty.contains(Dirty::PROC_LIST));
        assert_eq!(state.next_update, due + Duration::from_millis(750));
    }

    #[test]
    fn mark_update_due_ignores_future_deadline() {
        let config = config::Config::new();
        let base = Instant::now();
        let mut state = AppState::new(&config, base);
        state.clear_dirty();
        state.next_update = base + Duration::from_millis(500);

        state.mark_update_due(base);

        assert!(state.dirty.is_empty());
        assert_eq!(state.next_update, base + Duration::from_millis(500));
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
        let out = render_too_small(TerminalSize {
            width: 40,
            height: 10,
        });

        assert!(out.contains("Terminal too small (40x10)."));
        assert!(out.contains(&format!(
            "Need {}x{}.",
            draw::layout::MIN_TERM_WIDTH,
            draw::layout::MIN_TERM_HEIGHT
        )));
    }
}
