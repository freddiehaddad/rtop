//! Logging subsystem.
//!
//! `tracing_subscriber::filter::LevelFilter` is the canonical type
//! used everywhere — Config field, options menu, reload handle. The
//! [`serde_filter`] adapter bridges serde to upstream `Display` /
//! `FromStr`. Lowercase canonical names (matching upstream
//! `LevelFilter::Display`) are used in TOML and the options menu;
//! the log file's per-line level prefix is produced independently by
//! the `tracing_subscriber::fmt` formatter.
//!
//! The subscriber is installed with a `reload::Layer` so the level
//! can be changed at runtime via [`set_level`] without restarting. The
//! single reload handle lives in a module-private `OnceLock` because
//! the `tracing` global subscriber is itself process-wide.
//!
//! # Logging conventions
//!
//! Every `tracing::*!` call must include
//! `subsystem = %log::Subsystem::Foo` as a structured field. Vendor
//! and Win32 return codes use `code = %log::Hex(ret)`. Other typed
//! values (pid, device index, theme name, option key) use structured
//! fields, not message interpolation. The message string is a stable
//! present-tense identifier of the operation
//! (`"PdhCollectQueryData failed"`, `"option toggled"`).
//!
//! # Level rubric
//!
//! - `ERROR` — unrecoverable failure that requires the process to
//!   exit. Reserved for panic-hook output and terminal-init failure
//!   in `main`.
//! - `WARN` — recoverable failure that degrades observable behavior;
//!   the user might notice missing data and want to know why.
//! - `INFO` — significant lifecycle event the user may want to
//!   confirm (subscriber installed, level changed, config reloaded,
//!   GPU vendor detected, theme loaded, user took a state-changing
//!   action).
//! - `DEBUG` — per-cycle or per-resource diagnostics for bug reports;
//!   includes vendor-init failures expected on systems without that
//!   vendor.
//! - `TRACE` — reserved.
//!
//! High-frequency events (per-frame render, per-keystroke
//! navigation, sub-threshold resize bursts, RAII-drop errors during
//! shutdown) are not logged.

use std::fmt as std_fmt;
use std::path::{Path, PathBuf};
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

/// Errors returned by [`set_level`].
#[derive(Debug, Error)]
pub enum SetLevelError {
    #[error(transparent)]
    Install(#[from] InitError),
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

/// Identifies which part of rtop emitted a log event.
///
/// Every `tracing::*!` call attaches one of these as the `subsystem`
/// structured field via `subsystem = %Subsystem::Foo`. The `Display`
/// impl produces lowercase snake_case so log lines stay consistent
/// and greppable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Cpu,
    CpuThermal,
    Memory,
    Disk,
    Network,
    Process,
    GpuNvml,
    GpuNvapi,
    GpuIgcl,
    GpuAdl,
    /// Cross-vendor GPU subsystem (discovery, per-device collector
    /// lifecycle) — distinct from the per-vendor variants above
    /// which name the SDK responsible for an individual log event.
    Gpu,
    Theme,
    Config,
    Terminal,
    Logger,
    Startup,
    Ui,
    Input,
    Runner,
}

impl std_fmt::Display for Subsystem {
    fn fmt(&self, f: &mut std_fmt::Formatter<'_>) -> std_fmt::Result {
        let s = match self {
            Self::Cpu => "cpu",
            Self::CpuThermal => "cpu_thermal",
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::Network => "network",
            Self::Process => "process",
            Self::GpuNvml => "gpu_nvml",
            Self::GpuNvapi => "gpu_nvapi",
            Self::GpuIgcl => "gpu_igcl",
            Self::GpuAdl => "gpu_adl",
            Self::Gpu => "gpu",
            Self::Theme => "theme",
            Self::Config => "config",
            Self::Terminal => "terminal",
            Self::Logger => "logger",
            Self::Startup => "startup",
            Self::Ui => "ui",
            Self::Input => "input",
            Self::Runner => "runner",
        };
        f.write_str(s)
    }
}

/// `Display` wrapper that formats vendor / Win32 return codes as
/// `0x` prefixed, uppercase, 8-wide hex.
///
/// Used in structured fields: `code = %log::Hex(ret)` produces e.g.
/// `code=0x000000C5`. Standardises the format across NvAPI / NVML /
/// IGCL / ADL which today print decimal or `{:#x}` inconsistently.
pub struct Hex<T>(pub T);

impl<T: std_fmt::UpperHex> std_fmt::Display for Hex<T> {
    fn fmt(&self, f: &mut std_fmt::Formatter<'_>) -> std_fmt::Result {
        write!(f, "{:#010X}", self.0)
    }
}

/// `tracing_subscriber::fmt::time::FormatTime` implementation that
/// emits local wall-clock time with millisecond precision.
///
/// Format: `YYYY-MM-DD HH:MM:SS.mmm`. Sources the time from
/// `GetLocalTime` (Win32 `Win32_System_SystemInformation` feature,
/// already enabled), matching the `tools::format::format_clock`
/// idiom used by the in-UI clock.
pub struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std_fmt::Result {
        // SAFETY: GetLocalTime takes no input and writes to a stack-allocated
        // SYSTEMTIME struct returned by value. There are no preconditions.
        let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        write!(
            w,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
        )
    }
}

static RELOAD_HANDLE: OnceLock<reload::Handle<LevelFilter, Registry>> = OnceLock::new();
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Create the log directory if missing and truncate `rtop.log`.
fn prepare_log_dir(log_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let log_file = log_dir.join("rtop.log");
    std::fs::write(&log_file, b"")?;
    Ok(())
}

/// Install a process-wide panic hook that logs panics to `rtop.log`
/// at the moment they occur, then chains the previous hook so default
/// stderr behavior is preserved for users who run rtop from a parent
/// shell with `RUST_BACKTRACE=1`.
fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("(non-string panic payload)");
        tracing::error!(
            subsystem = %Subsystem::Logger,
            location = %location,
            "panic: {payload}",
        );
        previous_hook(info);
    }));
}

/// Build the file appender, install the subscriber as the global
/// default, install the panic hook, and store the reload handle.
/// Truncates `rtop.log`.
fn install_subscriber(log_dir: &Path, filter: LevelFilter) -> Result<(), InitError> {
    prepare_log_dir(log_dir)?;
    let file_appender = rolling::never(log_dir, "rtop.log");
    let (filter_layer, handle) = reload::Layer::new(filter);
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .with_timer(LocalTime)
            .with_filter(filter_layer),
    );
    tracing::subscriber::set_global_default(subscriber).map_err(|_| InitError::SubscriberSet)?;
    RELOAD_HANDLE
        .set(handle)
        .map_err(|_| InitError::AlreadyInitialised)?;
    install_panic_hook();
    tracing::info!(
        subsystem = %Subsystem::Logger,
        level = %filter,
        "subscriber installed",
    );
    Ok(())
}

/// Initialise the logging system.
///
/// `rtop.log` lives inside `log_dir`. With `filter` set to a
/// non-`OFF` level the directory is created, the file is
/// truncated, and the subscriber is installed immediately. With
/// `filter == LevelFilter::OFF` no subscriber is installed and
/// the filesystem is not touched; the subscriber will be installed
/// on the first [`set_level`] call that raises the level.
///
/// The level filter is reloadable via [`set_level`].
pub fn init(log_dir: &Path, filter: LevelFilter) -> Result<(), InitError> {
    LOG_DIR
        .set(log_dir.to_path_buf())
        .map_err(|_| InitError::AlreadyInitialised)?;
    if filter != LevelFilter::OFF {
        install_subscriber(log_dir, filter)?;
    }
    Ok(())
}

/// Apply a new level filter to the live subscriber, installing
/// the subscriber on first activation if necessary.
///
/// Three exhaustive cases:
///
/// * Subscriber already installed → flip the filter via the
///   reload handle.
/// * No subscriber and `filter == LevelFilter::OFF` → no-op.
/// * No subscriber and `filter != LevelFilter::OFF` → install
///   the subscriber for the first time, which prepares the log
///   directory and truncates `rtop.log`.
///
/// Panics if [`init`] was not called first; the in-tree callers
/// all run after `main` has called `init`.
pub fn set_level(filter: LevelFilter) -> Result<(), SetLevelError> {
    if let Some(handle) = RELOAD_HANDLE.get() {
        handle
            .modify(|f| *f = filter)
            .expect("reload handle should accept a LevelFilter");
        tracing::info!(
            subsystem = %Subsystem::Logger,
            level = %filter,
            "log level changed",
        );
        return Ok(());
    }
    if filter == LevelFilter::OFF {
        return Ok(());
    }
    let log_dir = LOG_DIR
        .get()
        .expect("log::set_level called before log::init");
    install_subscriber(log_dir, filter)?;
    Ok(())
}

/// Read the OS version via `RtlGetVersion` and format as `MAJOR.MINOR.BUILD`
/// (e.g. `10.0.26100`). The build number maps unambiguously to a marketing
/// label via Microsoft's release-health table.
fn windows_version() -> String {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info.dwOSVersionInfoSize` is set to the struct size above.
    // RtlGetVersion writes only into the caller-provided struct and never
    // fails for a valid struct size.
    unsafe {
        let _ = RtlGetVersion(&mut info);
    }
    format!(
        "{}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}

/// Emit the startup diagnostic banner — a single INFO event with all
/// the context a maintainer needs from a bug report: rtop version,
/// build profile, target triple, Windows version, host, user, config
/// path, log path, log level.
///
/// The banner is INFO so it appears only when `log_level >= info`.
/// Bug reports should reproduce with `log_level = "info"` to capture
/// it.
pub fn startup_banner(level: LevelFilter, log_file: &Path, config_file: Option<&Path>) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let config_display = config_file
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "default".into());
    tracing::info!(
        subsystem = %Subsystem::Startup,
        rtop = env!("CARGO_PKG_VERSION"),
        profile,
        windows = %windows_version(),
        host = %crate::tools::hostname(),
        user = %crate::tools::username(),
        config = %config_display,
        log = %log_file.display(),
        log_level = %level,
        "rtop starting",
    );
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

    #[test]
    fn subsystem_display_uses_lowercase_snake_case() {
        let cases: &[(Subsystem, &str)] = &[
            (Subsystem::Cpu, "cpu"),
            (Subsystem::CpuThermal, "cpu_thermal"),
            (Subsystem::Memory, "memory"),
            (Subsystem::Disk, "disk"),
            (Subsystem::Network, "network"),
            (Subsystem::Process, "process"),
            (Subsystem::GpuNvml, "gpu_nvml"),
            (Subsystem::GpuNvapi, "gpu_nvapi"),
            (Subsystem::GpuIgcl, "gpu_igcl"),
            (Subsystem::GpuAdl, "gpu_adl"),
            (Subsystem::Gpu, "gpu"),
            (Subsystem::Theme, "theme"),
            (Subsystem::Config, "config"),
            (Subsystem::Terminal, "terminal"),
            (Subsystem::Logger, "logger"),
            (Subsystem::Startup, "startup"),
            (Subsystem::Ui, "ui"),
            (Subsystem::Input, "input"),
            (Subsystem::Runner, "runner"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.to_string(), *expected);
        }
        assert_eq!(cases.len(), 19);
    }

    #[test]
    fn hex_formats_with_uppercase_padding() {
        assert_eq!(Hex(0xC5_u32).to_string(), "0x000000C5");
        assert_eq!(Hex(0_i32).to_string(), "0x00000000");
        assert_eq!(Hex(0xDEAD_BEEF_u32).to_string(), "0xDEADBEEF");
        assert_eq!(Hex(-1_i32).to_string(), "0xFFFFFFFF");
    }

    #[test]
    fn local_time_formatter_writes_expected_shape() {
        use regex::Regex;
        use tracing_subscriber::fmt::time::FormatTime;

        let timer = LocalTime;
        let mut buf = String::new();
        let mut writer = tracing_subscriber::fmt::format::Writer::new(&mut buf);
        timer
            .format_time(&mut writer)
            .expect("formatter must write");

        let pattern = Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$")
            .expect("regex must compile");
        assert!(pattern.is_match(&buf), "got: {buf}");
    }

    #[test]
    fn windows_version_returns_dotted_form() {
        use regex::Regex;
        let v = windows_version();
        let pattern = Regex::new(r"^\d+\.\d+\.\d+$").expect("regex must compile");
        assert!(pattern.is_match(&v), "got: {v}");
    }
}
