mod banner;
mod cell_buffer;
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
    let log_dir = directories::BaseDirs::new()
        .map(|d| d.data_local_dir().join("rtop"))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    log::init(&log_dir, "WARNING");

    // Load config
    let mut config = config::Config::new();
    if let Some(ref path) = cli.config_file {
        let warnings = config.load(path);
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
    let theme = theme::Theme::new();

    // Set terminal base colors from theme
    let base_colors = format!(
        "{}{}",
        theme.c("main_fg"),
        theme.c("main_bg").replace("38;2", "48;2")  // fg escape → bg escape
    );
    let _ = terminal.write_raw(&base_colors);

    // Init runner (collectors)
    let mut runner = runner::Runner::new();

    let mut rounded = config.get_bool("rounded_corners");
    let update_ms = config.get_int("update_ms") as u64;

    #[derive(PartialEq)]
    enum MenuState {
        None,
        Main,
        Help,
        Options,
    }

    let mut menu_state = MenuState::None;

    let mut menu_active = false;
    let mut options_selected: usize = 0;

    // Main event loop
    loop {
        // Check resize
        terminal.refresh();
        let (tw, th) = terminal.size();
        let tw = tw as usize;
        let th = th as usize;

        if menu_state == MenuState::None {
            // Collect data
            runner.collect_all();

            // Calculate layout
            let shown: Vec<String> = config
                .get_string("shown_boxes")
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let layout = draw::layout::calc_sizes(
                tw,
                th,
                &shown,
                config.get_bool("cpu_bottom"),
                config.get_bool("mem_below_net"),
                config.get_bool("proc_left"),
                runner.cpu.info.core_count,
            );

            // Build output
            let mut output = String::new();
            output.push_str(term::SYNC_START);
            output.push_str("\x1b[2J");

            if let Some(ref cpu_dim) = layout.cpu {
                output.push_str(&ui::cpu_box::draw(
                    &mut cell_buffer::CellBuffer::new(tw, th),
                    &runner.cpu.info,
                    cpu_dim.x,
                    cpu_dim.y,
                    cpu_dim.width,
                    cpu_dim.height,
                    rounded,
                    &theme,
                ));
            }
            if let Some(ref mem_dim) = layout.mem {
                output.push_str(&ui::mem_box::draw(
                    &runner.mem.info,
                    mem_dim.x,
                    mem_dim.y,
                    mem_dim.width,
                    mem_dim.height,
                    rounded,
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
                output.push_str(&ui::net_box::draw(
                    &net_info,
                    iface,
                    net_dim.x,
                    net_dim.y,
                    net_dim.width,
                    net_dim.height,
                    rounded,
                    &theme,
                ));
            }
            if let Some(ref proc_dim) = layout.proc_box {
                output.push_str(&ui::proc_box::draw(
                    &runner.proc_collector.procs,
                    proc_dim.x,
                    proc_dim.y,
                    proc_dim.width,
                    proc_dim.height,
                    rounded,
                    0,
                    0,
                    &theme,
                ));
            }

            output.push_str(term::SYNC_END);
            let _ = terminal.write_raw(&output);
        }

        // Poll for input
        if input::poll(update_ms) {
            if let Some(key) = input::get() {
                if key.is_empty() {
                    continue; // Skip empty events (key releases)
                }
                match menu_state {
                    MenuState::Main => match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            menu_state = MenuState::None;
                        }
                        "o" | "f2" => {
                            options_selected = 0;
                            let menu_out = draw_options_menu(tw, th, &config, options_selected, &theme);
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
                        }
                        _ => {}
                    },
                    MenuState::Options => match key.as_str() {
                        "q" => break,
                        "escape" | "o" | "f2" => {
                            menu_state = MenuState::None;
                        }
                        "up" | "k" => {
                            if options_selected > 0 {
                                options_selected -= 1;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, options_selected, &theme);
                            let _ = terminal.write_raw(&menu_out);
                        }
                        "down" | "j" => {
                            let entries = build_options_entries(&config);
                            if options_selected < entries.len().saturating_sub(1) {
                                options_selected += 1;
                            }
                            let menu_out = draw_options_menu(tw, th, &config, options_selected, &theme);
                            let _ = terminal.write_raw(&menu_out);
                        }
                        "enter" | "space" => {
                            // Toggle the selected option
                            let entries = build_options_entries(&config);
                            if let Some((key_name, _, is_bool)) = entries.get(options_selected) {
                                if *is_bool {
                                    config.flip(key_name);
                                    // Update rounded if changed
                                    rounded = config.get_bool("rounded_corners");
                                }
                            }
                            let menu_out = draw_options_menu(tw, th, &config, options_selected, &theme);
                            let _ = terminal.write_raw(&menu_out);
                        }
                        _ => {}
                    },
                    MenuState::None => match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            let menu_out = menu::main_menu::draw(tw, th);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Main;
                        }
                        "h" | "?" | "f1" => {
                            let menu_out = menu::help_menu::draw(tw, th);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Help;
                        }
                        "o" | "f2" => {
                            options_selected = 0;
                            let menu_out = draw_options_menu(tw, th, &config, options_selected, &theme);
                            let _ = terminal.write_raw(&menu_out);
                            menu_state = MenuState::Options;
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

fn build_options_entries(config: &config::Config) -> Vec<(String, String, bool)> {
    vec![
        ("color_theme".into(), config.get_string("color_theme").to_string(), false),
        ("truecolor".into(), config.get_bool("truecolor").to_string(), true),
        ("rounded_corners".into(), config.get_bool("rounded_corners").to_string(), true),
        ("vim_keys".into(), config.get_bool("vim_keys").to_string(), true),
        ("show_battery".into(), config.get_bool("show_battery").to_string(), true),
        ("show_battery_watts".into(), config.get_bool("show_battery_watts").to_string(), true),
        ("theme_background".into(), config.get_bool("theme_background").to_string(), true),
        ("force_tty".into(), config.get_bool("force_tty").to_string(), true),
        ("disable_mouse".into(), config.get_bool("disable_mouse").to_string(), true),
        ("terminal_sync".into(), config.get_bool("terminal_sync").to_string(), true),
        ("base_10_sizes".into(), config.get_bool("base_10_sizes").to_string(), true),
        ("background_update".into(), config.get_bool("background_update").to_string(), true),
        ("save_config_on_exit".into(), config.get_bool("save_config_on_exit").to_string(), true),
        ("cpu_bottom".into(), config.get_bool("cpu_bottom").to_string(), true),
        ("cpu_single_graph".into(), config.get_bool("cpu_single_graph").to_string(), true),
        ("cpu_invert_lower".into(), config.get_bool("cpu_invert_lower").to_string(), true),
        ("check_temp".into(), config.get_bool("check_temp").to_string(), true),
        ("show_coretemp".into(), config.get_bool("show_coretemp").to_string(), true),
        ("show_cpu_freq".into(), config.get_bool("show_cpu_freq").to_string(), true),
        ("show_uptime".into(), config.get_bool("show_uptime").to_string(), true),
        ("mem_graphs".into(), config.get_bool("mem_graphs").to_string(), true),
        ("mem_below_net".into(), config.get_bool("mem_below_net").to_string(), true),
        ("show_disks".into(), config.get_bool("show_disks").to_string(), true),
        ("show_swap".into(), config.get_bool("show_swap").to_string(), true),
        ("show_io_stat".into(), config.get_bool("show_io_stat").to_string(), true),
        ("io_mode".into(), config.get_bool("io_mode").to_string(), true),
        ("net_auto".into(), config.get_bool("net_auto").to_string(), true),
        ("net_sync".into(), config.get_bool("net_sync").to_string(), true),
        ("swap_upload_download".into(), config.get_bool("swap_upload_download").to_string(), true),
        ("proc_left".into(), config.get_bool("proc_left").to_string(), true),
        ("proc_tree".into(), config.get_bool("proc_tree").to_string(), true),
        ("proc_colors".into(), config.get_bool("proc_colors").to_string(), true),
        ("proc_gradient".into(), config.get_bool("proc_gradient").to_string(), true),
        ("proc_per_core".into(), config.get_bool("proc_per_core").to_string(), true),
        ("proc_mem_bytes".into(), config.get_bool("proc_mem_bytes").to_string(), true),
        ("proc_cpu_graphs".into(), config.get_bool("proc_cpu_graphs").to_string(), true),
        ("proc_reversed".into(), config.get_bool("proc_reversed").to_string(), true),
        ("proc_filter_kernel".into(), config.get_bool("proc_filter_kernel").to_string(), true),
    ]
}

fn draw_options_menu(tw: usize, th: usize, config: &config::Config, selected: usize, theme: &theme::Theme) -> String {
    let entries = build_options_entries(config);
    menu::options_menu::draw(tw, th, &entries, selected, theme)
}

