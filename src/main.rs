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

    // Init theme (kept for later use)
    let _theme = theme::Theme::new();

    // Init runner (collectors)
    let mut runner = runner::Runner::new();

    let rounded = config.get_bool("rounded_corners");
    let update_ms = config.get_int("update_ms") as u64;

    let mut menu_active = false;

    // Main event loop
    loop {
        // Check resize
        terminal.refresh();
        let (tw, th) = terminal.size();
        let tw = tw as usize;
        let th = th as usize;

        if !menu_active {
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
                if menu_active {
                    // Any key dismisses the menu
                    match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            menu_active = false;
                        }
                        _ => {
                            menu_active = false;
                        }
                    }
                } else {
                    match key.as_str() {
                        "q" => break,
                        "escape" | "m" => {
                            // Show menu overlay
                            let menu_out = menu::main_menu::draw(tw, th);
                            let _ = terminal.write_raw(&menu_out);
                            menu_active = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

