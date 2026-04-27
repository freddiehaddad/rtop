use clap::Parser;
use std::path::PathBuf;

/// A terminal-based system monitor for Windows, inspired by btop.
#[derive(Parser, Debug)]
#[command(name = "rtop", version, about)]
pub struct Cli {
    /// Path to config file.
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<PathBuf>,

    /// Enable debug mode.
    #[arg(short = 'd', long = "debug")]
    pub debug: bool,

    /// Set initial process filter.
    #[arg(short = 'f', long = "filter")]
    pub filter: Option<String>,

    /// Disable truecolor (256 colors only).
    #[arg(short = 'l', long = "low-color")]
    pub low_color: bool,

    /// Start with preset (0-9).
    #[arg(short = 'p', long = "preset", value_parser = clap::value_parser!(u32).range(0..=9))]
    pub preset: Option<u32>,

    /// Force TTY mode (16 colors, ASCII symbols).
    #[arg(short = 't', long = "tty", conflicts_with = "no_tty")]
    pub tty: bool,

    /// Force disable TTY mode.
    #[arg(long = "no-tty", conflicts_with = "tty")]
    pub no_tty: bool,

    /// Update rate in milliseconds (minimum 100).
    #[arg(short = 'u', long = "update", value_parser = clap::value_parser!(u32).range(100..))]
    pub update_ms: Option<u32>,

    /// Print default config to stdout and exit.
    #[arg(long = "default-config")]
    pub default_config: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_args() {
        let cli = Cli::parse_from(["rtop"]);
        assert!(!cli.debug);
        assert!(cli.config_file.is_none());
        assert!(cli.filter.is_none());
        assert!(cli.preset.is_none());
        assert!(cli.update_ms.is_none());
    }

    #[test]
    fn parse_debug_flag() {
        let cli = Cli::parse_from(["rtop", "-d"]);
        assert!(cli.debug);
    }

    #[test]
    fn parse_config_file() {
        let cli = Cli::parse_from(["rtop", "-c", "my.conf"]);
        assert_eq!(cli.config_file.unwrap(), PathBuf::from("my.conf"));
    }

    #[test]
    fn parse_filter() {
        let cli = Cli::parse_from(["rtop", "-f", "chrome"]);
        assert_eq!(cli.filter.unwrap(), "chrome");
    }

    #[test]
    fn parse_preset_valid() {
        let cli = Cli::parse_from(["rtop", "-p", "5"]);
        assert_eq!(cli.preset.unwrap(), 5);
    }

    #[test]
    fn parse_preset_out_of_range_error() {
        let result = Cli::try_parse_from(["rtop", "-p", "10"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_tty_and_no_tty_conflict() {
        let result = Cli::try_parse_from(["rtop", "-t", "--no-tty"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_update_ms_minimum() {
        let result = Cli::try_parse_from(["rtop", "-u", "50"]);
        assert!(result.is_err()); // Below minimum of 100
    }

    #[test]
    fn parse_default_config() {
        let cli = Cli::parse_from(["rtop", "--default-config"]);
        assert!(cli.default_config);
    }

    #[test]
    fn parse_low_color() {
        let cli = Cli::parse_from(["rtop", "-l"]);
        assert!(cli.low_color);
    }

    #[test]
    fn parse_short_flags() {
        let cli = Cli::parse_from(["rtop", "-d", "-l", "-t"]);
        assert!(cli.debug);
        assert!(cli.low_color);
        assert!(cli.tty);
    }

    #[test]
    fn parse_update_valid() {
        let cli = Cli::parse_from(["rtop", "-u", "500"]);
        assert_eq!(cli.update_ms.unwrap(), 500);
    }
}
