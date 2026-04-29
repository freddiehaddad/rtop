use std::path::Path;
use tracing_appender::rolling;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Initialize the logging system.
///
/// Logs are written to `rtop.log` inside `log_dir`. The file is truncated on
/// each run so it only contains the current session's output.
/// The log level is set from the config (ERROR, WARNING, INFO, DEBUG).
pub fn init(log_dir: &Path, level: &str) {
    let filter = match level.to_uppercase().as_str() {
        "ERROR" => tracing::Level::ERROR,
        "WARNING" | "WARN" => tracing::Level::WARN,
        "INFO" => tracing::Level::INFO,
        "DEBUG" => tracing::Level::DEBUG,
        "TRACE" => tracing::Level::TRACE,
        _ => tracing::Level::WARN,
    };

    // Create log directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(log_dir) {
        eprintln!("Failed to create log directory: {e}");
        return;
    }

    // Truncate the log file so each session starts fresh
    let log_file = log_dir.join("rtop.log");
    let _ = std::fs::write(&log_file, b"");

    let file_appender = rolling::never(log_dir, "rtop.log");

    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(false)
            .with_filter(tracing_subscriber::filter::LevelFilter::from_level(filter)),
    );

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        // Already initialized — ignore
    }
}

#[cfg(test)]
/// Convert btop log level string to tracing level name.
pub fn level_from_str(s: &str) -> &'static str {
    match s.to_uppercase().as_str() {
        "ERROR" => "ERROR",
        "WARNING" | "WARN" => "WARN",
        "INFO" => "INFO",
        "DEBUG" => "DEBUG",
        "TRACE" => "TRACE",
        "DISABLED" => "OFF",
        _ => "WARN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_from_str_maps_correctly() {
        assert_eq!(level_from_str("ERROR"), "ERROR");
        assert_eq!(level_from_str("WARNING"), "WARN");
        assert_eq!(level_from_str("INFO"), "INFO");
        assert_eq!(level_from_str("DEBUG"), "DEBUG");
        assert_eq!(level_from_str("DISABLED"), "OFF");
        assert_eq!(level_from_str("garbage"), "WARN");
    }

    #[test]
    fn level_from_str_case_insensitive() {
        assert_eq!(level_from_str("error"), "ERROR");
        assert_eq!(level_from_str("Warning"), "WARN");
        assert_eq!(level_from_str("debug"), "DEBUG");
    }

    #[test]
    fn init_creates_log_directory() {
        let tmp = std::env::temp_dir().join("rtop_test_log");
        let _ = std::fs::remove_dir_all(&tmp);
        // Just verify it doesn't panic — actual subscriber may already be set
        init(&tmp, "DEBUG");
        assert!(tmp.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
