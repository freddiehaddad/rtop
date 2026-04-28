mod app;
mod banner;
mod cli;
mod collect;
mod config;
mod config_keys;
mod dirty;
mod domain;
mod draw;
mod handlers;
mod input;
mod log;
mod menu;
mod runner;
mod term;
mod theme;
mod theme_keys;
mod tools;
mod ui;

use crate::config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk};
use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    // --default-config: print and exit
    if cli.default_config {
        print!("{}", config::Config::new().to_config_string());
        return;
    }

    // Init logging
    let log_level = if cli.debug { "DEBUG" } else { "WARNING" };
    let log_dir = tools::data_dir();
    log::init(&log_dir, log_level);

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
        config.set_bool(bk::LOWCOLOR, true);
    }
    if cli.tty {
        config.set_bool(bk::FORCE_TTY, true);
    }
    if cli.no_tty {
        config.set_bool(bk::FORCE_TTY, false);
    }
    if let Some(ms) = cli.update_ms {
        config.set_int(ik::UPDATE_MS, ms as i64);
    }
    if let Some(ref f) = cli.filter {
        config.set_string(sk::PROC_FILTER, f);
    }
    if let Some(p) = cli.preset {
        config.set_int(ik::CURRENT_PRESET, p as i64);
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
    let mut theme = theme::Theme::from_name(config.get_string(sk::COLOR_THEME));

    // Set terminal base colors from theme
    let base_colors = theme.base_style(config.get_bool(bk::THEME_BACKGROUND));
    let _ = terminal.write_raw(&base_colors);

    app::run(&mut config, &mut terminal, &mut theme);
}
