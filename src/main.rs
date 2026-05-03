mod app;
mod banner;
mod cli;
mod collect;
mod config;
mod dirty;
mod domain;
mod draw;
mod event;
mod handlers;
mod input;
mod log;
mod menu;
mod runner;
mod term;
mod theme;
mod theme_keys;
mod themes;
mod tools;
mod ui;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    // --default-config: print and exit
    if cli.default_config {
        let config = config::Config::new();
        let output = toml::to_string_pretty(&config).unwrap_or_default();
        print!("{output}");
        return;
    }

    // Load config (from CLI path or default location) BEFORE logging,
    // so we can read config.log_level.
    let mut config = config::Config::new();
    let default_conf_path = tools::config_dir().join("rtop.toml");
    let load_warnings = if let Some(ref path) = cli.config_file {
        config.load(path)
    } else if default_conf_path.exists() {
        config.load(&default_conf_path)
    } else {
        Vec::new()
    };

    // Init logging — failure at startup is non-recoverable.
    let log_dir = tools::data_dir();
    log::init(&log_dir, config.log_level).expect("logging must initialise at startup");

    for w in &load_warnings {
        tracing::warn!("{}", w);
    }

    // Apply CLI overrides
    if let Some(ms) = cli.update_ms {
        config.update_ms = ms as i64;
    }
    if let Some(ref f) = cli.filter {
        config.proc_filter = f.clone();
    }
    if let Some(p) = cli.preset {
        config.current_preset = p as i64;
    }

    // Init terminal
    let mut terminal = match term::Terminal::init(config.terminal_sync) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize terminal: {e}");
            return;
        }
    };

    // Init theme
    let mut theme = theme::Theme::from_name(&config.color_theme);

    // Set terminal base colors from theme
    let base_colors = theme.base_style(config.theme_background);
    let _ = terminal.write_raw(&base_colors);

    app::run(&mut config, &mut terminal, &mut theme);
}
