use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::{CpuGraphSource, GraphSymbol, TempScale};
use crate::domain::widget_kind::{WidgetKind, WidgetList};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing_subscriber::filter::LevelFilter;

/// Maximum number of GPUs supported.
pub const MAX_GPUS: usize = 8;

/// Error returned by `ConfigKey::set_string` when the supplied
/// string cannot be parsed into the field's value type.
///
/// The single in-tree caller (`menu::options_menu::cycle_browsable`)
/// always passes a value drawn from `browsable_values()`, so this
/// error is unreachable at runtime and the caller `.expect()`s it.
/// Carrying the key name and offending value makes the panic
/// message informative if the contract is ever violated.
#[derive(Debug, Error)]
#[error("invalid value '{value}' for config key '{key}'")]
pub struct SetStringError {
    pub key: &'static str,
    pub value: String,
}

/// The kind of a config key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Bool,
    Int,
    String,
    /// A field backed by a typed enum with a closed set of
    /// canonical names. Treated by the menu exactly like
    /// `String` with `browsable_values`, but the underlying
    /// Config field is the enum type itself.
    Enum,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn clamp_warn(field: &mut i64, min: i64, max: i64, name: &str, warnings: &mut Vec<String>) {
    let old = *field;
    *field = old.clamp(min, max);
    if old != *field {
        warnings.push(format!(
            "Value for '{name}' out of range ({old}), clamped to {}",
            *field
        ));
    }
}

fn validate_choice(
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

/// The full set of widgets a freshly-defaulted `Config` lists in
/// `custom_widgets`: the five base widgets plus a `Gpu(N)`
/// entry for every supported index. The layout engine drops Gpu
/// entries whose index is `>= detected gpu_count` so the list can
/// safely include all of them — widgets only render when both
/// listed and backed by hardware.
fn default_custom_widgets() -> Vec<WidgetKind> {
    let mut widgets = vec![
        WidgetKind::Cpu,
        WidgetKind::Mem,
        WidgetKind::Net,
        WidgetKind::Proc,
        WidgetKind::Disk,
    ];
    widgets.extend((0..MAX_GPUS).filter_map(WidgetKind::gpu));
    widgets
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// All configuration state for rtop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // -- bools --
    pub theme_background: bool,
    pub rounded_corners: bool,
    pub proc_reversed: bool,
    pub proc_tree: bool,
    pub proc_colors: bool,
    pub proc_gradient: bool,
    pub proc_per_core: bool,
    pub proc_mem_bytes: bool,

    pub proc_aggregate: bool,
    pub keep_dead_proc_usage: bool,
    pub cpu_invert_lower: bool,
    pub cpu_single_graph: bool,
    pub cpu_auto_scale: bool,
    pub show_uptime: bool,
    pub show_cpu_watts: bool,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub show_cpu_freq: bool,
    pub show_swap: bool,

    pub show_io_stat: bool,
    pub io_mode: bool,
    pub io_graph_combined: bool,
    pub swap_upload_download: bool,
    pub base_10_sizes: bool,
    pub net_auto: bool,
    pub net_sync: bool,

    pub vim_keys: bool,
    pub background_update: bool,
    pub terminal_sync: bool,
    pub save_config_on_exit: bool,

    pub disk_io_mode: bool,

    // -- ints --
    pub update_ms: i64,
    pub cpu_update_ms: i64,
    pub mem_update_ms: i64,
    pub disk_update_ms: i64,
    pub net_update_ms: i64,
    pub gpu_update_ms: i64,
    pub proc_update_ms: i64,
    pub net_download: i64,
    pub net_upload: i64,

    pub current_preset: i64,

    // -- custom preset (the only layout state persisted to TOML) --
    /// Widget list for the user's custom preset (preset index
    /// `BUILTIN_PRESETS.len()`). Only this and the three
    /// `custom_*` bools below describe layout in TOML.
    pub custom_widgets: WidgetList,
    pub custom_cpu_bottom: bool,
    pub custom_mem_below_net: bool,
    pub custom_proc_left: bool,

    // -- strings --
    pub color_theme: String,
    pub graph_symbol: GraphSymbol,
    pub graph_symbol_cpu: GraphSymbol,
    pub graph_symbol_net: GraphSymbol,
    pub graph_symbol_disk: GraphSymbol,
    pub proc_sorting: ProcSort,
    pub cpu_graph_upper: CpuGraphSource,
    pub cpu_graph_lower: CpuGraphSource,

    pub temp_scale: TempScale,
    pub clock_format: String,
    pub custom_cpu_name: String,
    pub disks_filter: Vec<String>,
    pub net_iface: String,
    #[serde(
        with = "crate::log::serde_filter",
        default = "crate::log::default_filter"
    )]
    pub log_level: LevelFilter,
    pub proc_filter: String,
    pub custom_gpu_names: [String; MAX_GPUS],

    #[serde(skip)]
    conf_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // bools
            theme_background: true,
            rounded_corners: true,
            proc_reversed: false,
            proc_tree: false,
            proc_colors: true,
            proc_gradient: true,
            proc_per_core: false,
            proc_mem_bytes: true,

            proc_aggregate: false,
            keep_dead_proc_usage: false,
            cpu_invert_lower: true,
            cpu_single_graph: false,
            cpu_auto_scale: false,
            show_uptime: true,
            show_cpu_watts: true,
            check_temp: true,
            show_coretemp: true,
            show_cpu_freq: true,
            show_swap: true,

            show_io_stat: true,
            io_mode: false,
            io_graph_combined: false,
            swap_upload_download: false,
            base_10_sizes: false,
            net_auto: true,
            net_sync: false,

            vim_keys: false,
            background_update: true,
            terminal_sync: true,
            save_config_on_exit: true,

            disk_io_mode: false,

            // ints
            update_ms: 2000,
            cpu_update_ms: 0,
            mem_update_ms: 0,
            disk_update_ms: 0,
            net_update_ms: 0,
            gpu_update_ms: 0,
            proc_update_ms: 0,
            net_download: 100,
            net_upload: 100,

            // Default to the custom preset so first-launch users
            // see what their TOML actually says (the `custom_*`
            // fields below). Otherwise the cursor would lie.
            current_preset: crate::domain::preset::BUILTIN_PRESETS.len() as i64,

            // Custom preset default: every widget rtop knows about.
            // `Gpu(N)` entries for indices not backed by physical
            // hardware are silently ignored by the layout engine
            // (no slot allocated when `N >= detected gpu_count`),
            // so the list can include them all without producing
            // empty widgets. New GPUs plugged in later are picked
            // up automatically because their index is already in
            // the list.
            custom_widgets: WidgetList::from_kinds(default_custom_widgets()),
            custom_cpu_bottom: false,
            custom_mem_below_net: false,
            custom_proc_left: false,

            // strings
            color_theme: "default".to_string(),
            graph_symbol: GraphSymbol::Braille,
            graph_symbol_cpu: GraphSymbol::Default,
            graph_symbol_net: GraphSymbol::Default,
            graph_symbol_disk: GraphSymbol::Default,
            proc_sorting: ProcSort::CpuLazy,
            cpu_graph_upper: CpuGraphSource::User,
            cpu_graph_lower: CpuGraphSource::System,

            temp_scale: TempScale::Celsius,
            clock_format: "%X".to_string(),
            custom_cpu_name: String::new(),
            disks_filter: Vec::new(),
            net_iface: "auto".to_string(),
            log_level: LevelFilter::WARN,
            proc_filter: String::new(),
            custom_gpu_names: Default::default(),

            conf_file: None,
        }
    }
}

impl Config {
    /// Create a new Config with all default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the effective update interval for a per-widget field.
    ///
    /// If `widget_ms` is 0 (the default), returns the global `update_ms`.
    /// Otherwise returns `widget_ms` clamped to [100, 86_400_000].
    pub fn effective_interval(&self, widget_ms: i64) -> u64 {
        if widget_ms > 0 {
            (widget_ms.clamp(100, 86_400_000)) as u64
        } else {
            self.update_ms.max(100) as u64
        }
    }

    /// Load config from a TOML file. Returns warnings for invalid values.
    pub fn load(&mut self, path: &Path) -> Vec<String> {
        let mut warnings = Vec::new();
        self.conf_file = Some(path.to_path_buf());

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return warnings,
        };

        if content.trim().is_empty() {
            return warnings;
        }

        // Preserve runtime-only fields across the load.
        let saved_conf = self.conf_file.take();

        match toml::from_str::<Config>(&content) {
            Ok(loaded) => {
                *self = loaded;
            }
            Err(e) => {
                warnings.push(format!("Failed to parse config: {e}"));
                // Restore runtime fields and return early.
                self.conf_file = saved_conf;
                return warnings;
            }
        }

        self.conf_file = saved_conf;

        warnings.append(&mut self.validate());
        warnings
    }

    /// Write config to a TOML file.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let content =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    /// Validate all config values, clamping ints and resetting invalid strings.
    pub fn validate(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Clamp integers
        clamp_warn(
            &mut self.update_ms,
            100,
            86_400_000,
            "update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.cpu_update_ms,
            0,
            86_400_000,
            "cpu_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.mem_update_ms,
            0,
            86_400_000,
            "mem_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.disk_update_ms,
            0,
            86_400_000,
            "disk_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_update_ms,
            0,
            86_400_000,
            "net_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.gpu_update_ms,
            0,
            86_400_000,
            "gpu_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.proc_update_ms,
            0,
            86_400_000,
            "proc_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_download,
            0,
            10_000_000,
            "net_download",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_upload,
            0,
            10_000_000,
            "net_upload",
            &mut warnings,
        );

        self.clamp_current_preset();

        // The only remaining `validate_choice` call covers
        // `color_theme` — the single string-typed field with a
        // closed set of choices. The other browsable config
        // fields are typed enums whose validity is enforced by
        // serde at deserialise time.
        validate_choice(
            &mut self.color_theme,
            "default",
            crate::theme::THEME_NAMES,
            "color_theme",
            &mut warnings,
        );

        // Surface deserialise-time parse failures from the custom
        // preset's widget list. Bad strings are already dropped by
        // WidgetList's deserialiser; we just need to fold the
        // captured invalid entries into the warning list.
        let invalid_widgets = self.custom_widgets.take_invalid();
        if !invalid_widgets.is_empty() {
            warnings.push(format!(
                "Invalid widget name(s) in 'custom_widgets': {}",
                invalid_widgets.join(", ")
            ));
        }

        // Validate disks_filter: surface invalid drive entries as
        // warnings and drop them in place so the saved config
        // matches what the runtime actually uses.
        let filter = crate::domain::disk::DisksFilter::parse(&self.disks_filter);
        if !filter.invalid().is_empty() {
            warnings.push(format!(
                "Invalid drive entry/entries in 'disks_filter': {}",
                filter.invalid().join(", ")
            ));
            let invalid_tokens = filter.invalid().to_vec();
            self.disks_filter
                .retain(|tok| !invalid_tokens.contains(tok));
        }

        warnings
    }

    /// Reload config from the stored config file path.
    pub fn reload(&mut self) -> Vec<String> {
        if let Some(path) = self.conf_file.clone() {
            self.apply_defaults();
            self.load(&path)
        } else {
            Vec::new()
        }
    }

    /// Reset to defaults, preserving runtime-only fields.
    pub fn apply_defaults(&mut self) {
        let saved_conf = self.conf_file.take();
        *self = Self::default();
        self.conf_file = saved_conf;
    }

    /// Clamp `current_preset` to a valid index in
    /// `0..=BUILTIN_PRESETS.len()` (inclusive — the upper bound is
    /// the custom preset). Out-of-range values fall back to the
    /// custom preset so the user's saved layout is what they see
    /// rather than a builtin they didn't pick.
    pub fn clamp_current_preset(&mut self) {
        let custom_idx = crate::domain::preset::BUILTIN_PRESETS.len() as i64;
        if self.current_preset < 0 || self.current_preset > custom_idx {
            self.current_preset = custom_idx;
        }
    }

    /// Live layout: the widget list shown on screen, dispatched from
    /// whichever preset is currently active. Builtins return their
    /// `&'static [WidgetKind]` directly; the custom preset returns a
    /// borrow of `custom_widgets`.
    pub fn widgets(&self) -> &[WidgetKind] {
        match self.active_builtin() {
            Some(b) => b.widgets,
            None => self.custom_widgets.as_slice(),
        }
    }

    /// Live `cpu_bottom` for the active preset.
    pub fn cpu_bottom(&self) -> bool {
        match self.active_builtin() {
            Some(b) => b.cpu_bottom,
            None => self.custom_cpu_bottom,
        }
    }

    /// Live `mem_below_net` for the active preset.
    pub fn mem_below_net(&self) -> bool {
        match self.active_builtin() {
            Some(b) => b.mem_below_net,
            None => self.custom_mem_below_net,
        }
    }

    /// Live `proc_left` for the active preset.
    pub fn proc_left(&self) -> bool {
        match self.active_builtin() {
            Some(b) => b.proc_left,
            None => self.custom_proc_left,
        }
    }

    /// Move the preset cursor by `delta` (wrapping over the full
    /// `0..=BUILTIN_PRESETS.len()` range, where the last index is
    /// the custom preset).
    pub fn cycle_preset(&mut self, delta: i64) {
        let count = (crate::domain::preset::BUILTIN_PRESETS.len() + 1) as i64;
        self.current_preset = (self.current_preset + delta).rem_euclid(count);
    }

    /// Toggle a single widget on or off in the custom preset.
    /// Auto-promotes from a builtin preset to the custom one
    /// (copying the active builtin's layout into custom first) so
    /// the user's edit always lands somewhere persistent.
    pub fn toggle_widget(&mut self, kind: WidgetKind) {
        self.promote_to_custom_if_builtin();
        if !self.custom_widgets.remove_kind(kind) {
            self.custom_widgets.push(kind);
        }
    }

    /// Replace the custom preset's widget list with `kinds`.
    ///
    /// If the cursor is on a builtin preset whose layout already
    /// matches `kinds`, this is a no-op so the cursor stays on the
    /// builtin (no surprise promote-to-custom on an unchanged value).
    /// Otherwise it promotes-to-custom and writes the new list.
    pub fn set_custom_widgets(&mut self, kinds: Vec<WidgetKind>) {
        if self.widgets() == kinds.as_slice() {
            return;
        }
        self.promote_to_custom_if_builtin();
        self.custom_widgets = WidgetList::from_kinds(kinds);
    }

    /// Set `custom_cpu_bottom` (or first promote from a builtin).
    /// Used by the options-menu Bool toggle path.
    pub fn set_cpu_bottom(&mut self, value: bool) {
        self.promote_to_custom_if_builtin();
        self.custom_cpu_bottom = value;
    }

    /// Set `custom_mem_below_net` (or first promote from a builtin).
    pub fn set_mem_below_net(&mut self, value: bool) {
        self.promote_to_custom_if_builtin();
        self.custom_mem_below_net = value;
    }

    /// Set `custom_proc_left` (or first promote from a builtin).
    pub fn set_proc_left(&mut self, value: bool) {
        self.promote_to_custom_if_builtin();
        self.custom_proc_left = value;
    }

    /// Borrow the active builtin preset, or `None` when the
    /// current cursor points to the custom preset.
    fn active_builtin(&self) -> Option<&'static crate::domain::preset::Preset> {
        let presets = crate::domain::preset::BUILTIN_PRESETS;
        if self.current_preset < 0 {
            return None;
        }
        let idx = self.current_preset as usize;
        if idx < presets.len() {
            Some(&presets[idx])
        } else {
            None
        }
    }

    /// If the cursor is on a builtin, copy that builtin's layout
    /// into the custom_* fields and switch the cursor to custom.
    /// Called from every layout-mutating setter so that the user's
    /// edit always lands in the persistent custom preset.
    fn promote_to_custom_if_builtin(&mut self) {
        if let Some(b) = self.active_builtin() {
            self.custom_widgets = WidgetList::from_kinds(b.widgets.iter().copied());
            self.custom_cpu_bottom = b.cpu_bottom;
            self.custom_mem_below_net = b.mem_below_net;
            self.custom_proc_left = b.proc_left;
            self.current_preset = crate::domain::preset::BUILTIN_PRESETS.len() as i64;
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigKey — flat enum with one variant per config field
// ---------------------------------------------------------------------------

/// A flat enum identifying every config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    // -- bool variants --
    ThemeBackground,
    RoundedCorners,
    ProcReversed,
    ProcTree,
    ProcColors,
    ProcGradient,
    ProcPerCore,
    ProcMemBytes,
    ProcLeft,

    ProcAggregate,
    KeepDeadProcUsage,
    CpuInvertLower,
    CpuSingleGraph,
    CpuAutoScale,
    CpuBottom,
    ShowUptime,
    ShowCpuWatts,
    CheckTemp,
    ShowCoretemp,
    ShowCpuFreq,
    MemBelowNet,
    ShowSwap,

    ShowIoStat,
    IoMode,
    IoGraphCombined,
    SwapUploadDownload,
    Base10Sizes,
    NetAuto,
    NetSync,

    VimKeys,
    BackgroundUpdate,
    TerminalSync,
    SaveConfigOnExit,

    DiskIoMode,

    // -- int variants --
    UpdateMs,
    CpuUpdateMs,
    MemUpdateMs,
    DiskUpdateMs,
    NetUpdateMs,
    GpuUpdateMs,
    ProcUpdateMs,
    NetDownload,
    NetUpload,

    // -- string variants --
    ColorTheme,
    Widgets,
    GraphSymbol,
    GraphSymbolCpu,
    GraphSymbolNet,
    GraphSymbolDisk,
    ProcSorting,
    CpuGraphUpper,
    CpuGraphLower,

    TempScale,
    ClockFormat,
    CustomCpuName,
    DisksFilter,
    LogLevel,
    ProcFilter,
    CustomGpuName0,
    CustomGpuName1,
    CustomGpuName2,
    CustomGpuName3,
    CustomGpuName4,
    CustomGpuName5,
    CustomGpuName6,
    CustomGpuName7,
}

impl ConfigKey {
    /// Returns the TOML field name (snake_case).
    pub fn name(self) -> &'static str {
        match self {
            Self::ThemeBackground => "theme_background",
            Self::RoundedCorners => "rounded_corners",
            Self::ProcReversed => "proc_reversed",
            Self::ProcTree => "proc_tree",
            Self::ProcColors => "proc_colors",
            Self::ProcGradient => "proc_gradient",
            Self::ProcPerCore => "proc_per_core",
            Self::ProcMemBytes => "proc_mem_bytes",
            Self::ProcLeft => "proc_left",

            Self::ProcAggregate => "proc_aggregate",
            Self::KeepDeadProcUsage => "keep_dead_proc_usage",
            Self::CpuInvertLower => "cpu_invert_lower",
            Self::CpuSingleGraph => "cpu_single_graph",
            Self::CpuAutoScale => "cpu_auto_scale",
            Self::CpuBottom => "cpu_bottom",
            Self::ShowUptime => "show_uptime",
            Self::ShowCpuWatts => "show_cpu_watts",
            Self::CheckTemp => "check_temp",
            Self::ShowCoretemp => "show_coretemp",
            Self::ShowCpuFreq => "show_cpu_freq",
            Self::MemBelowNet => "mem_below_net",
            Self::ShowSwap => "show_swap",

            Self::ShowIoStat => "show_io_stat",
            Self::IoMode => "io_mode",
            Self::IoGraphCombined => "io_graph_combined",
            Self::SwapUploadDownload => "swap_upload_download",
            Self::Base10Sizes => "base_10_sizes",
            Self::NetAuto => "net_auto",
            Self::NetSync => "net_sync",

            Self::VimKeys => "vim_keys",
            Self::BackgroundUpdate => "background_update",
            Self::TerminalSync => "terminal_sync",
            Self::SaveConfigOnExit => "save_config_on_exit",

            Self::DiskIoMode => "disk_io_mode",

            Self::UpdateMs => "update_ms",
            Self::CpuUpdateMs => "cpu_update_ms",
            Self::MemUpdateMs => "mem_update_ms",
            Self::DiskUpdateMs => "disk_update_ms",
            Self::NetUpdateMs => "net_update_ms",
            Self::GpuUpdateMs => "gpu_update_ms",
            Self::ProcUpdateMs => "proc_update_ms",
            Self::NetDownload => "net_download",
            Self::NetUpload => "net_upload",

            Self::ColorTheme => "color_theme",
            Self::Widgets => "widgets",
            Self::GraphSymbol => "graph_symbol",
            Self::GraphSymbolCpu => "graph_symbol_cpu",
            Self::GraphSymbolNet => "graph_symbol_net",
            Self::GraphSymbolDisk => "graph_symbol_disk",
            Self::ProcSorting => "proc_sorting",
            Self::CpuGraphUpper => "cpu_graph_upper",
            Self::CpuGraphLower => "cpu_graph_lower",

            Self::TempScale => "temp_scale",
            Self::ClockFormat => "clock_format",
            Self::CustomCpuName => "custom_cpu_name",
            Self::DisksFilter => "disks_filter",
            Self::LogLevel => "log_level",
            Self::ProcFilter => "proc_filter",
            Self::CustomGpuName0 => "custom_gpu_name0",
            Self::CustomGpuName1 => "custom_gpu_name1",
            Self::CustomGpuName2 => "custom_gpu_name2",
            Self::CustomGpuName3 => "custom_gpu_name3",
            Self::CustomGpuName4 => "custom_gpu_name4",
            Self::CustomGpuName5 => "custom_gpu_name5",
            Self::CustomGpuName6 => "custom_gpu_name6",
            Self::CustomGpuName7 => "custom_gpu_name7",
        }
    }

    /// Returns the kind of value this key holds.
    pub fn kind(self) -> KeyKind {
        match self {
            Self::ThemeBackground
            | Self::RoundedCorners
            | Self::ProcReversed
            | Self::ProcTree
            | Self::ProcColors
            | Self::ProcGradient
            | Self::ProcPerCore
            | Self::ProcMemBytes
            | Self::ProcLeft
            | Self::ProcAggregate
            | Self::KeepDeadProcUsage
            | Self::CpuInvertLower
            | Self::CpuSingleGraph
            | Self::CpuAutoScale
            | Self::CpuBottom
            | Self::ShowUptime
            | Self::ShowCpuWatts
            | Self::CheckTemp
            | Self::ShowCoretemp
            | Self::ShowCpuFreq
            | Self::MemBelowNet
            | Self::ShowSwap
            | Self::ShowIoStat
            | Self::IoMode
            | Self::IoGraphCombined
            | Self::SwapUploadDownload
            | Self::Base10Sizes
            | Self::NetAuto
            | Self::NetSync
            | Self::VimKeys
            | Self::BackgroundUpdate
            | Self::TerminalSync
            | Self::SaveConfigOnExit
            | Self::DiskIoMode => KeyKind::Bool,

            Self::UpdateMs
            | Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs
            | Self::NetDownload
            | Self::NetUpload => KeyKind::Int,

            Self::ColorTheme
            | Self::Widgets
            | Self::ClockFormat
            | Self::CustomCpuName
            | Self::DisksFilter
            | Self::ProcFilter
            | Self::CustomGpuName0
            | Self::CustomGpuName1
            | Self::CustomGpuName2
            | Self::CustomGpuName3
            | Self::CustomGpuName4
            | Self::CustomGpuName5
            | Self::CustomGpuName6
            | Self::CustomGpuName7 => KeyKind::String,

            Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolDisk
            | Self::ProcSorting
            | Self::CpuGraphUpper
            | Self::CpuGraphLower
            | Self::TempScale
            | Self::LogLevel => KeyKind::Enum,
        }
    }

    /// Parse a TOML field name into a ConfigKey.
    #[cfg(test)]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "theme_background" => Some(Self::ThemeBackground),
            "rounded_corners" => Some(Self::RoundedCorners),
            "proc_reversed" => Some(Self::ProcReversed),
            "proc_tree" => Some(Self::ProcTree),
            "proc_colors" => Some(Self::ProcColors),
            "proc_gradient" => Some(Self::ProcGradient),
            "proc_per_core" => Some(Self::ProcPerCore),
            "proc_mem_bytes" => Some(Self::ProcMemBytes),
            "proc_left" => Some(Self::ProcLeft),

            "proc_aggregate" => Some(Self::ProcAggregate),
            "keep_dead_proc_usage" => Some(Self::KeepDeadProcUsage),
            "cpu_invert_lower" => Some(Self::CpuInvertLower),
            "cpu_single_graph" => Some(Self::CpuSingleGraph),
            "cpu_auto_scale" => Some(Self::CpuAutoScale),
            "cpu_bottom" => Some(Self::CpuBottom),
            "show_uptime" => Some(Self::ShowUptime),
            "show_cpu_watts" => Some(Self::ShowCpuWatts),
            "check_temp" => Some(Self::CheckTemp),
            "show_coretemp" => Some(Self::ShowCoretemp),
            "show_cpu_freq" => Some(Self::ShowCpuFreq),
            "mem_below_net" => Some(Self::MemBelowNet),
            "show_swap" => Some(Self::ShowSwap),

            "show_io_stat" => Some(Self::ShowIoStat),
            "io_mode" => Some(Self::IoMode),
            "io_graph_combined" => Some(Self::IoGraphCombined),
            "swap_upload_download" => Some(Self::SwapUploadDownload),
            "base_10_sizes" => Some(Self::Base10Sizes),
            "net_auto" => Some(Self::NetAuto),
            "net_sync" => Some(Self::NetSync),

            "vim_keys" => Some(Self::VimKeys),
            "background_update" => Some(Self::BackgroundUpdate),
            "terminal_sync" => Some(Self::TerminalSync),
            "save_config_on_exit" => Some(Self::SaveConfigOnExit),

            "disk_io_mode" => Some(Self::DiskIoMode),

            "update_ms" => Some(Self::UpdateMs),
            "cpu_update_ms" => Some(Self::CpuUpdateMs),
            "mem_update_ms" => Some(Self::MemUpdateMs),
            "disk_update_ms" => Some(Self::DiskUpdateMs),
            "net_update_ms" => Some(Self::NetUpdateMs),
            "gpu_update_ms" => Some(Self::GpuUpdateMs),
            "proc_update_ms" => Some(Self::ProcUpdateMs),
            "net_download" => Some(Self::NetDownload),
            "net_upload" => Some(Self::NetUpload),

            "color_theme" => Some(Self::ColorTheme),
            "widgets" => Some(Self::Widgets),
            "graph_symbol" => Some(Self::GraphSymbol),
            "graph_symbol_cpu" => Some(Self::GraphSymbolCpu),
            "graph_symbol_net" => Some(Self::GraphSymbolNet),
            "graph_symbol_disk" => Some(Self::GraphSymbolDisk),
            "proc_sorting" => Some(Self::ProcSorting),
            "cpu_graph_upper" => Some(Self::CpuGraphUpper),
            "cpu_graph_lower" => Some(Self::CpuGraphLower),

            "temp_scale" => Some(Self::TempScale),
            "clock_format" => Some(Self::ClockFormat),
            "custom_cpu_name" => Some(Self::CustomCpuName),
            "disks_filter" => Some(Self::DisksFilter),
            "log_level" => Some(Self::LogLevel),
            "proc_filter" => Some(Self::ProcFilter),
            "custom_gpu_name0" => Some(Self::CustomGpuName0),
            "custom_gpu_name1" => Some(Self::CustomGpuName1),
            "custom_gpu_name2" => Some(Self::CustomGpuName2),
            "custom_gpu_name3" => Some(Self::CustomGpuName3),
            "custom_gpu_name4" => Some(Self::CustomGpuName4),
            "custom_gpu_name5" => Some(Self::CustomGpuName5),
            "custom_gpu_name6" => Some(Self::CustomGpuName6),
            "custom_gpu_name7" => Some(Self::CustomGpuName7),

            _ => None,
        }
    }

    /// Returns a display string for the value of this key in the given config.
    pub fn get_display(self, config: &Config) -> String {
        match self {
            // bools
            Self::ThemeBackground => bool_display(config.theme_background),
            Self::RoundedCorners => bool_display(config.rounded_corners),
            Self::ProcReversed => bool_display(config.proc_reversed),
            Self::ProcTree => bool_display(config.proc_tree),
            Self::ProcColors => bool_display(config.proc_colors),
            Self::ProcGradient => bool_display(config.proc_gradient),
            Self::ProcPerCore => bool_display(config.proc_per_core),
            Self::ProcMemBytes => bool_display(config.proc_mem_bytes),
            Self::ProcLeft => bool_display(config.proc_left()),

            Self::ProcAggregate => bool_display(config.proc_aggregate),
            Self::KeepDeadProcUsage => bool_display(config.keep_dead_proc_usage),
            Self::CpuInvertLower => bool_display(config.cpu_invert_lower),
            Self::CpuSingleGraph => bool_display(config.cpu_single_graph),
            Self::CpuAutoScale => bool_display(config.cpu_auto_scale),
            Self::CpuBottom => bool_display(config.cpu_bottom()),
            Self::ShowUptime => bool_display(config.show_uptime),
            Self::ShowCpuWatts => bool_display(config.show_cpu_watts),
            Self::CheckTemp => bool_display(config.check_temp),
            Self::ShowCoretemp => bool_display(config.show_coretemp),
            Self::ShowCpuFreq => bool_display(config.show_cpu_freq),
            Self::MemBelowNet => bool_display(config.mem_below_net()),
            Self::ShowSwap => bool_display(config.show_swap),

            Self::ShowIoStat => bool_display(config.show_io_stat),
            Self::IoMode => bool_display(config.io_mode),
            Self::IoGraphCombined => bool_display(config.io_graph_combined),
            Self::SwapUploadDownload => bool_display(config.swap_upload_download),
            Self::Base10Sizes => bool_display(config.base_10_sizes),
            Self::NetAuto => bool_display(config.net_auto),
            Self::NetSync => bool_display(config.net_sync),

            Self::VimKeys => bool_display(config.vim_keys),
            Self::BackgroundUpdate => bool_display(config.background_update),
            Self::TerminalSync => bool_display(config.terminal_sync),
            Self::SaveConfigOnExit => bool_display(config.save_config_on_exit),

            Self::DiskIoMode => bool_display(config.disk_io_mode),

            // ints
            Self::UpdateMs => config.update_ms.to_string(),
            Self::CpuUpdateMs => config.cpu_update_ms.to_string(),
            Self::MemUpdateMs => config.mem_update_ms.to_string(),
            Self::DiskUpdateMs => config.disk_update_ms.to_string(),
            Self::NetUpdateMs => config.net_update_ms.to_string(),
            Self::GpuUpdateMs => config.gpu_update_ms.to_string(),
            Self::ProcUpdateMs => config.proc_update_ms.to_string(),
            Self::NetDownload => config.net_download.to_string(),
            Self::NetUpload => config.net_upload.to_string(),

            // strings
            Self::ColorTheme => config.color_theme.clone(),
            Self::Widgets => config
                .widgets()
                .iter()
                .map(WidgetKind::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            Self::GraphSymbol => config.graph_symbol.to_string(),
            Self::GraphSymbolCpu => config.graph_symbol_cpu.to_string(),
            Self::GraphSymbolNet => config.graph_symbol_net.to_string(),
            Self::GraphSymbolDisk => config.graph_symbol_disk.to_string(),
            Self::ProcSorting => config.proc_sorting.to_string(),
            Self::CpuGraphUpper => config.cpu_graph_upper.to_string(),
            Self::CpuGraphLower => config.cpu_graph_lower.to_string(),

            Self::TempScale => config.temp_scale.to_string(),
            Self::ClockFormat => config.clock_format.clone(),
            Self::CustomCpuName => config.custom_cpu_name.clone(),
            Self::DisksFilter => config.disks_filter.join(" "),
            Self::LogLevel => config.log_level.to_string(),
            Self::ProcFilter => config.proc_filter.clone(),
            Self::CustomGpuName0 => config.custom_gpu_names[0].clone(),
            Self::CustomGpuName1 => config.custom_gpu_names[1].clone(),
            Self::CustomGpuName2 => config.custom_gpu_names[2].clone(),
            Self::CustomGpuName3 => config.custom_gpu_names[3].clone(),
            Self::CustomGpuName4 => config.custom_gpu_names[4].clone(),
            Self::CustomGpuName5 => config.custom_gpu_names[5].clone(),
            Self::CustomGpuName6 => config.custom_gpu_names[6].clone(),
            Self::CustomGpuName7 => config.custom_gpu_names[7].clone(),
        }
    }

    /// Toggle the boolean field. Panics on non-bool keys.
    pub fn toggle_bool(self, config: &mut Config) {
        match self {
            Self::ThemeBackground => config.theme_background = !config.theme_background,
            Self::RoundedCorners => config.rounded_corners = !config.rounded_corners,
            Self::ProcReversed => config.proc_reversed = !config.proc_reversed,
            Self::ProcTree => config.proc_tree = !config.proc_tree,
            Self::ProcColors => config.proc_colors = !config.proc_colors,
            Self::ProcGradient => config.proc_gradient = !config.proc_gradient,
            Self::ProcPerCore => config.proc_per_core = !config.proc_per_core,
            Self::ProcMemBytes => config.proc_mem_bytes = !config.proc_mem_bytes,
            Self::ProcLeft => config.set_proc_left(!config.proc_left()),

            Self::ProcAggregate => config.proc_aggregate = !config.proc_aggregate,
            Self::KeepDeadProcUsage => config.keep_dead_proc_usage = !config.keep_dead_proc_usage,
            Self::CpuInvertLower => config.cpu_invert_lower = !config.cpu_invert_lower,
            Self::CpuSingleGraph => config.cpu_single_graph = !config.cpu_single_graph,
            Self::CpuAutoScale => config.cpu_auto_scale = !config.cpu_auto_scale,
            Self::CpuBottom => config.set_cpu_bottom(!config.cpu_bottom()),
            Self::ShowUptime => config.show_uptime = !config.show_uptime,
            Self::ShowCpuWatts => config.show_cpu_watts = !config.show_cpu_watts,
            Self::CheckTemp => config.check_temp = !config.check_temp,
            Self::ShowCoretemp => config.show_coretemp = !config.show_coretemp,
            Self::ShowCpuFreq => config.show_cpu_freq = !config.show_cpu_freq,
            Self::MemBelowNet => config.set_mem_below_net(!config.mem_below_net()),
            Self::ShowSwap => config.show_swap = !config.show_swap,

            Self::ShowIoStat => config.show_io_stat = !config.show_io_stat,
            Self::IoMode => config.io_mode = !config.io_mode,
            Self::IoGraphCombined => config.io_graph_combined = !config.io_graph_combined,
            Self::SwapUploadDownload => {
                config.swap_upload_download = !config.swap_upload_download;
            }
            Self::Base10Sizes => config.base_10_sizes = !config.base_10_sizes,
            Self::NetAuto => config.net_auto = !config.net_auto,
            Self::NetSync => config.net_sync = !config.net_sync,

            Self::VimKeys => config.vim_keys = !config.vim_keys,
            Self::BackgroundUpdate => config.background_update = !config.background_update,
            Self::TerminalSync => config.terminal_sync = !config.terminal_sync,
            Self::SaveConfigOnExit => config.save_config_on_exit = !config.save_config_on_exit,

            Self::DiskIoMode => config.disk_io_mode = !config.disk_io_mode,
            _ => panic!("toggle_bool called on non-bool key '{}'", self.name()),
        }
    }

    /// Get an integer value. Panics on non-int keys.
    pub fn get_int(self, config: &Config) -> i64 {
        match self {
            Self::UpdateMs => config.update_ms,
            Self::CpuUpdateMs => config.cpu_update_ms,
            Self::MemUpdateMs => config.mem_update_ms,
            Self::DiskUpdateMs => config.disk_update_ms,
            Self::NetUpdateMs => config.net_update_ms,
            Self::GpuUpdateMs => config.gpu_update_ms,
            Self::ProcUpdateMs => config.proc_update_ms,
            Self::NetDownload => config.net_download,
            Self::NetUpload => config.net_upload,
            _ => panic!("get_int called on non-int key '{}'", self.name()),
        }
    }

    /// Set an integer value. No clamping — caller should call validate().
    /// Panics on non-int keys.
    pub fn set_int(self, config: &mut Config, value: i64) {
        match self {
            Self::UpdateMs => config.update_ms = value,
            Self::CpuUpdateMs => config.cpu_update_ms = value,
            Self::MemUpdateMs => config.mem_update_ms = value,
            Self::DiskUpdateMs => config.disk_update_ms = value,
            Self::NetUpdateMs => config.net_update_ms = value,
            Self::GpuUpdateMs => config.gpu_update_ms = value,
            Self::ProcUpdateMs => config.proc_update_ms = value,
            Self::NetDownload => config.net_download = value,
            Self::NetUpload => config.net_upload = value,
            _ => panic!("set_int called on non-int key '{}'", self.name()),
        }
    }

    /// Set a value from its canonical string form.
    ///
    /// Returns `Err(SetStringError)` if `value` does not parse for
    /// the field's type. Used both by `cycle_browsable` (which
    /// passes a value drawn from `browsable_values()`, so failure
    /// is a contract violation) and by the inline-edit commit path
    /// (which validates first via [`Self::validate_string`] so the
    /// contract is also already satisfied).
    pub fn set_string(self, config: &mut Config, value: &str) -> Result<(), SetStringError> {
        let err = || SetStringError {
            key: self.name(),
            value: value.to_string(),
        };
        match self {
            Self::ColorTheme => config.color_theme = value.to_string(),
            Self::Widgets => {
                let kinds = Self::parse_widgets(value).map_err(|_| err())?;
                config.set_custom_widgets(kinds);
            }
            Self::GraphSymbol => config.graph_symbol = value.parse().map_err(|_| err())?,
            Self::GraphSymbolCpu => config.graph_symbol_cpu = value.parse().map_err(|_| err())?,
            Self::GraphSymbolNet => config.graph_symbol_net = value.parse().map_err(|_| err())?,
            Self::GraphSymbolDisk => config.graph_symbol_disk = value.parse().map_err(|_| err())?,
            Self::ProcSorting => config.proc_sorting = value.parse().map_err(|_| err())?,
            Self::CpuGraphUpper => config.cpu_graph_upper = value.parse().map_err(|_| err())?,
            Self::CpuGraphLower => config.cpu_graph_lower = value.parse().map_err(|_| err())?,

            Self::TempScale => config.temp_scale = value.parse().map_err(|_| err())?,
            Self::ClockFormat => config.clock_format = value.to_string(),
            Self::CustomCpuName => config.custom_cpu_name = value.to_string(),
            Self::DisksFilter => {
                config.disks_filter = Self::parse_disks_filter(value).map_err(|_| err())?;
            }
            Self::LogLevel => config.log_level = value.parse().map_err(|_| err())?,
            Self::ProcFilter => config.proc_filter = value.to_string(),
            Self::CustomGpuName0 => config.custom_gpu_names[0] = value.to_string(),
            Self::CustomGpuName1 => config.custom_gpu_names[1] = value.to_string(),
            Self::CustomGpuName2 => config.custom_gpu_names[2] = value.to_string(),
            Self::CustomGpuName3 => config.custom_gpu_names[3] = value.to_string(),
            Self::CustomGpuName4 => config.custom_gpu_names[4] = value.to_string(),
            Self::CustomGpuName5 => config.custom_gpu_names[5] = value.to_string(),
            Self::CustomGpuName6 => config.custom_gpu_names[6] = value.to_string(),
            Self::CustomGpuName7 => config.custom_gpu_names[7] = value.to_string(),
            // Bool and Int keys go through `set_bool` / `set_int`.
            // If the contract is violated we return an error
            // identifying the offending key.
            _ => return Err(err()),
        }
        Ok(())
    }

    /// Parse `value` as a whitespace-separated list of widget names.
    ///
    /// Used by both [`Self::set_string`] and [`Self::validate_string`]
    /// for the [`Self::Widgets`] key. Returns a static error message
    /// suitable for inline display in the options menu.
    pub fn parse_widgets(value: &str) -> Result<Vec<WidgetKind>, &'static str> {
        let kinds: Result<Vec<_>, _> = value
            .split_whitespace()
            .map(str::parse::<WidgetKind>)
            .collect();
        let kinds = kinds.map_err(|_| "invalid widget name")?;
        if kinds.is_empty() {
            return Err("at least one widget required");
        }
        Ok(kinds)
    }

    /// Parse `value` as a whitespace-separated list of drive filter
    /// entries (e.g. `"C: !D:"`).
    ///
    /// An empty list is allowed (matches every disk). Each entry
    /// must be a single ASCII letter followed by `:`, optionally
    /// prefixed with `!`. Returns the original token list (so case
    /// and `!` prefixes are preserved verbatim for round-trip
    /// fidelity) — normalisation happens at match time.
    pub fn parse_disks_filter(value: &str) -> Result<Vec<String>, &'static str> {
        let tokens: Vec<String> = value.split_whitespace().map(str::to_string).collect();
        let parsed = crate::domain::disk::DisksFilter::parse(&tokens);
        if !parsed.invalid().is_empty() {
            return Err("drive entries must be like 'C:' or '!D:'");
        }
        Ok(tokens)
    }

    /// Parse `value` as an integer for this int-typed key, enforcing
    /// the same bounds that [`Config::validate`] would clamp to.
    ///
    /// Calling this on a non-int key returns an error rather than
    /// panicking so the inline editor can degrade gracefully.
    pub fn parse_int(self, value: &str) -> Result<i64, &'static str> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("must be an integer");
        }
        let n: i64 = trimmed.parse().map_err(|_| "must be an integer")?;
        let in_range = match self {
            Self::UpdateMs => (100..=86_400_000).contains(&n),
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => (0..=86_400_000).contains(&n),
            Self::NetDownload | Self::NetUpload => (0..=10_000_000).contains(&n),
            _ => return Err("not an integer key"),
        };
        if in_range {
            Ok(n)
        } else {
            Err(self.int_bounds_message())
        }
    }

    fn int_bounds_message(self) -> &'static str {
        match self {
            Self::UpdateMs => "must be 100..86400000 ms",
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => "must be 0..86400000 ms (0=inherit)",
            Self::NetDownload | Self::NetUpload => "must be 0..10000000 KiB/s",
            _ => "out of range",
        }
    }

    /// Validate that `value` is acceptable for this string-typed key
    /// without mutating any config field. Returns a static error
    /// message suitable for inline display in the options menu when
    /// validation fails.
    ///
    /// Used by the inline editor commit path: validate first, and
    /// only call [`Self::set_string`] when validation succeeds.
    pub fn validate_string(self, value: &str) -> Result<(), &'static str> {
        match self {
            Self::Widgets => Self::parse_widgets(value).map(|_| ()),
            Self::DisksFilter => Self::parse_disks_filter(value).map(|_| ()),
            Self::ClockFormat
            | Self::CustomCpuName
            | Self::ProcFilter
            | Self::CustomGpuName0
            | Self::CustomGpuName1
            | Self::CustomGpuName2
            | Self::CustomGpuName3
            | Self::CustomGpuName4
            | Self::CustomGpuName5
            | Self::CustomGpuName6
            | Self::CustomGpuName7 => Ok(()),
            _ => match self.kind() {
                KeyKind::String | KeyKind::Enum => {
                    if self.browsable_values().contains(&value) {
                        Ok(())
                    } else {
                        Err("invalid value")
                    }
                }
                _ => Err("not a string key"),
            },
        }
    }

    /// Returns the allowed values for constrained string keys, or None for free-form.
    pub fn choice_values(self) -> Option<&'static [&'static str]> {
        match self {
            Self::ColorTheme => Some(crate::theme::THEME_NAMES),
            Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolDisk => Some(GraphSymbol::NAMES),
            Self::CpuGraphUpper | Self::CpuGraphLower => Some(CpuGraphSource::NAMES),
            Self::TempScale => Some(TempScale::NAMES),
            Self::ProcSorting => Some(ProcSort::NAMES),
            Self::LogLevel => Some(crate::log::FILTER_NAMES),
            _ => None,
        }
    }

    /// Returns the canonical values cycled by the options menu for
    /// browsable keys, or `&[]` for free-form keys.
    pub fn browsable_values(self) -> &'static [&'static str] {
        self.choice_values().unwrap_or(&[])
    }
}

fn bool_display(v: bool) -> String {
    if v {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_file_uses_defaults() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_empty.toml");
        fs::write(&tmp, "").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.update_ms, 2000);
        assert_eq!(config.color_theme, "default");
        assert!(config.theme_background);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_valid_config_parses_all_types() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_valid.toml");
        fs::write(
            &tmp,
            "color_theme = \"dracula\"\ntheme_background = false\nupdate_ms = 500\n",
        )
        .unwrap();

        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.color_theme, "dracula");
        assert!(!config.theme_background);
        assert_eq!(config.update_ms, 500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_preserves_comments() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_comments.toml");
        fs::write(&tmp, "# this is a comment\nupdate_ms = 1000\n").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.update_ms, 1000);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_unknown_keys_are_ignored() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_unknown.toml");
        fs::write(&tmp, "nonexistent_key = \"value\"\n").unwrap();
        let warnings = config.load(&tmp);
        // serde with #[serde(default)] silently ignores unknown keys
        assert!(warnings.is_empty());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_toml_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_badtoml.toml");
        fs::write(&tmp, "this is not valid toml {{{\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("parse"));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_out_of_range_int_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_out_of_range.toml");
        fs::write(&tmp, "update_ms = 50\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("out of range"));
        assert_eq!(config.update_ms, 100);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_typed_value_aborts_whole_parse() {
        // An invalid value for a typed enum field causes serde to
        // fail the whole `toml::from_str::<Config>` and the loader
        // returns a single "Failed to parse config" warning; the
        // entire config falls back to defaults.
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_typed_value.toml");
        fs::write(&tmp, "graph_symbol = \"ascii\"\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Failed to parse config"));
        assert!(warnings[0].contains("graph_symbol"));
        // Defaults preserved.
        assert_eq!(config.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.color_theme, "default");
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_string_choice_resets_just_that_field() {
        // String-typed fields validated by `validate_choice` reset
        // the offending field and emit a per-field warning while
        // the rest of the config loads normally. `custom_widgets`
        // validation also strips invalid widget names with a warning.
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_string_values.toml");
        fs::write(
            &tmp,
            "color_theme = \"foo\"\ncustom_widgets = [\"cpu\", \"nope\"]\n",
        )
        .unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("color_theme") && w.contains("foo"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("custom_widgets") && w.contains("nope"))
        );
        assert_eq!(config.color_theme, "default");
        // After dropping invalid "nope", only "cpu" remains in custom.
        assert_eq!(config.custom_widgets.as_slice(), &[WidgetKind::Cpu]);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn write_roundtrip_preserves_values() {
        let mut config = Config::new();
        config.color_theme = "nord".to_string();
        config.vim_keys = true;
        config.update_ms = 1500;

        let tmp = std::env::temp_dir().join("rtop_test_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut config2 = Config::new();
        config2.load(&tmp);
        assert_eq!(config2.color_theme, "nord");
        assert!(config2.vim_keys);
        assert_eq!(config2.update_ms, 1500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn defaults_are_correct() {
        let config = Config::new();
        assert!(config.theme_background);
        assert!(!config.vim_keys);
        assert_eq!(config.update_ms, 2000);
        assert_eq!(config.color_theme, "default");
        assert_eq!(config.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.proc_sorting, ProcSort::CpuLazy);
        // First-launch cursor lands on the custom preset so the
        // visible layout matches what's persisted in TOML.
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        // On the custom cursor, the live layout view is the
        // custom widget list verbatim. Specific contents are covered
        // by `default_custom_widgets_includes_all_supported_widgets`.
        assert_eq!(config.widgets(), config.custom_widgets.as_slice());
        assert!(!config.cpu_bottom());
        assert!(!config.mem_below_net());
        assert!(!config.proc_left());
    }

    #[test]
    fn validate_clamps_ints() {
        let mut config = Config::new();
        config.update_ms = 50;
        config.net_download = -1;
        let warnings = config.validate();
        assert_eq!(config.update_ms, 100);
        assert_eq!(config.net_download, 0);
        assert!(warnings.len() >= 2);
    }

    #[test]
    fn validate_keeps_valid_disks_filter_unchanged() {
        let mut config = Config::new();
        config.disks_filter = vec!["C:".into(), "!D:".into()];
        let warnings = config.validate();
        assert!(
            !warnings.iter().any(|w| w.contains("disks_filter")),
            "valid disks_filter should not warn, got: {warnings:?}"
        );
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn validate_warns_and_strips_invalid_disks_filter_entries() {
        let mut config = Config::new();
        config.disks_filter = vec![
            "C:".into(),
            "abc".into(),
            "D:".into(),
            "3".into(),
            "!!".into(),
        ];
        let warnings = config.validate();
        let disks_warning = warnings
            .iter()
            .find(|w| w.contains("disks_filter"))
            .expect("validate must surface a disks_filter warning");
        assert!(disks_warning.contains("abc"));
        assert!(disks_warning.contains('3'));
        assert!(disks_warning.contains("!!"));
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "D:".to_string()]
        );
    }

    #[test]
    fn validate_empty_disks_filter_does_not_warn() {
        let mut config = Config::new();
        config.disks_filter = Vec::new();
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("disks_filter")));
        assert!(config.disks_filter.is_empty());
    }

    #[test]
    fn toggle_widget_adds_when_missing() {
        let mut config = Config::new();
        // Start from a controlled custom layout.
        config.custom_widgets = WidgetList::from_kinds([WidgetKind::Cpu, WidgetKind::Mem]);
        config.toggle_widget(WidgetKind::Net);
        assert!(config.widgets().contains(&WidgetKind::Net));
        assert!(config.custom_widgets.as_slice().contains(&WidgetKind::Net));
    }

    #[test]
    fn toggle_widget_removes_when_present() {
        let mut config = Config::new();
        config.custom_widgets =
            WidgetList::from_kinds([WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Net]);
        config.toggle_widget(WidgetKind::Net);
        assert!(!config.widgets().contains(&WidgetKind::Net));
        assert!(!config.custom_widgets.as_slice().contains(&WidgetKind::Net));
    }

    #[test]
    fn toggle_widget_on_builtin_promotes_to_custom() {
        let mut config = Config::new();
        // Move to a builtin (preset 0 = "all") explicitly.
        config.current_preset = 0;
        // Pre-conditions: live layout matches builtin "all".
        assert_eq!(
            config.widgets(),
            &[
                WidgetKind::Cpu,
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk,
            ]
        );

        config.toggle_widget(WidgetKind::Cpu);

        // Cursor switched to custom and custom captured the new
        // (builtin minus cpu) layout.
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        assert_eq!(
            config.custom_widgets.as_slice(),
            &[
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk
            ]
        );
        assert_eq!(
            config.widgets(),
            &[
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk
            ]
        );
    }

    #[test]
    fn toggle_widget_on_custom_does_not_change_cursor() {
        let mut config = Config::new();
        // Default cursor is custom.
        let before = config.current_preset;
        config.toggle_widget(WidgetKind::Cpu);
        assert_eq!(config.current_preset, before);
    }

    #[test]
    fn current_preset_default_is_custom() {
        let config = Config::new();
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
    }

    #[test]
    fn cycle_preset_wraps_modulo_total_count() {
        let mut config = Config::new();
        let total = (crate::domain::preset::BUILTIN_PRESETS.len() + 1) as i64;
        config.current_preset = 0;
        for _ in 0..total {
            config.cycle_preset(1);
        }
        // After one full lap forward we are back where we started.
        assert_eq!(config.current_preset, 0);
        config.cycle_preset(-1);
        assert_eq!(config.current_preset, total - 1);
    }

    #[test]
    fn cycle_preset_to_builtin_dispatches_to_builtin_layout() {
        let mut config = Config::new();
        config.custom_widgets = WidgetList::from_kinds([WidgetKind::Mem, WidgetKind::Proc]);
        config.custom_cpu_bottom = true;
        config.current_preset = crate::domain::preset::BUILTIN_PRESETS.len() as i64;
        // On custom, live = custom.
        assert_eq!(config.widgets(), &[WidgetKind::Mem, WidgetKind::Proc]);
        assert!(config.cpu_bottom());

        // Cycle to builtin 0 (all). Live now reads from the builtin.
        config.current_preset = 0;
        assert_eq!(
            config.widgets(),
            &[
                WidgetKind::Cpu,
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk,
            ]
        );
        assert!(!config.cpu_bottom());
        // Custom storage is untouched by cycling.
        assert_eq!(
            config.custom_widgets.as_slice(),
            &[WidgetKind::Mem, WidgetKind::Proc]
        );
        assert!(config.custom_cpu_bottom);
    }

    #[test]
    fn cycle_preset_back_to_custom_restores_user_layout() {
        let mut config = Config::new();
        config.custom_widgets =
            WidgetList::from_kinds([WidgetKind::Mem, WidgetKind::Proc, WidgetKind::Gpu(0)]);
        config.current_preset = crate::domain::preset::BUILTIN_PRESETS.len() as i64;

        // Visit a builtin and come back.
        config.cycle_preset(1);
        config.cycle_preset(-1);
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        assert_eq!(
            config.widgets(),
            &[WidgetKind::Mem, WidgetKind::Proc, WidgetKind::Gpu(0)]
        );
    }

    #[test]
    fn set_cpu_bottom_on_builtin_promotes_to_custom() {
        let mut config = Config::new();
        config.current_preset = 0;

        config.set_cpu_bottom(true);

        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        assert!(config.custom_cpu_bottom);
        assert!(config.cpu_bottom());
    }

    #[test]
    fn set_proc_left_on_custom_just_mutates_custom() {
        let mut config = Config::new();
        // Default is custom.
        let before = config.current_preset;
        config.set_proc_left(true);
        assert_eq!(config.current_preset, before);
        assert!(config.custom_proc_left);
        assert!(config.proc_left());
    }

    #[test]
    fn default_custom_widgets_includes_all_supported_widgets() {
        let config = Config::new();
        let kinds = config.custom_widgets.as_slice();
        // 5 base widgets + every supported GPU index.
        assert_eq!(kinds.len(), 5 + MAX_GPUS);
        assert!(kinds.contains(&WidgetKind::Cpu));
        assert!(kinds.contains(&WidgetKind::Mem));
        assert!(kinds.contains(&WidgetKind::Net));
        assert!(kinds.contains(&WidgetKind::Proc));
        assert!(kinds.contains(&WidgetKind::Disk));
        for i in 0..MAX_GPUS {
            let gpu = WidgetKind::gpu(i).expect("0..MAX_GPUS is in range");
            assert!(kinds.contains(&gpu), "default custom must list {gpu}");
        }
    }

    #[test]
    fn clamp_current_preset_keeps_in_range_value() {
        let mut config = Config::new();
        config.current_preset = 1;
        config.clamp_current_preset();
        assert_eq!(config.current_preset, 1);
    }

    #[test]
    fn clamp_current_preset_resets_negative_to_custom() {
        let mut config = Config::new();
        config.current_preset = -3;
        config.clamp_current_preset();
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
    }

    #[test]
    fn clamp_current_preset_resets_overflow_to_custom() {
        let mut config = Config::new();
        config.current_preset = (crate::domain::preset::BUILTIN_PRESETS.len() + 99) as i64;
        config.clamp_current_preset();
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
    }

    #[test]
    fn config_round_trips_custom_layout_through_toml() {
        let mut config = Config::new();
        config.custom_widgets = WidgetList::from_kinds([WidgetKind::Cpu, WidgetKind::Mem]);
        config.custom_cpu_bottom = true;
        let tmp = std::env::temp_dir().join("rtop_test_layout_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut loaded = Config::new();
        loaded.load(&tmp);
        // Cursor on custom (default), so live should match custom.
        assert_eq!(loaded.widgets(), &[WidgetKind::Cpu, WidgetKind::Mem]);
        assert!(loaded.cpu_bottom());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn config_key_parse_returns_none_for_unknown() {
        assert_eq!(ConfigKey::parse("nonexistent"), None);
    }

    #[test]
    fn config_key_roundtrip_through_parser() {
        let keys = [
            ConfigKey::ColorTheme,
            ConfigKey::ProcSorting,
            ConfigKey::ThemeBackground,
            ConfigKey::UpdateMs,
            ConfigKey::NetDownload,
        ];

        for key in keys {
            assert_eq!(ConfigKey::parse(key.name()), Some(key));
        }
    }

    // -----------------------------------------------------------------
    // Inline editor parser/validator/set_custom_widgets coverage
    // -----------------------------------------------------------------

    #[test]
    fn parse_widgets_rejects_empty_input() {
        assert!(ConfigKey::parse_widgets("").is_err());
        assert!(ConfigKey::parse_widgets("   ").is_err());
    }

    #[test]
    fn parse_widgets_accepts_known_kinds() {
        let result = ConfigKey::parse_widgets("cpu mem net proc disk").unwrap();
        assert_eq!(
            result,
            vec![
                WidgetKind::Cpu,
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk,
            ]
        );
    }

    #[test]
    fn parse_widgets_accepts_gpu_indices() {
        let result = ConfigKey::parse_widgets("cpu gpu0 gpu7").unwrap();
        assert_eq!(
            result,
            vec![WidgetKind::Cpu, WidgetKind::Gpu(0), WidgetKind::Gpu(7)]
        );
    }

    #[test]
    fn parse_widgets_rejects_unknown_token() {
        assert!(ConfigKey::parse_widgets("cpu nope").is_err());
        assert!(ConfigKey::parse_widgets("gpu99").is_err());
    }

    #[test]
    fn parse_disks_filter_accepts_empty() {
        assert_eq!(
            ConfigKey::parse_disks_filter("").unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            ConfigKey::parse_disks_filter("   ").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parse_disks_filter_preserves_case_and_prefix() {
        let result = ConfigKey::parse_disks_filter("c: !D: E:").unwrap();
        assert_eq!(
            result,
            vec!["c:".to_string(), "!D:".to_string(), "E:".to_string()]
        );
    }

    #[test]
    fn parse_disks_filter_rejects_bare_letter() {
        assert!(ConfigKey::parse_disks_filter("C").is_err());
        assert!(ConfigKey::parse_disks_filter("C: D").is_err());
        assert!(ConfigKey::parse_disks_filter("!E").is_err());
    }

    #[test]
    fn parse_int_enforces_update_ms_lower_bound() {
        assert!(ConfigKey::UpdateMs.parse_int("99").is_err());
        assert_eq!(ConfigKey::UpdateMs.parse_int("100").unwrap(), 100);
    }

    #[test]
    fn parse_int_accepts_zero_for_inherit_keys() {
        for key in [
            ConfigKey::CpuUpdateMs,
            ConfigKey::MemUpdateMs,
            ConfigKey::DiskUpdateMs,
            ConfigKey::NetUpdateMs,
            ConfigKey::GpuUpdateMs,
            ConfigKey::ProcUpdateMs,
        ] {
            assert_eq!(key.parse_int("0").unwrap(), 0);
        }
    }

    #[test]
    fn parse_int_rejects_negative_and_overflow() {
        assert!(ConfigKey::CpuUpdateMs.parse_int("-1").is_err());
        assert!(ConfigKey::NetDownload.parse_int("10000001").is_err());
        assert_eq!(
            ConfigKey::NetDownload.parse_int("10000000").unwrap(),
            10_000_000
        );
    }

    #[test]
    fn parse_int_trims_whitespace() {
        assert_eq!(ConfigKey::UpdateMs.parse_int("  500  ").unwrap(), 500);
    }

    #[test]
    fn parse_int_rejects_non_numeric_or_empty() {
        assert!(ConfigKey::UpdateMs.parse_int("abc").is_err());
        assert!(ConfigKey::UpdateMs.parse_int("").is_err());
    }

    #[test]
    fn parse_int_rejects_non_int_key() {
        assert!(ConfigKey::ColorTheme.parse_int("100").is_err());
        assert!(ConfigKey::Widgets.parse_int("100").is_err());
    }

    #[test]
    fn validate_string_widgets() {
        assert!(ConfigKey::Widgets.validate_string("cpu mem").is_ok());
        assert!(ConfigKey::Widgets.validate_string("").is_err());
        assert!(ConfigKey::Widgets.validate_string("nope").is_err());
    }

    #[test]
    fn validate_string_disks_filter() {
        assert!(ConfigKey::DisksFilter.validate_string("").is_ok());
        assert!(ConfigKey::DisksFilter.validate_string("C: !D:").is_ok());
        assert!(ConfigKey::DisksFilter.validate_string("X").is_err());
    }

    #[test]
    fn validate_string_free_form_keys_always_ok() {
        for key in [
            ConfigKey::ClockFormat,
            ConfigKey::CustomCpuName,
            ConfigKey::ProcFilter,
            ConfigKey::CustomGpuName0,
            ConfigKey::CustomGpuName7,
        ] {
            assert!(key.validate_string("").is_ok());
            assert!(key.validate_string("anything goes !@#$%").is_ok());
        }
    }

    #[test]
    fn validate_string_constrained_choice_keys() {
        assert!(ConfigKey::ColorTheme.validate_string("default").is_ok());
        assert!(
            ConfigKey::ColorTheme
                .validate_string("nonexistent")
                .is_err()
        );
        assert!(ConfigKey::LogLevel.validate_string("info").is_ok());
        assert!(ConfigKey::LogLevel.validate_string("loud").is_err());
    }

    #[test]
    fn validate_string_rejects_non_string_keys() {
        assert!(ConfigKey::UpdateMs.validate_string("100").is_err());
        assert!(ConfigKey::RoundedCorners.validate_string("true").is_err());
    }

    #[test]
    fn set_custom_widgets_no_op_keeps_builtin_cursor() {
        let mut config = Config::new();
        config.current_preset = 0;
        let preset_zero = config.widgets().to_vec();
        config.set_custom_widgets(preset_zero);
        assert_eq!(config.current_preset, 0, "no-op must not promote to custom");
    }

    #[test]
    fn set_custom_widgets_change_promotes_builtin_to_custom() {
        let mut config = Config::new();
        config.current_preset = 1;
        config.set_custom_widgets(vec![WidgetKind::Cpu, WidgetKind::Mem]);
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        assert_eq!(
            config.custom_widgets.as_slice(),
            &[WidgetKind::Cpu, WidgetKind::Mem]
        );
    }

    #[test]
    fn set_custom_widgets_on_custom_writes_without_changing_cursor() {
        let mut config = Config::new();
        let custom_idx = crate::domain::preset::BUILTIN_PRESETS.len() as i64;
        config.current_preset = custom_idx;
        config.set_custom_widgets(vec![WidgetKind::Cpu]);
        assert_eq!(config.current_preset, custom_idx);
        assert_eq!(config.custom_widgets.as_slice(), &[WidgetKind::Cpu]);
    }

    #[test]
    fn set_string_widgets_via_inline_editor_path() {
        let mut config = Config::new();
        config.current_preset = 1;
        ConfigKey::Widgets
            .set_string(&mut config, "cpu mem disk")
            .unwrap();
        assert_eq!(
            config.current_preset,
            crate::domain::preset::BUILTIN_PRESETS.len() as i64
        );
        assert_eq!(
            config.custom_widgets.as_slice(),
            &[WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Disk]
        );
    }

    #[test]
    fn set_string_widgets_invalid_returns_err() {
        let mut config = Config::new();
        assert!(ConfigKey::Widgets.set_string(&mut config, "nope").is_err());
        assert!(ConfigKey::Widgets.set_string(&mut config, "").is_err());
    }

    #[test]
    fn set_string_disks_filter_via_inline_editor_path() {
        let mut config = Config::new();
        ConfigKey::DisksFilter
            .set_string(&mut config, "C: !D:")
            .unwrap();
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn set_string_disks_filter_invalid_returns_err() {
        let mut config = Config::new();
        assert!(ConfigKey::DisksFilter.set_string(&mut config, "X").is_err());
    }
}
