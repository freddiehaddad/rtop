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
mod overlay;
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

    if cli.default_config {
        let config = config::Config::new();
        let output = toml::to_string_pretty(&config).unwrap_or_default();
        print!("{output}");
        return;
    }

    let mut config = config::Config::new();
    let default_conf_path = tools::config_dir().join("rtop.toml");
    let load_warnings = if let Some(ref path) = cli.config_file {
        config.load(path)
    } else if default_conf_path.exists() {
        config.load(&default_conf_path)
    } else {
        Vec::new()
    };

    let log_dir = tools::data_dir();
    log::init(&log_dir, config.log.log_level).expect("logging must initialise at startup");
    let active_config_file = cli.config_file.as_deref().or_else(|| {
        default_conf_path
            .exists()
            .then_some(default_conf_path.as_path())
    });
    log::startup_banner(
        config.log.log_level,
        &log_dir.join("rtop.log"),
        active_config_file,
    );

    for w in &load_warnings {
        tracing::warn!(subsystem = %log::Subsystem::Config, warning = %w, "config load warning");
    }

    if let Some(ms) = cli.update_ms {
        config.refresh.update_ms = ms as i64;
    }
    if let Some(ref f) = cli.filter {
        config.view.proc_filter = f.clone();
    }

    let mut terminal = match term::Terminal::init(config.ui.terminal_sync) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(
                subsystem = %log::Subsystem::Terminal,
                error = %e,
                "terminal init failed",
            );
            eprintln!("Failed to initialize terminal: {e}");
            return;
        }
    };

    let mut theme = theme::Theme::from_name(&config.ui.color_theme);

    let base_colors = theme.base_style(config.ui.theme_background);
    if let Err(e) = terminal.write_raw(&base_colors) {
        tracing::warn!(
            subsystem = %log::Subsystem::Terminal,
            error = %e,
            "terminal write failed",
        );
    }

    app::run(&mut config, &mut terminal, &mut theme);
}
