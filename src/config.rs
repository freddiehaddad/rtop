use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::{CpuGraphSource, GraphSymbol, TempScale};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing_subscriber::filter::LevelFilter;

/// Maximum number of GPUs supported.
pub const MAX_GPUS: usize = 8;

/// Static box names (non-GPU).
const STATIC_BOX_NAMES: &[&str] = &["cpu", "mem", "net", "proc", "disk"];

/// Check if a box name is valid. Static boxes are checked by name,
/// GPU boxes are validated by the `gpuN` pattern where N is a digit.
fn is_valid_box_name(name: &str) -> bool {
    if STATIC_BOX_NAMES.contains(&name) {
        return true;
    }
    if let Some(suffix) = name.strip_prefix("gpu") {
        return suffix.len() == 1 && suffix.chars().all(|c| c.is_ascii_digit());
    }
    false
}

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
    pub proc_left: bool,

    pub proc_aggregate: bool,
    pub keep_dead_proc_usage: bool,
    pub cpu_invert_lower: bool,
    pub cpu_single_graph: bool,
    pub cpu_bottom: bool,
    pub show_uptime: bool,
    pub show_cpu_watts: bool,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub show_cpu_freq: bool,
    pub mem_below_net: bool,
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

    // -- strings --
    pub color_theme: String,
    pub shown_boxes: Vec<String>,
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
    pub disks_filter: String,
    pub net_iface: String,
    #[serde(
        with = "crate::log::serde_filter",
        default = "crate::log::default_filter"
    )]
    pub log_level: LevelFilter,
    pub proc_filter: String,
    pub presets: String,
    pub custom_gpu_names: [String; MAX_GPUS],

    // -- runtime-only (not serialized) --
    /// Internal-only: the startup layout snapshot (not persisted).
    #[serde(skip)]
    pub initial_shown_boxes: Vec<String>,
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
            proc_left: false,

            proc_aggregate: false,
            keep_dead_proc_usage: false,
            cpu_invert_lower: true,
            cpu_single_graph: false,
            cpu_bottom: false,
            show_uptime: true,
            show_cpu_watts: true,
            check_temp: true,
            show_coretemp: true,
            show_cpu_freq: true,
            mem_below_net: false,
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

            current_preset: 0,

            // strings
            color_theme: "default".to_string(),
            shown_boxes: vec![
                "cpu".into(),
                "mem".into(),
                "net".into(),
                "proc".into(),
                "disk".into(),
            ],
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
            disks_filter: String::new(),
            net_iface: "auto".to_string(),
            log_level: LevelFilter::WARN,
            proc_filter: String::new(),
            presets: "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default".to_string(),
            custom_gpu_names: Default::default(),

            // runtime-only
            initial_shown_boxes: Vec::new(),
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
        let saved_initial = std::mem::take(&mut self.initial_shown_boxes);
        let saved_conf = self.conf_file.take();

        match toml::from_str::<Config>(&content) {
            Ok(loaded) => {
                *self = loaded;
            }
            Err(e) => {
                warnings.push(format!("Failed to parse config: {e}"));
                // Restore runtime fields and return early.
                self.initial_shown_boxes = saved_initial;
                self.conf_file = saved_conf;
                return warnings;
            }
        }

        self.initial_shown_boxes = saved_initial;
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

        clamp_warn(
            &mut self.current_preset,
            0,
            i64::MAX,
            "current_preset",
            &mut warnings,
        );

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

        // Validate shown_boxes: remove invalid box names
        let invalid: Vec<String> = self
            .shown_boxes
            .iter()
            .filter(|b| !is_valid_box_name(b))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            warnings.push(format!(
                "Invalid box name(s) in 'shown_boxes': {}",
                invalid.join(", ")
            ));
            self.shown_boxes.retain(|b| is_valid_box_name(b));
        }

        // Validate disks_filter: surface invalid drive entries as
        // warnings and strip them from the stored raw string so the
        // saved config matches what the runtime actually uses.
        let filter = crate::domain::disk::DisksFilter::parse(&self.disks_filter);
        if !filter.invalid().is_empty() {
            warnings.push(format!(
                "Invalid drive entry/entries in 'disks_filter': {}",
                filter.invalid().join(", ")
            ));
            let invalid_tokens = filter.invalid();
            self.disks_filter = self
                .disks_filter
                .split_whitespace()
                .filter(|tok| !invalid_tokens.iter().any(|inv| inv == *tok))
                .collect::<Vec<_>>()
                .join(" ");
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
        let saved_initial = std::mem::take(&mut self.initial_shown_boxes);
        let saved_conf = self.conf_file.take();
        *self = Self::default();
        self.initial_shown_boxes = saved_initial;
        self.conf_file = saved_conf;
    }

    /// Parse the presets config string into a list of preset strings.
    /// Preset 0 is the startup layout stored in `initial_shown_boxes`.
    pub fn preset_list(&self) -> Vec<String> {
        let source = if self.initial_shown_boxes.is_empty() {
            &self.shown_boxes
        } else {
            &self.initial_shown_boxes
        };
        let preset0 = source
            .iter()
            .map(|b| format!("{b}:0:default"))
            .collect::<Vec<_>>()
            .join(",");
        let mut list = vec![preset0];

        if !self.presets.is_empty() {
            for preset in self.presets.split_whitespace() {
                if !preset.is_empty() {
                    list.push(preset.to_string());
                }
            }
        }
        list
    }

    /// Save the current layout as a new preset and return its index.
    pub fn save_preset(&mut self) -> usize {
        let cpu_bottom = if self.cpu_bottom { "1" } else { "0" };
        let mem_below_net = if self.mem_below_net { "1" } else { "0" };
        let proc_left = if self.proc_left { "1" } else { "0" };

        let new_preset = self
            .shown_boxes
            .iter()
            .map(|box_name| {
                let pos = match box_name.as_str() {
                    "cpu" => cpu_bottom,
                    "mem" => mem_below_net,
                    "proc" => proc_left,
                    _ => "0",
                };
                format!("{box_name}:{pos}:default")
            })
            .collect::<Vec<_>>()
            .join(",");

        let updated = if self.presets.is_empty() {
            new_preset
        } else {
            format!("{} {new_preset}", self.presets)
        };
        self.presets = updated;
        let idx = self.preset_list().len() - 1;
        self.current_preset = idx as i64;
        idx
    }

    /// Delete the preset at the given index. Index 0 (the default) cannot be deleted.
    /// Returns true if a preset was deleted.
    pub fn delete_preset(&mut self, index: usize) -> bool {
        if index == 0 {
            return false;
        }
        let presets_str = self.presets.clone();
        let custom: Vec<&str> = presets_str.split_whitespace().collect();
        let custom_idx = index - 1;
        if custom_idx >= custom.len() {
            return false;
        }
        let remaining: Vec<&str> = custom
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != custom_idx)
            .map(|(_, s)| *s)
            .collect();
        self.presets = remaining.join(" ");
        let total = self.preset_list().len() as i64;
        if self.current_preset >= total {
            self.current_preset = total - 1;
        }
        true
    }

    /// Apply a preset string like "cpu:0:default,mem:0:default,net:0:default,proc:0:default".
    /// Sets shown_boxes, cpu_bottom, mem_below_net, proc_left, and graph symbols.
    pub fn apply_preset(&mut self, preset: &str) {
        let mut boxes = Vec::new();
        for part in preset.split(',') {
            let vals: Vec<&str> = part.split(':').collect();
            if vals.len() != 3 {
                continue;
            }
            let box_name = vals[0];
            let position = vals[1];

            boxes.push(box_name.to_string());

            match box_name {
                "cpu" => self.cpu_bottom = position != "0",
                "mem" => self.mem_below_net = position != "0",
                "proc" => self.proc_left = position != "0",
                _ => {}
            }
        }
        self.shown_boxes = boxes;
    }

    /// Toggle a box's visibility in shown_boxes.
    pub fn toggle_box(&mut self, box_name: &str) -> bool {
        if !is_valid_box_name(box_name) {
            return false;
        }

        if let Some(pos) = self.shown_boxes.iter().position(|b| b == box_name) {
            self.shown_boxes.remove(pos);
        } else {
            self.shown_boxes.push(box_name.to_string());
        }

        true
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
    ShownBoxes,
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
    Presets,
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
            Self::ShownBoxes => "shown_boxes",
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
            Self::Presets => "presets",
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
            | Self::ShownBoxes
            | Self::ClockFormat
            | Self::CustomCpuName
            | Self::DisksFilter
            | Self::ProcFilter
            | Self::Presets
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
            "shown_boxes" => Some(Self::ShownBoxes),
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
            "presets" => Some(Self::Presets),
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
            Self::ProcLeft => bool_display(config.proc_left),

            Self::ProcAggregate => bool_display(config.proc_aggregate),
            Self::KeepDeadProcUsage => bool_display(config.keep_dead_proc_usage),
            Self::CpuInvertLower => bool_display(config.cpu_invert_lower),
            Self::CpuSingleGraph => bool_display(config.cpu_single_graph),
            Self::CpuBottom => bool_display(config.cpu_bottom),
            Self::ShowUptime => bool_display(config.show_uptime),
            Self::ShowCpuWatts => bool_display(config.show_cpu_watts),
            Self::CheckTemp => bool_display(config.check_temp),
            Self::ShowCoretemp => bool_display(config.show_coretemp),
            Self::ShowCpuFreq => bool_display(config.show_cpu_freq),
            Self::MemBelowNet => bool_display(config.mem_below_net),
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
            Self::ShownBoxes => config.shown_boxes.join(" "),
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
            Self::DisksFilter => config.disks_filter.clone(),
            Self::LogLevel => config.log_level.to_string(),
            Self::ProcFilter => config.proc_filter.clone(),
            Self::Presets => config.presets.clone(),
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
            Self::ProcLeft => config.proc_left = !config.proc_left,

            Self::ProcAggregate => config.proc_aggregate = !config.proc_aggregate,
            Self::KeepDeadProcUsage => config.keep_dead_proc_usage = !config.keep_dead_proc_usage,
            Self::CpuInvertLower => config.cpu_invert_lower = !config.cpu_invert_lower,
            Self::CpuSingleGraph => config.cpu_single_graph = !config.cpu_single_graph,
            Self::CpuBottom => config.cpu_bottom = !config.cpu_bottom,
            Self::ShowUptime => config.show_uptime = !config.show_uptime,
            Self::ShowCpuWatts => config.show_cpu_watts = !config.show_cpu_watts,
            Self::CheckTemp => config.check_temp = !config.check_temp,
            Self::ShowCoretemp => config.show_coretemp = !config.show_coretemp,
            Self::ShowCpuFreq => config.show_cpu_freq = !config.show_cpu_freq,
            Self::MemBelowNet => config.mem_below_net = !config.mem_below_net,
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
    /// the field's type. The single in-tree caller
    /// (`menu::options_menu::cycle_browsable`) always passes a value
    /// drawn from `browsable_values()`, so it `.expect()`s success
    /// — failure indicates a contract violation, not a runtime
    /// condition.
    pub fn set_string(self, config: &mut Config, value: &str) -> Result<(), SetStringError> {
        let err = || SetStringError {
            key: self.name(),
            value: value.to_string(),
        };
        match self {
            Self::ColorTheme => config.color_theme = value.to_string(),
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
            Self::DisksFilter => config.disks_filter = value.to_string(),
            Self::LogLevel => config.log_level = value.parse().map_err(|_| err())?,
            Self::ProcFilter => config.proc_filter = value.to_string(),
            Self::Presets => config.presets = value.to_string(),
            Self::CustomGpuName0 => config.custom_gpu_names[0] = value.to_string(),
            Self::CustomGpuName1 => config.custom_gpu_names[1] = value.to_string(),
            Self::CustomGpuName2 => config.custom_gpu_names[2] = value.to_string(),
            Self::CustomGpuName3 => config.custom_gpu_names[3] = value.to_string(),
            Self::CustomGpuName4 => config.custom_gpu_names[4] = value.to_string(),
            Self::CustomGpuName5 => config.custom_gpu_names[5] = value.to_string(),
            Self::CustomGpuName6 => config.custom_gpu_names[6] = value.to_string(),
            Self::CustomGpuName7 => config.custom_gpu_names[7] = value.to_string(),
            // ShownBoxes (Vec<String>) and any Bool/Int keys never
            // reach this function via the in-tree caller, which
            // dispatches on `kind()`. If the contract is violated
            // we return an error identifying the offending key.
            _ => return Err(err()),
        }
        Ok(())
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
        // the rest of the config loads normally. `shown_boxes`
        // validation also strips invalid box names with a warning.
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_string_values.toml");
        fs::write(
            &tmp,
            "color_theme = \"foo\"\nshown_boxes = [\"cpu\", \"nope\"]\n",
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
                .any(|w| w.contains("shown_boxes") && w.contains("nope"))
        );
        assert_eq!(config.color_theme, "default");
        // After removing invalid "nope", only "cpu" remains
        assert_eq!(config.shown_boxes, vec!["cpu"]);
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
        assert_eq!(
            config.shown_boxes,
            vec!["cpu", "mem", "net", "proc", "disk"]
        );
        assert_eq!(config.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.proc_sorting, ProcSort::CpuLazy);
        assert_eq!(config.current_preset, 0);
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
        config.disks_filter = "C: !D:".to_string();
        let warnings = config.validate();
        assert!(
            !warnings.iter().any(|w| w.contains("disks_filter")),
            "valid disks_filter should not warn, got: {warnings:?}"
        );
        assert_eq!(config.disks_filter, "C: !D:");
    }

    #[test]
    fn validate_warns_and_strips_invalid_disks_filter_entries() {
        let mut config = Config::new();
        config.disks_filter = "C: abc D: 3 !!".to_string();
        let warnings = config.validate();
        let disks_warning = warnings
            .iter()
            .find(|w| w.contains("disks_filter"))
            .expect("validate must surface a disks_filter warning");
        assert!(disks_warning.contains("abc"));
        assert!(disks_warning.contains('3'));
        assert!(disks_warning.contains("!!"));
        assert_eq!(config.disks_filter, "C: D:");
    }

    #[test]
    fn validate_empty_disks_filter_does_not_warn() {
        let mut config = Config::new();
        config.disks_filter = String::new();
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("disks_filter")));
        assert_eq!(config.disks_filter, "");
    }

    #[test]
    fn toggle_box_adds_when_missing() {
        let mut config = Config::new();
        config.shown_boxes = vec!["cpu".into(), "mem".into()];
        assert!(config.toggle_box("net"));
        assert!(config.shown_boxes.contains(&"net".to_string()));
    }

    #[test]
    fn toggle_box_removes_when_present() {
        let mut config = Config::new();
        config.shown_boxes = vec!["cpu".into(), "mem".into(), "net".into()];
        assert!(config.toggle_box("net"));
        assert!(!config.shown_boxes.contains(&"net".to_string()));
    }

    #[test]
    fn toggle_box_invalid_name_returns_false() {
        let mut config = Config::new();
        assert!(!config.toggle_box("invalid_box"));
    }

    #[test]
    fn toggle_box_accepts_any_gpu_digit() {
        let mut config = Config::new();
        assert!(config.toggle_box("gpu0"));
        assert!(config.toggle_box("gpu7"));
        assert!(config.toggle_box("gpu9"));
        assert!(!config.toggle_box("gpu10"));
        assert!(!config.toggle_box("gpuX"));
    }

    #[test]
    fn preset_list_default_has_builtin_presets() {
        let config = Config::new();
        let list = config.preset_list();
        assert_eq!(list.len(), 4);
        assert!(list[0].contains("cpu:0:default"));
    }

    #[test]
    fn preset_list_with_custom_presets() {
        let mut config = Config::new();
        config.presets = "cpu:0:default,proc:0:default cpu:1:braille,mem:0:default".to_string();
        let list = config.preset_list();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn apply_preset_sets_shown_boxes() {
        let mut config = Config::new();
        config.apply_preset("cpu:0:default,proc:1:default");
        assert_eq!(config.shown_boxes, vec!["cpu", "proc"]);
        assert!(config.proc_left);
    }

    #[test]
    fn apply_preset_cpu_bottom() {
        let mut config = Config::new();
        config.apply_preset("cpu:1:default,mem:0:default");
        assert!(config.cpu_bottom);
        assert!(!config.mem_below_net);
    }

    #[test]
    fn current_preset_default_is_zero() {
        let config = Config::new();
        assert_eq!(config.current_preset, 0);
    }

    #[test]
    fn save_preset_appends_and_sets_current() {
        let mut config = Config::new();
        config.presets = String::new();
        config.shown_boxes = vec!["cpu".into(), "proc".into()];
        config.proc_left = true;
        let idx = config.save_preset();
        assert!(idx > 0);
        assert_eq!(config.current_preset, idx as i64);
        let list = config.preset_list();
        let last = &list[idx];
        assert!(last.contains("cpu:0:default"));
        assert!(last.contains("proc:1:default"));
    }

    #[test]
    fn delete_preset_removes_custom() {
        let mut config = Config::new();
        config.presets = "cpu:0:default,proc:0:default mem:0:default,net:0:default".to_string();
        let before = config.preset_list().len();
        assert!(config.delete_preset(1));
        let after = config.preset_list().len();
        assert_eq!(after, before - 1);
    }

    #[test]
    fn delete_preset_zero_is_rejected() {
        let mut config = Config::new();
        assert!(!config.delete_preset(0));
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

    #[test]
    fn initial_shown_boxes_is_internal() {
        let mut config = Config::new();
        config.initial_shown_boxes = vec!["cpu".into(), "mem".into()];
        assert_eq!(config.initial_shown_boxes, vec!["cpu", "mem"]);
        // Should not appear in TOML output
        let output = toml::to_string_pretty(&config).unwrap();
        assert!(!output.contains("initial_shown_boxes"));
    }
}
