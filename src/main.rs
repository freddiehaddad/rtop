mod app;
mod banner;
mod cli;
mod collect;
mod config;
mod dirty;
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
        theme.c("main_bg").replace("38;2", "48;2") // fg escape → bg escape
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

    // Snapshot the startup layout for preset 0
    let initial = config.get_string("shown_boxes").to_string();
    config.set_string("initial_shown_boxes", &initial);

    app::run(&mut config, &mut terminal, &mut theme, &mut runner);
}
