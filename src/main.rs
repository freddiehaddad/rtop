mod banner;
mod cli;
mod collect;
mod config;
mod domain;
mod draw;
mod input;
mod log;
mod menu;
mod runner;
mod term;
mod theme;
mod tools;
mod ui;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    // --default-config: print and exit
    if cli.default_config {
        print!("{}", config::Config::new().to_config_string());
        return;
    }

    // Init logging
    let log_dir = tools::data_dir();
    log::init(&log_dir, "WARNING");

    // Load config (from CLI path or default location)
    let mut config = config::Config::new();
    let default_conf_path = tools::config_dir().join("rtop.conf");
    if let Some(ref path) = cli.config_file {
        let warnings = config.load(path);
        for w in &warnings {
            tracing::warn!("{}", w);
        }
    } else if default_conf_path.exists() {
        let warnings = config.load(&default_conf_path);
        for w in &warnings {
            tracing::warn!("{}", w);
        }
    }

    // Apply CLI overrides
    if cli.low_color {
        config.set_bool("lowcolor", true);
    }
    if cli.tty {
        config.set_bool("force_tty", true);
    }
    if let Some(ms) = cli.update_ms {
        config.set_int("update_ms", ms as i64);
    }
    if let Some(ref f) = cli.filter {
        config.set_string("proc_filter", f);
    }

    // Init terminal
    let mut terminal = match term::Terminal::init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize terminal: {e}");
            return;
        }
    };

    // Init theme
    let mut theme = theme::Theme::new();

    // Set terminal base colors from theme
    let base_colors = format!(
        "{}{}",
        theme.c("main_fg"),
        theme.c("main_bg").replace("38;2", "48;2")  // fg escape → bg escape
    );
    let _ = terminal.write_raw(&base_colors);

    // Init runner (collectors)
    let mut runner = runner::Runner::new();

    // Auto-add detected GPU boxes to shown_boxes if not already present
    if runner.gpu.gpu_count() > 0 {
        let shown = config.get_string("shown_boxes").to_string();
        let mut boxes: Vec<String> = shown.split_whitespace().map(|s| s.to_string()).collect();
        for i in 0..runner.gpu.gpu_count() {
            let name = format!("gpu{i}");
            if !boxes.iter().any(|b| b == &name) {
                boxes.push(name);
            }
        }
        config.set_string("shown_boxes", &boxes.join(" "));
    }

    let mut rounded = config.get_bool("rounded_corners");
    let mut update_ms = config.get_int("update_ms") as u64;

    #[derive(PartialEq)]
    enum MenuState {
        None,
        Main,
        Help,
        Options,
        Filter,
    }

    let mut menu_state = MenuState::None;

    let mut options_cat: usize = 0;
    let mut options_selected: usize = 0;
    let mut options_page: usize = 0;
    let mut main_menu_selected: usize = 0;
    let mut proc_start: usize = 0;
    let mut proc_selected: usize = 0;
    let mut filter_text = String::new();

    // Main event loop — timer-based like btop.
    // Collection runs on a wall-clock deadline, input never blocks it.
    let mut needs_full_redraw = true;
    let mut needs_proc_redraw = false;
    let mut cached_layout: Option<draw::layout::Layout> = None;

    let mut next_update = std::time::Instant::now();

    loop {
        // Check resize
        if terminal.refresh() {
            needs_full_redraw = true;
        }
        let (tw, th) = terminal.size();
        let tw = tw as usize;
        let th = th as usize;

        // Check if it's time for a full collection cycle (wall-clock deadline)
        let now = std::time::Instant::now();
        if now >= next_update {
            needs_full_redraw = true;
            next_update = now + std::time::Duration::from_millis(update_ms);
        }

        // Full redraw: collect data + render all boxes
        if (menu_state == MenuState::None || menu_state == MenuState::Filter) && needs_full_redraw {
            needs_full_redraw = false;
            needs_proc_redraw = false;

            // Collect data
            runner.collect_all();

            // Sort processes by configured column
            let sort_by = config.get_string("proc_sorting").to_string();
            let reversed = config.get_bool("proc_reversed");
            collect::process::sort_procs(&mut runner.proc_collector.procs, &sort_by, reversed);

            // Filter processes if filter is active
            let proc_filter = config.get_string("proc_filter").to_string();
            if !proc_filter.is_empty() {
                runner.proc_collector.procs.retain(|p| collect::process::matches_filter(p, &proc_filter));
            }

            // Build tree if tree mode is enabled
            let tree_mode = config.get_bool("proc_tree");
            if tree_mode {
                let children = collect::process::build_tree(&runner.proc_collector.procs);
                collect::process::generate_tree_prefixes(&mut runner.proc_collector.procs, &children);
            }

            // Calculate layout
            let shown: Vec<String> = config
                .get_string("shown_boxes")
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let layout = draw::layout::calc_sizes(&draw::layout::LayoutConfig {
                term_width: tw,
                term_height: th,
                shown_boxes: &shown,
                cpu_bottom: config.get_bool("cpu_bottom"),
                mem_below_net: config.get_bool("mem_below_net"),
                proc_left: config.get_bool("proc_left"),
                core_count: runner.cpu.info.core_count,
                gpu_count: runner.gpu.gpu_count(),
            });

            // Build output
            let mut output = String::new();
            output.push_str(term::SYNC_START);
            output.push_str("\x1b[2J");

            if let Some(ref cpu_dim) = layout.cpu {
                let area = ui::BoxArea { x: cpu_dim.x, y: cpu_dim.y, width: cpu_dim.width, height: cpu_dim.height, rounded };
                output.push_str(&ui::cpu_box::draw(
                    &runner.cpu.info,
                    &area,
                    &theme,
                ));
            }
            for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
                if gi < runner.gpu.gpus.len() {
                    let area = ui::BoxArea { x: gpu_dim.x, y: gpu_dim.y, width: gpu_dim.width, height: gpu_dim.height, rounded };
                    output.push_str(&ui::gpu_box::draw(
                        &runner.gpu.gpus[gi],
                        gi,
                        &area,
                        &theme,
                    ));
                }
            }
            if let Some(ref mem_dim) = layout.mem {
                let area = ui::BoxArea { x: mem_dim.x, y: mem_dim.y, width: mem_dim.width, height: mem_dim.height, rounded };
                output.push_str(&ui::mem_box::draw(
                    &runner.mem.info,
                    &area,
                    &theme,
                ));
            }
            if let Some(ref net_dim) = layout.net {
                let iface = &runner.net.selected_iface;
                let net_info = runner
                    .net
                    .current_net
                    .get(iface)
                    .cloned()
                    .unwrap_or_default();
                let area = ui::BoxArea { x: net_dim.x, y: net_dim.y, width: net_dim.width, height: net_dim.height, rounded };
                output.push_str(&ui::net_box::draw(
                    &net_info,
                    iface,
                    &area,
                    &theme,
                ));
            }
            if let Some(ref proc_dim) = layout.proc_box {
                clamp_proc_selection(&runner.proc_collector.procs, proc_dim.height, &mut proc_selected, &mut proc_start);
                let detailed_pid = config.get_int("detailed_pid") as u32;
                let pf = config.get_string("proc_filter").to_string();
                let is_filtering = menu_state == MenuState::Filter;
                let area = ui::BoxArea { x: proc_dim.x, y: proc_dim.y, width: proc_dim.width, height: proc_dim.height, rounded };
                let view = ui::ProcView { start: proc_start, selected: proc_selected, sort_by: &sort_by, sort_reversed: reversed, tree_mode, detailed_pid, filter: &pf, filtering: is_filtering };
                output.push_str(&ui::proc_box::draw_with_sort(
                    &runner.proc_collector.procs,
                    &area,
                    &view,
                    &theme,
                ));
            }

            output.push_str(term::SYNC_END);
            let _ = terminal.write_raw(&output);
            cached_layout = Some(layout);
        }

        // Proc-only redraw: just re-render the proc box without collecting data
        if (menu_state == MenuState::None || menu_state == MenuState::Filter) && needs_proc_redraw {
            needs_proc_redraw = false;
            let sort_by = config.get_string("proc_sorting").to_string();
            let reversed = config.get_bool("proc_reversed");
            let tree_mode = config.get_bool("proc_tree");
            collect::process::sort_procs(&mut runner.proc_collector.procs, &sort_by, reversed);
            let proc_filter = config.get_string("proc_filter").to_string();
            if !proc_filter.is_empty() {
                runner.proc_collector.procs.retain(|p| collect::process::matches_filter(p, &proc_filter));
            }
            if tree_mode {
                let children = collect::process::build_tree(&runner.proc_collector.procs);
                collect::process::generate_tree_prefixes(&mut runner.proc_collector.procs, &children);
            }
            if let Some(ref layout) = cached_layout {
                if let Some(ref proc_dim) = layout.proc_box {
                    clamp_proc_selection(&runner.proc_collector.procs, proc_dim.height, &mut proc_selected, &mut proc_start);
                    let detailed_pid = config.get_int("detailed_pid") as u32;
                    let area = ui::BoxArea { x: proc_dim.x, y: proc_dim.y, width: proc_dim.width, height: proc_dim.height, rounded };
                    let pf = config.get_string("proc_filter").to_string();
                    let is_filtering = menu_state == MenuState::Filter;
                    let view = ui::ProcView { start: proc_start, selected: proc_selected, sort_by: &sort_by, sort_reversed: reversed, tree_mode, detailed_pid, filter: &pf, filtering: is_filtering };
                    let mut output = String::new();
                    output.push_str(term::SYNC_START);
                    output.push_str(&ui::proc_box::draw_with_sort(
                        &runner.proc_collector.procs,
                        &area,
                        &view,
                        &theme,
                    ));
                    output.push_str(term::SYNC_END);
                    let _ = terminal.write_raw(&output);
                }
            }
        }

        // Poll for input — wait at most until the next update deadline
        let remaining = next_update
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let poll_ms = remaining.clamp(10, 1000); // At least 10ms, at most 1s

        if input::poll(poll_ms) {
            if let Some(key) = input::get() {
                if key.is_empty()
                    || key.starts_with("mouse_")
                    || key == "resize"
                {
                    if key == "resize" {
                        needs_full_redraw = true;
                    }
                    continue;
                }
                match menu_state {
                    MenuState::Main => match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            menu_state = MenuState::None;
                            needs_full_redraw = true;
                        }
                        "up" | "k" | "shift_tab" => {
                            main_menu_selected = if main_menu_selected == 0 { 2 } else { main_menu_selected - 1 };
                            let menu_out = menu::main_menu::draw_with_selection(tw, th, main_menu_selected);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "down" | "j" | "tab" => {
                            main_menu_selected = (main_menu_selected + 1) % 3;
                            let menu_out = menu::main_menu::draw_with_selection(tw, th, main_menu_selected);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "enter" | "space" => {
                            match main_menu_selected {
                                0 => {
                                    // Options
                                    options_cat = 0;
                                    options_selected = 0;
                                    options_page = 0;
                                    let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                                    let _ = terminal.write_raw(&menu_out);
                                    menu_state = MenuState::Options;
                                }
                                1 => {
                                    // Help
                                    let menu_out = menu::help_menu::draw(tw, th);
                                    let _ = terminal.write_raw(&menu_out);
                                    menu_state = MenuState::Help;
                                }
                                2 => {
                                    // Quit
                                    break;
                                }
                                _ => {}
                            }
                        }
                        "o" | "f2" => {
                            options_cat = 0;
                            options_selected = 0;
                            options_page = 0;
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Options;
                        }
                        "h" | "?" | "f1" => {
                            // Show help menu
                            let menu_out = menu::help_menu::draw(tw, th);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Help;
                        }
                        _ => {}
                    },
                    MenuState::Help => match key.as_str() {
                        "q" => break,
                        "escape" | "h" | "?" | "f1" => {
                            menu_state = MenuState::None;
                            needs_full_redraw = true;
                        }
                        _ => {}
                    },
                    MenuState::Options => match key.as_str() {
                        "q" => break,
                        "escape" | "backspace" => {
                            menu_state = MenuState::None;
                            needs_full_redraw = true;
                        }
                        "tab" => {
                            options_cat = (options_cat + 1) % 5;
                            options_page = 0;
                            options_selected = 0;
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "shift_tab" => {
                            options_cat = if options_cat == 0 { 4 } else { options_cat - 1 };
                            options_page = 0;
                            options_selected = 0;
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "1" | "2" | "3" | "4" | "5" => {
                            let new_cat = key.parse::<usize>().unwrap_or(1) - 1;
                            if new_cat != options_cat {
                                options_cat = new_cat;
                                options_page = 0;
                                options_selected = 0;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "up" | "k" => {
                            if options_selected > 0 {
                                options_selected -= 1;
                            } else {
                                // wrap to previous page or last page
                                let pages = menu::options_menu::page_count(options_cat, th);
                                if options_page > 0 {
                                    options_page -= 1;
                                } else if pages > 1 {
                                    options_page = pages - 1;
                                }
                                options_selected = menu::options_menu::select_max(options_cat, options_page, th);
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "down" | "j" => {
                            let sm = menu::options_menu::select_max(options_cat, options_page, th);
                            if options_selected < sm {
                                options_selected += 1;
                            } else {
                                // wrap to next page or first page
                                let pages = menu::options_menu::page_count(options_cat, th);
                                if options_page < pages - 1 {
                                    options_page += 1;
                                } else if pages > 1 {
                                    options_page = 0;
                                }
                                options_selected = 0;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "page_up" => {
                            let pages = menu::options_menu::page_count(options_cat, th);
                            if pages > 1 {
                                options_page = if options_page > 0 { options_page - 1 } else { pages - 1 };
                                options_selected = 0;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "page_down" => {
                            let pages = menu::options_menu::page_count(options_cat, th);
                            if pages > 1 {
                                options_page = if options_page < pages - 1 { options_page + 1 } else { 0 };
                                options_selected = 0;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        "left" | "right" | "h" | "l" | "enter" | "space" => {
                            if let Some(opt_key) = menu::options_menu::opt_key(options_cat, options_page, options_selected, th) {
                                let kind = if config.bools.contains_key(opt_key) {
                                    menu::options_menu::OptKind::Bool
                                } else if config.ints.contains_key(opt_key) {
                                    menu::options_menu::OptKind::Int
                                } else if !menu::options_menu::browsable_values(opt_key).is_empty() {
                                    menu::options_menu::OptKind::Browsable
                                } else {
                                    menu::options_menu::OptKind::StringVal
                                };

                                let dir: i64 = if key == "left" || key == "h" { -1 } else { 1 };

                                match kind {
                                    menu::options_menu::OptKind::Bool => {
                                        config.flip(opt_key);
                                        rounded = config.get_bool("rounded_corners");
                                    }
                                    menu::options_menu::OptKind::Int => {
                                        menu::options_menu::step_int(opt_key, &mut config, dir);
                                    }
                                    menu::options_menu::OptKind::Browsable => {
                                        menu::options_menu::cycle_browsable(opt_key, &mut config, dir as i32);
                                        if opt_key == "color_theme" {
                                            let name = config.get_string("color_theme").to_string();
                                            theme = theme::Theme::from_name(&name);
                                            let base = format!(
                                                "{}{}",
                                                theme.c("main_fg"),
                                                theme.c("main_bg").replace("38;2", "48;2"),
                                            );
                                            let _ = terminal.write_raw(&base);
                                        }
                                    }
                                    menu::options_menu::OptKind::StringVal => {
                                        // No inline editing yet — strings shown read-only
                                    }
                                }
                            }
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&format!("{}{}{}", term::SYNC_START, menu_out, term::SYNC_END));
                        }
                        _ => {}
                    },
                    MenuState::Filter => match key.as_str() {
                        "escape" => {
                            menu_state = MenuState::None;
                            needs_proc_redraw = true;
                        }
                        "enter" => {
                            config.set_string("proc_filter", &filter_text);
                            menu_state = MenuState::None;
                            proc_selected = 0;
                            proc_start = 0;
                            needs_proc_redraw = true;
                        }
                        "backspace" => {
                            filter_text.pop();
                            config.set_string("proc_filter", &filter_text);
                            proc_selected = 0;
                            proc_start = 0;
                            needs_proc_redraw = true;
                        }
                        "delete" => {
                            filter_text.clear();
                            config.set_string("proc_filter", "");
                            proc_selected = 0;
                            proc_start = 0;
                            needs_proc_redraw = true;
                        }
                        s if s.len() == 1 && !s.starts_with('\x1b') => {
                            filter_text.push_str(s);
                            config.set_string("proc_filter", &filter_text);
                            proc_selected = 0;
                            proc_start = 0;
                            needs_proc_redraw = true;
                        }
                        _ => {}
                    },
                    MenuState::None => match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            main_menu_selected = 0;
                            let menu_out = menu::main_menu::draw_with_selection(tw, th, main_menu_selected);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Main;
                        }
                        "h" | "?" | "f1" => {
                            let menu_out = menu::help_menu::draw(tw, th);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Help;
                        }
                        "o" | "f2" => {
                            options_cat = 0;
                            options_selected = 0;
                            options_page = 0;
                            let menu_out = draw_options_menu(tw, th, &config, &theme, options_cat, options_selected, options_page);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Options;
                        }
                        // Preset cycling
                        "p" => {
                            let presets = config.preset_list();
                            if presets.len() > 1 {
                                let cur = config.get_int("current_preset");
                                let next = if cur < 0 || (cur + 1) >= presets.len() as i64 {
                                    0i64
                                } else {
                                    cur + 1
                                };
                                config.set_int("current_preset", next);
                                config.apply_preset(&presets[next as usize]);
                                needs_full_redraw = true;
                            }
                        }
                        "P" => {
                            let presets = config.preset_list();
                            if presets.len() > 1 {
                                let cur = config.get_int("current_preset");
                                let next = if cur <= 0 {
                                    presets.len() as i64 - 1
                                } else {
                                    cur - 1
                                };
                                config.set_int("current_preset", next);
                                config.apply_preset(&presets[next as usize]);
                                needs_full_redraw = true;
                            }
                        }
                        // Config reload
                        "ctrl_r" => {
                            let warnings = config.reload();
                            for w in &warnings {
                                tracing::warn!("{}", w);
                            }
                            // Reapply theme
                            let theme_name = config.get_string("color_theme").to_string();
                            theme = theme::Theme::from_name(&theme_name);
                            let base = format!(
                                "{}{}",
                                theme.c("main_fg"),
                                theme.c("main_bg").replace("38;2", "48;2"),
                            );
                            let _ = terminal.write_raw(&base);
                            rounded = config.get_bool("rounded_corners");
                            update_ms = config.get_int("update_ms") as u64;
                            needs_full_redraw = true;
                        }
                        "up" | "k"
                            if proc_selected > 0 => {
                                proc_selected -= 1;
                                needs_proc_redraw = true;
                            }
                        "down" | "j" => {
                            let count = runner.proc_collector.procs.len();
                            if proc_selected + 1 < count {
                                proc_selected += 1;
                                needs_proc_redraw = true;
                            }
                        }
                        "page_up" => {
                            let page = th.saturating_sub(10);
                            proc_selected = proc_selected.saturating_sub(page);
                            needs_proc_redraw = true;
                        }
                        "page_down" => {
                            let page = th.saturating_sub(10);
                            let count = runner.proc_collector.procs.len();
                            proc_selected = (proc_selected + page).min(count.saturating_sub(1));
                            needs_proc_redraw = true;
                        }
                        "home" | "g" => {
                            proc_selected = 0;
                            proc_start = 0;
                            needs_proc_redraw = true;
                        }
                        "end" | "G" => {
                            let count = runner.proc_collector.procs.len();
                            proc_selected = count.saturating_sub(1);
                            needs_proc_redraw = true;
                        }
                        "1" => {
                            config.toggle_box("cpu");
                            needs_full_redraw = true;
                        }
                        "2" => {
                            config.toggle_box("mem");
                            needs_full_redraw = true;
                        }
                        "3" => {
                            config.toggle_box("net");
                            needs_full_redraw = true;
                        }
                        "4" => {
                            config.toggle_box("proc");
                            needs_full_redraw = true;
                        }
                        "5" => {
                            // Toggle all detected GPU boxes
                            for i in 0..runner.gpu.gpu_count() {
                                config.toggle_box(&format!("gpu{i}"));
                            }
                            needs_full_redraw = true;
                        }
                        "d" => {
                            config.flip("show_disks");
                            needs_full_redraw = true;
                        }
                        // Process keybinds
                        "f" | "/" => {
                            menu_state = MenuState::Filter;
                            filter_text = config.get_string("proc_filter").to_string();
                            needs_full_redraw = true;
                        }
                        "e" => {
                            config.flip("proc_tree");
                            // Reset tree-related state when toggling
                            needs_full_redraw = true;
                        }
                        "r" => {
                            config.flip("proc_reversed");
                            needs_full_redraw = true;
                        }
                        "c" => {
                            config.flip("proc_per_core");
                            needs_full_redraw = true;
                        }
                        "i" => {
                            config.flip("io_mode");
                            needs_full_redraw = true;
                        }
                        "left" => {
                            let sort_opts = ["pid", "name", "command", "threads", "user", "memory", "cpu lazy", "cpu direct"];
                            let current = config.get_string("proc_sorting").to_string();
                            let idx = sort_opts.iter().position(|&s| s == current).unwrap_or(0);
                            let new_idx = if idx == 0 { sort_opts.len() - 1 } else { idx - 1 };
                            config.set_string("proc_sorting", sort_opts[new_idx]);
                            needs_full_redraw = true;
                        }
                        "right" => {
                            let sort_opts = ["pid", "name", "command", "threads", "user", "memory", "cpu lazy", "cpu direct"];
                            let current = config.get_string("proc_sorting").to_string();
                            let idx = sort_opts.iter().position(|&s| s == current).unwrap_or(0);
                            let new_idx = (idx + 1) % sort_opts.len();
                            config.set_string("proc_sorting", sort_opts[new_idx]);
                            needs_full_redraw = true;
                        }
                        "t"
                            // Terminate selected process
                            if proc_selected < runner.proc_collector.procs.len() => {
                                let pid = runner.proc_collector.procs[proc_selected].pid;
                                terminate_process(pid);
                                needs_full_redraw = true;
                            }
                        "enter"
                            // Toggle process detailed view
                            if proc_selected < runner.proc_collector.procs.len() => {
                                let pid = runner.proc_collector.procs[proc_selected].pid;
                                let current_detailed = config.get_int("detailed_pid");
                                if current_detailed == pid as i64 {
                                    // Close detail panel
                                    config.set_int("detailed_pid", 0);
                                } else {
                                    config.set_int("detailed_pid", pid as i64);
                                }
                                needs_full_redraw = true;
                            }
                        // Network keybinds
                        "b"
                            if !runner.net.interfaces.is_empty() => {
                                let idx = runner.net.interfaces.iter()
                                    .position(|s| s == &runner.net.selected_iface)
                                    .unwrap_or(0);
                                let new_idx = if idx == 0 { runner.net.interfaces.len() - 1 } else { idx - 1 };
                                runner.net.selected_iface = runner.net.interfaces[new_idx].clone();
                                needs_full_redraw = true;
                            }
                        "n"
                            if !runner.net.interfaces.is_empty() => {
                                let idx = runner.net.interfaces.iter()
                                    .position(|s| s == &runner.net.selected_iface)
                                    .unwrap_or(0);
                                let new_idx = (idx + 1) % runner.net.interfaces.len();
                                runner.net.selected_iface = runner.net.interfaces[new_idx].clone();
                                needs_full_redraw = true;
                            }
                        "a" => {
                            config.flip("net_auto");
                            needs_full_redraw = true;
                        }
                        "y" => {
                            config.flip("net_sync");
                            needs_full_redraw = true;
                        }
                        // Network zero reset
                        "z" => {
                            let iface = runner.net.selected_iface.clone();
                            if let Some(net_info) = runner.net.current_net.get_mut(&iface) {
                                let dl = net_info.stat.get("download").cloned().unwrap_or_default();
                                let ul = net_info.stat.get("upload").cloned().unwrap_or_default();
                                if dl.offset + ul.offset > 0 {
                                    // Already offset — clear it
                                    if let Some(d) = net_info.stat.get_mut("download") {
                                        d.offset = 0;
                                    }
                                    if let Some(u) = net_info.stat.get_mut("upload") {
                                        u.offset = 0;
                                    }
                                } else {
                                    // Set offset to current last + rollover
                                    if let Some(d) = net_info.stat.get_mut("download") {
                                        d.offset = d.last + d.rollover;
                                    }
                                    if let Some(u) = net_info.stat.get_mut("upload") {
                                        u.offset = u.last + u.rollover;
                                    }
                                }
                                needs_full_redraw = true;
                            }
                        }
                        // Update rate keybinds
                        "+" => {
                            let step = if update_ms > 2000 { 1000 } else { 100 };
                            let new_ms = (update_ms as i64 + step).min(86_400_000);
                            config.set_int("update_ms", new_ms);
                            update_ms = config.get_int("update_ms") as u64;
                            needs_full_redraw = true;
                        }
                        "-" => {
                            let step = if update_ms > 2000 { 1000 } else { 100 };
                            let new_ms = (update_ms as i64 - step).max(100);
                            config.set_int("update_ms", new_ms);
                            update_ms = config.get_int("update_ms") as u64;
                            needs_full_redraw = true;
                        }
                        _ => {}
                    },
                }
            }
        }
        // No else branch needed — the wall-clock check at the top of the loop
        // handles periodic updates regardless of input activity.
    }

    // Save config on exit
    if config.get_bool("save_config_on_exit") {
        let conf_path = tools::config_dir().join("rtop.conf");
        let _ = config.write(&conf_path);
    }
}

fn clamp_proc_selection(procs: &[crate::domain::process::ProcInfo], box_height: usize, selected: &mut usize, start: &mut usize) {
    let count = procs.len();
    let max_visible = box_height.saturating_sub(5);
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

fn draw_options_menu(tw: usize, th: usize, config: &config::Config, theme: &theme::Theme, cat: usize, selected: usize, page: usize) -> String {
    menu::options_menu::draw(tw, th, cat, selected, page, config, theme)
}

fn terminate_process(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::*;

    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
    }
}

