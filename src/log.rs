//! Logging subsystem.
//!
//! `tracing_subscriber::filter::LevelFilter` is the canonical
//! type used everywhere — Config field, options menu, reload
//! handle. The `serde_filter` adapter bridges serde to its
//! upstream `Display` / `FromStr`. Lowercase canonical names
//! (matching upstream `LevelFilter::Display`) are used in TOML
//! and the options menu; the log file's per-line level prefix
//! is produced independently by the `tracing_subscriber::fmt`
//! formatter.
//!
//! The subscriber is installed with a `reload::Layer` so the
//! level can be changed at runtime via [`set_level`] without
//! restarting. The single reload handle lives in a module-
//! private `OnceLock` because the `tracing` global subscriber
//! is itself process-wide.

use std::path::Path;
use std::sync::OnceLock;
use thiserror::Error;
use tracing_appender::rolling;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Default value for the `log_level` config field when missing
/// from `rtop.toml` or for new configs.
pub fn default_filter() -> LevelFilter {
    LevelFilter::WARN
}

/// All filter settings, in the order the options menu cycles them.
///
/// The only consumer is the sync test that guarantees
/// `FILTER_NAMES[i] == FILTERS[i].to_string()`. Production code
/// receives the canonical lowercase names through [`FILTER_NAMES`];
/// the typed slice is not needed at runtime because the menu cycles
/// strings, not `LevelFilter` values.
#[cfg(test)]
const FILTERS: &[LevelFilter] = &[
    LevelFilter::OFF,
    LevelFilter::ERROR,
    LevelFilter::WARN,
    LevelFilter::INFO,
    LevelFilter::DEBUG,
    LevelFilter::TRACE,
];

/// Lowercase canonical names for the menu, matching upstream
/// `LevelFilter::Display`. A unit test asserts
/// `FILTER_NAMES[i] == FILTERS[i].to_string()` so the two
/// cannot drift.
pub const FILTER_NAMES: &[&str] = &["off", "error", "warn", "info", "debug", "trace"];

/// Errors returned by [`init`].
#[derive(Debug, Error)]
pub enum InitError {
    #[error("logging already initialised")]
    AlreadyInitialised,
    #[error("global tracing subscriber already set")]
    SubscriberSet,
    #[error("failed to prepare log directory: {0}")]
    PrepareLogDir(#[from] std::io::Error),
}

/// Serde adapter for `tracing_subscriber::filter::LevelFilter`.
///
/// Used via `#[serde(with = "crate::log::serde_filter")]` on the
/// `Config::log_level` field. Both directions delegate to
/// upstream `Display` / `FromStr`.
pub mod serde_filter {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S: Serializer>(filter: &LevelFilter, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(filter)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<LevelFilter, D::Error> {
        let raw = <String>::deserialize(d)?;
        LevelFilter::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

static RELOAD_HANDLE: OnceLock<reload::Handle<LevelFilter, Registry>> = OnceLock::new();

/// Prepare the log directory: create it if missing, truncate the
/// log file so each session starts fresh.
fn prepare_log_dir(log_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let log_file = log_dir.join("rtop.log");
    std::fs::write(&log_file, b"")?;
    Ok(())
}

/// Initialise the logging system.
///
/// Logs are written to `rtop.log` inside `log_dir`; the file is
/// truncated on each run. The level filter is reloadable via
/// [`set_level`].
pub fn init(log_dir: &Path, filter: LevelFilter) -> Result<(), InitError> {
    prepare_log_dir(log_dir)?;
    let file_appender = rolling::never(log_dir, "rtop.log");
    let (filter_layer, handle) = reload::Layer::new(filter);
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(false)
            .with_filter(filter_layer),
    );
    tracing::subscriber::set_global_default(subscriber).map_err(|_| InitError::SubscriberSet)?;
    RELOAD_HANDLE
        .set(handle)
        .map_err(|_| InitError::AlreadyInitialised)
}

/// Apply a new level filter to the live subscriber.
///
/// Panics if [`init`] was not called first; the in-tree callers
/// all run after `main` has called `init`.
pub fn set_level(filter: LevelFilter) {
    let handle = RELOAD_HANDLE
        .get()
        .expect("log::set_level called before log::init");
    handle
        .modify(|f| *f = filter)
        .expect("reload handle should accept a LevelFilter");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn default_filter_is_warn() {
        assert_eq!(default_filter(), LevelFilter::WARN);
    }

    #[test]
    fn filter_names_match_filters() {
        assert_eq!(FILTER_NAMES.len(), FILTERS.len());
        for (name, filter) in FILTER_NAMES.iter().zip(FILTERS.iter()) {
            assert_eq!(*name, filter.to_string());
        }
    }

    #[test]
    fn filter_names_round_trip_through_from_str() {
        for (name, filter) in FILTER_NAMES.iter().zip(FILTERS.iter()) {
            assert_eq!(LevelFilter::from_str(name).unwrap(), *filter);
        }
    }

    #[test]
    fn from_str_is_case_insensitive() {
        assert_eq!(LevelFilter::from_str("WARN").unwrap(), LevelFilter::WARN);
        assert_eq!(LevelFilter::from_str("Warn").unwrap(), LevelFilter::WARN);
        assert_eq!(LevelFilter::from_str("warn").unwrap(), LevelFilter::WARN);
    }

    #[test]
    fn prepare_log_dir_creates_missing_directory() {
        let tmp = std::env::temp_dir().join("rtop_log_dir_test");
        let _ = std::fs::remove_dir_all(&tmp);
        prepare_log_dir(&tmp).expect("prepare_log_dir must succeed");
        assert!(tmp.exists());
        assert!(tmp.join("rtop.log").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prepare_log_dir_truncates_existing_log_file() {
        let tmp = std::env::temp_dir().join("rtop_log_dir_truncate_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let log_file = tmp.join("rtop.log");
        std::fs::write(&log_file, b"stale content").unwrap();
        prepare_log_dir(&tmp).expect("prepare_log_dir must succeed");
        assert_eq!(std::fs::read(&log_file).unwrap(), b"");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
