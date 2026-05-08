//! Per-subsystem persisted configuration sections.
//!
//! `Config` was originally a 50+-field flat struct; renderers and
//! handlers reached into it for a few unrelated fields each, and
//! adding a per-widget setting touched the same struct as adding a
//! log-level setting.
//!
//! The new shape splits the persisted state into per-subsystem
//! sub-structs (`UiConfig`, `RefreshConfig`, `CpuConfig`, …) that
//! `Config` composes via `#[serde(flatten)]`. The on-disk TOML
//! format is unchanged — every field still serialises at the
//! top level — but the in-memory layout matches the natural
//! dependency boundaries:
//!
//! * Widget renderers can take `&CpuConfig` (or `&NetConfig`, etc.)
//!   directly, eliminating the per-widget `*WidgetSettings`
//!   adapter layer.
//! * Config consumers that only care about refresh intervals don't
//!   pull in CPU/disk/network field knowledge.
//! * Adding a new option is a one-section edit, not a 50-field
//!   struct edit.
//!
//! Top-level fields (`layout`, `hidden_widgets`, `conf_file`) stay
//! on `Config` directly because they don't fit a single subsystem
//! (layout selection, runtime view filter, file path).

use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::{CpuGraphSource, GraphSymbol, TempScale};
use serde::{Deserialize, Serialize};
use tracing_subscriber::filter::LevelFilter;

use super::MAX_GPUS;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Reset `field` to `default` and emit a warning when its current
/// value isn't in `choices`. Used by [`super::Config::validate`] to
/// surface invalid loaded values without aborting the whole load.
pub(super) fn validate_choice(
    field: &mut String,
    default: &str,
    choices: &[&str],
    name: &str,
    warnings: &mut Vec<String>,
) {
    if !choices.contains(&field.as_str()) {
        warnings.push(format!(
            "Invalid value for '{name}': '{}' (expected one of: {})",
            field,
            choices.join(", ")
        ));
        *field = default.to_string();
    }
}

// ---------------------------------------------------------------------------
// Per-subsystem sections
// ---------------------------------------------------------------------------

/// UI / general appearance settings (`general` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme_background: bool,
    pub rounded_corners: bool,
    pub color_theme: String,
    pub vim_keys: bool,
    pub terminal_sync: bool,
    pub background_update: bool,
    pub base_10_sizes: bool,
    /// Default graph drawing style; per-widget `graph_symbol_*`
    /// fields override this when set to a non-`Default` value.
    pub graph_symbol: GraphSymbol,
    /// Format string for the clock displayed in the CPU widget
    /// header. Empty hides the clock.
    pub clock_format: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme_background: true,
            rounded_corners: true,
            color_theme: "default".to_string(),
            vim_keys: false,
            terminal_sync: true,
            background_update: true,
            base_10_sizes: false,
            graph_symbol: GraphSymbol::Braille,
            clock_format: "%X".to_string(),
        }
    }
}

/// Refresh-interval settings (one global + per-subsystem
/// overrides). 0 on a per-subsystem field means "inherit the
/// global `update_ms`".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshConfig {
    pub update_ms: i64,
    pub cpu_update_ms: i64,
    pub mem_update_ms: i64,
    pub disk_update_ms: i64,
    pub net_update_ms: i64,
    pub gpu_update_ms: i64,
    pub proc_update_ms: i64,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            update_ms: 2000,
            cpu_update_ms: 0,
            mem_update_ms: 0,
            disk_update_ms: 0,
            net_update_ms: 0,
            gpu_update_ms: 0,
            proc_update_ms: 0,
        }
    }
}

/// CPU widget settings (`cpu` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuConfig {
    pub graph_symbol_cpu: GraphSymbol,
    pub cpu_graph_upper: CpuGraphSource,
    pub cpu_graph_lower: CpuGraphSource,
    pub cpu_invert_lower: bool,
    pub cpu_single_graph: bool,
    pub cpu_auto_scale: bool,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub temp_scale: TempScale,
    pub show_cpu_freq: bool,
    pub custom_cpu_name: String,
    pub show_uptime: bool,
    pub show_cpu_watts: bool,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            graph_symbol_cpu: GraphSymbol::Default,
            cpu_graph_upper: CpuGraphSource::User,
            cpu_graph_lower: CpuGraphSource::System,
            cpu_invert_lower: true,
            cpu_single_graph: false,
            cpu_auto_scale: false,
            check_temp: true,
            show_coretemp: true,
            temp_scale: TempScale::Celsius,
            show_cpu_freq: true,
            custom_cpu_name: String::new(),
            show_uptime: true,
            show_cpu_watts: true,
        }
    }
}

/// Memory widget settings (`mem` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemConfig {
    pub show_swap: bool,
}

impl Default for MemConfig {
    fn default() -> Self {
        Self { show_swap: true }
    }
}

/// Network widget settings (`net` options-menu tab).
///
/// Runtime-mutable network bits (`net_auto`, `net_sync`,
/// `net_iface`) live on [`ViewConfig`] / `AppState::view`
/// ([`crate::app::RuntimeView`]) — they're toggled outside the
/// options menu (`a`, `s`, Tab keys) and the architecture treats
/// them as session view-state that mirrors back to the persisted
/// form on save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetConfig {
    pub graph_symbol_net: GraphSymbol,
    pub swap_upload_download: bool,
    pub net_download: i64,
    pub net_upload: i64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            graph_symbol_net: GraphSymbol::Default,
            swap_upload_download: false,
            net_download: 100,
            net_upload: 100,
        }
    }
}

/// Process widget settings (`proc` options-menu tab).
///
/// Runtime-mutable process bits (`proc_tree`, `proc_reversed`,
/// `proc_per_core`, `proc_sorting`, `proc_filter`) live on
/// [`ViewConfig`] / `AppState::view` ([`crate::app::RuntimeView`])
/// — they're toggled outside the options menu (`e`, `r`, `c`,
/// Left/Right, `f`/`/` keys) and the architecture treats them as
/// session view-state that mirrors back to the persisted form on
/// save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcConfig {
    pub proc_aggregate: bool,
    pub proc_colors: bool,
    pub proc_gradient: bool,
    pub proc_mem_bytes: bool,
    pub keep_dead_proc_usage: bool,
}

impl Default for ProcConfig {
    fn default() -> Self {
        Self {
            proc_aggregate: false,
            proc_colors: true,
            proc_gradient: true,
            proc_mem_bytes: true,
            keep_dead_proc_usage: false,
        }
    }
}

/// GPU widget settings (`gpu` options-menu tab).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuConfig {
    pub custom_gpu_names: [String; MAX_GPUS],
}

/// Disk widget settings (`disk` options-menu tab).
///
/// Runtime-mutable disk bits (`io_mode`) live on [`ViewConfig`] /
/// `AppState::view` ([`crate::app::RuntimeView`]) — `i` toggles it
/// outside the options menu and the architecture treats it as
/// session view-state that mirrors back to the persisted form on
/// save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskConfig {
    pub graph_symbol_disk: GraphSymbol,
    pub show_io_stat: bool,
    pub io_graph_combined: bool,
    pub disk_io_mode: bool,
    pub disks_filter: Vec<String>,
}

impl Default for DiskConfig {
    fn default() -> Self {
        Self {
            graph_symbol_disk: GraphSymbol::Default,
            show_io_stat: true,
            io_graph_combined: false,
            disk_io_mode: false,
            disks_filter: Vec::new(),
        }
    }
}

/// View-state config: fields the user toggles outside the
/// options menu (process tree mode, sort, filter, IO mode,
/// network auto-scale / sync / interface).
///
/// These fields persist across restarts (so toggle gestures
/// survive the next launch) but their *runtime* mutation goes
/// through [`crate::app::RuntimeView`] in `AppState`, not
/// directly through `Config`. Sync points:
///
/// 1. `AppState::new` initialises `RuntimeView` from
///    `Config::view`.
/// 2. Opening the options menu copies `RuntimeView ->
///    Config::view` so the menu shows the current values.
/// 3. Committing an options-menu edit copies
///    `Config::view -> RuntimeView` so the runtime picks up the
///    user's change.
/// 4. Process exit copies `RuntimeView -> Config::view`
///    before serialising so the on-disk form is current.
///
/// Handler runtime toggles (`e`, `r`, `c`, Left/Right, `i`, `a`,
/// `s`, Tab, `f`/`/`) mutate `RuntimeView` only — they never
/// reach `&mut Config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    pub proc_tree: bool,
    pub proc_reversed: bool,
    pub proc_per_core: bool,
    pub proc_sorting: ProcSort,
    pub proc_filter: String,
    pub io_mode: bool,
    pub net_auto: bool,
    pub net_sync: bool,
    /// Preferred network interface name. `"auto"` picks the first
    /// available interface; any other value is the user's pinned
    /// selection (cycled at runtime via `Tab` / `Shift+Tab`).
    pub net_iface: String,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            proc_tree: false,
            proc_reversed: false,
            proc_per_core: false,
            proc_sorting: ProcSort::Cpu,
            proc_filter: String::new(),
            io_mode: false,
            net_auto: true,
            net_sync: false,
            net_iface: "auto".to_string(),
        }
    }
}

/// Logging config — process-wide tracing level. Lives in its own
/// section purely so the `config_schema!` macro can address it
/// with the same `section.field` shape it uses for every other
/// key; logical owner is the logging subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// Process-wide tracing log level. Serialised as the canonical
    /// lowercase name (`off`/`error`/`warn`/`info`/`debug`/`trace`).
    #[serde(
        with = "crate::log::serde_filter",
        default = "crate::log::default_filter"
    )]
    pub log_level: LevelFilter,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            log_level: LevelFilter::WARN,
        }
    }
}
