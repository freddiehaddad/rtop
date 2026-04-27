use crate::{
    config,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    draw, handlers,
    handlers::{InputContext, MenuState},
    input, runner, term, theme, tools, ui,
};

/// Run the main event loop: collect data, render UI, and handle input.
pub fn run(
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    runner: &mut runner::Runner,
) {
    let mut rounded = config.get_bool(bk::ROUNDED_CORNERS);
    let mut update_ms = config.get_int(ik::UPDATE_MS) as u64;

    let mut menu_state = MenuState::None;

    let mut options_cat: usize = 0;
    let mut options_selected: usize = 0;
    let mut options_page: usize = 0;
    let mut main_menu_selected: usize = 0;
    let mut proc_start: usize = 0;
    let mut proc_selected: usize = 0;
    let mut filter_text = String::new();
    let mut menu_return_to = MenuState::None;

    // Main event loop — timer-based collection with per-box dirty tracking.
    let mut dirty = Dirty::FULL;
    let mut cached_layout: Option<draw::layout::Layout> = None;
    let mut next_update = std::time::Instant::now();

    loop {
        // ── Phase 1: Detect what's dirty ──────────────────────────────────

        // Terminal resize
        if terminal.refresh() {
            dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        let (tw, th) = terminal.size();
        let tw = tw as usize;
        let th = th as usize;

        // If terminal is too small, show a message and skip rendering
        let min_w = draw::layout::MIN_TERM_WIDTH;
        let min_h = draw::layout::MIN_TERM_HEIGHT;
        if tw < min_w || th < min_h {
            if dirty.contains(Dirty::LAYOUT) || dirty.intersects(Dirty::ALL_BOXES) {
                let msg = format!("Terminal too small ({tw}x{th}). Need {min_w}x{min_h}.");
                let msg_y = th.max(1) / 2;
                let msg_x = tw.saturating_sub(msg.len()) / 2 + 1;
                let out = format!("\x1b[2J\x1b[{msg_y};{msg_x}H\x1b[1;33m{msg}\x1b[0m",);
                let _ = terminal.write_synced(&out);
                dirty = Dirty::empty();
            }

            let remaining = next_update
                .saturating_duration_since(std::time::Instant::now())
                .as_millis() as u64;
            let poll_ms = remaining.clamp(10, 1000);
            if input::poll(poll_ms) {
                if let Some(key) = input::get() {
                    if key == "q" {
                        break;
                    }
                }
            }
            continue;
        }

        // Wall-clock collection deadline
        let now = std::time::Instant::now();
        if now >= next_update {
            dirty |= Dirty::COLLECT | Dirty::ALL_BOXES | Dirty::PROC_LIST;
            next_update = now + std::time::Duration::from_millis(update_ms);
        }

        // ── Phase 2: Execute dirty work (skip if menu overlay is active) ──

        let render_ui = menu_state == MenuState::None || menu_state == MenuState::Filter;

        if render_ui && !dirty.is_empty() {
            // Collect data from OS
            if dirty.contains(Dirty::COLLECT) {
                runner.collect_all();
            }

            // Rebuild derived process display list
            if dirty.contains(Dirty::PROC_LIST) {
                let sort_by = config.get_string(sk::PROC_SORTING);
                let reversed = config.get_bool(bk::PROC_REVERSED);
                let filter = config.get_string(sk::PROC_FILTER);
                let tree_mode = config.get_bool(bk::PROC_TREE);
                runner
                    .proc_collector
                    .rebuild_display(sort_by, reversed, filter, tree_mode);
            }

            // Calculate layout (or reuse cached)
            if dirty.contains(Dirty::LAYOUT) || cached_layout.is_none() {
                let shown: Vec<String> = config
                    .get_string(sk::SHOWN_BOXES)
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                cached_layout = Some(draw::layout::calc_sizes(&draw::layout::LayoutConfig {
                    term_width: tw,
                    term_height: th,
                    shown_boxes: &shown,
                    cpu_bottom: config.get_bool(bk::CPU_BOTTOM),
                    mem_below_net: config.get_bool(bk::MEM_BELOW_NET),
                    proc_left: config.get_bool(bk::PROC_LEFT),
                    core_count: runner.cpu.info.core_count,
                    gpu_count: runner.gpu.gpu_count(),
                    disk_count: runner.disk.data.disks.len(),
                    has_swap: runner.mem.info.stats.swap_total > 0,
                }));
            }
            let layout = cached_layout
                .as_ref()
                .expect("layout must be initialized before rendering");

            // ── Phase 3: Render dirty boxes ───────────────────────────────

            let mut output = String::new();

            // Full screen clear only when layout changed
            if dirty.contains(Dirty::LAYOUT) {
                output.push_str("\x1b[2J");
            }

            let is_filtering = menu_state == MenuState::Filter;
            let params = RenderParams {
                dirty,
                layout,
                runner,
                config,
                theme,
                rounded,
                update_ms,
                is_filtering,
            };
            output.push_str(&render_all(&params, &mut proc_selected, &mut proc_start));

            if let Err(e) = terminal.write_synced(&output) {
                tracing::debug!("terminal write failed: {e}");
            }

            dirty = Dirty::empty();
        }

        // Poll for input — wait at most until the next update deadline
        let remaining = next_update
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let poll_ms = remaining.clamp(10, 1000); // At least 10ms, at most 1s

        if input::poll(poll_ms) {
            if let Some(key) = input::get() {
                if key.is_empty() || key.starts_with("mouse_") || key == "resize" {
                    if key == "resize" {
                        dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
                    }
                    continue;
                }
                let mut ctx = InputContext {
                    config: &mut *config,
                    terminal: &mut *terminal,
                    theme: &mut *theme,
                    runner: &mut *runner,
                    menu_state: &mut menu_state,
                    dirty: &mut dirty,
                    rounded: &mut rounded,
                    update_ms: &mut update_ms,
                    main_menu_selected: &mut main_menu_selected,
                    options_cat: &mut options_cat,
                    options_selected: &mut options_selected,
                    options_page: &mut options_page,
                    proc_selected: &mut proc_selected,
                    proc_start: &mut proc_start,
                    filter_text: &mut filter_text,
                    cached_layout: &cached_layout,
                    menu_return_to: &mut menu_return_to,
                    tw,
                    th,
                };
                let quit = match *ctx.menu_state {
                    MenuState::Main => handlers::main_menu::handle(&key, &mut ctx),
                    MenuState::Help => handlers::help::handle(&key, &mut ctx),
                    MenuState::Options => handlers::options::handle(&key, &mut ctx),
                    MenuState::Filter => handlers::filter::handle(&key, &mut ctx),
                    MenuState::None => handlers::normal::handle(&key, &mut ctx),
                };
                if quit {
                    break;
                }
            }
        }
        // No else branch needed — the wall-clock check at the top of the loop
        // handles periodic updates regardless of input activity.
    }

    // Save config on exit
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
            output.push_str(&ui::disk_box::draw(
                &runner.disk.data,
                &area,
                theme,
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
