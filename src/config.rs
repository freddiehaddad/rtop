use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

const GRAPH_SYMBOL_VALUES: &[&str] = &["default", "braille", "block"];
const CPU_GRAPH_SOURCE_VALUES: &[&str] = &["Auto", "total", "user", "system"];
const TEMP_SCALE_VALUES: &[&str] = &["celsius", "fahrenheit", "kelvin", "rankine"];
const LOG_LEVEL_VALUES: &[&str] = &["ERROR", "WARNING", "INFO", "DEBUG", "TRACE"];

/// The kind of a config key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Bool,
    Int,
    String,
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
    pub proc_cpu_graphs: bool,
    pub proc_left: bool,

    pub proc_follow_detailed: bool,
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

    pub gpu_mirror_graph: bool,
    pub disk_io_mode: bool,

    // -- ints --
    pub update_ms: i64,
    pub net_download: i64,
    pub net_upload: i64,
    pub detailed_pid: i64,
    pub selected_pid: i64,
    pub followed_pid: i64,
    pub proc_start: i64,
    pub proc_selected: i64,
    pub current_preset: i64,

    // -- strings --
    pub color_theme: String,
    pub shown_boxes: String,
    pub graph_symbol: String,
    pub graph_symbol_cpu: String,
    pub graph_symbol_gpu: String,
    pub graph_symbol_net: String,
    pub graph_symbol_proc: String,
    pub graph_symbol_disk: String,
    pub proc_sorting: String,
    pub cpu_graph_upper: String,
    pub cpu_graph_lower: String,
    pub cpu_sensor: String,

    pub temp_scale: String,
    pub clock_format: String,
    pub custom_cpu_name: String,
    pub disks_filter: String,
    pub io_graph_speeds: String,
    pub net_iface: String,
    pub log_level: String,
    pub proc_filter: String,
    pub presets: String,
    pub custom_gpu_name0: String,
    pub custom_gpu_name1: String,
    pub custom_gpu_name2: String,
    pub custom_gpu_name3: String,
    pub custom_gpu_name4: String,
    pub custom_gpu_name5: String,

    // -- runtime-only (not serialized) --
    /// Internal-only: the startup layout snapshot (not persisted).
    #[serde(skip)]
    pub initial_shown_boxes: String,
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
            proc_cpu_graphs: true,
            proc_left: false,

            proc_follow_detailed: true,
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

            gpu_mirror_graph: true,
            disk_io_mode: false,

            // ints
            update_ms: 2000,
            net_download: 100,
            net_upload: 100,
            detailed_pid: 0,
            selected_pid: 0,
            followed_pid: 0,
            proc_start: 0,
            proc_selected: 0,
            current_preset: 0,

            // strings
            color_theme: "Default".to_string(),
            shown_boxes: "cpu mem net proc disk".to_string(),
            graph_symbol: "braille".to_string(),
            graph_symbol_cpu: "default".to_string(),
            graph_symbol_gpu: "default".to_string(),
            graph_symbol_net: "default".to_string(),
            graph_symbol_proc: "default".to_string(),
            graph_symbol_disk: "default".to_string(),
            proc_sorting: "cpu lazy".to_string(),
            cpu_graph_upper: "user".to_string(),
            cpu_graph_lower: "system".to_string(),
            cpu_sensor: "Auto".to_string(),

            temp_scale: "celsius".to_string(),
            clock_format: "%X".to_string(),
            custom_cpu_name: String::new(),
            disks_filter: String::new(),
            io_graph_speeds: String::new(),
            net_iface: String::new(),
            log_level: "WARNING".to_string(),
            proc_filter: String::new(),
            presets: "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default".to_string(),
            custom_gpu_name0: String::new(),
            custom_gpu_name1: String::new(),
            custom_gpu_name2: String::new(),
            custom_gpu_name3: String::new(),
            custom_gpu_name4: String::new(),
            custom_gpu_name5: String::new(),

            // runtime-only
            initial_shown_boxes: String::new(),
            conf_file: None,
        }
    }
}

impl Config {
    /// Create a new Config with all default values.
    pub fn new() -> Self {
        Self::default()
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
            &mut self.detailed_pid,
            i64::MIN,
            i64::MAX,
            "detailed_pid",
            &mut warnings,
        );
        clamp_warn(
            &mut self.selected_pid,
            0,
            i64::MAX,
            "selected_pid",
            &mut warnings,
        );
        clamp_warn(
            &mut self.followed_pid,
            0,
            i64::MAX,
            "followed_pid",
            &mut warnings,
        );
        clamp_warn(
            &mut self.proc_start,
            0,
            i64::MAX,
            "proc_start",
            &mut warnings,
        );
        clamp_warn(
            &mut self.proc_selected,
            0,
            i64::MAX,
            "proc_selected",
            &mut warnings,
        );
        clamp_warn(
            &mut self.current_preset,
            0,
            i64::MAX,
            "current_preset",
            &mut warnings,
        );

        // Validate choice-valued strings
        validate_choice(
            &mut self.color_theme,
            "Default",
            crate::theme::THEME_NAMES,
            "color_theme",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol,
            "braille",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol_cpu,
            "default",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol_cpu",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol_gpu,
            "default",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol_gpu",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol_net,
            "default",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol_net",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol_proc,
            "default",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol_proc",
            &mut warnings,
        );
        validate_choice(
            &mut self.graph_symbol_disk,
            "default",
            GRAPH_SYMBOL_VALUES,
            "graph_symbol_disk",
            &mut warnings,
        );
        validate_choice(
            &mut self.cpu_graph_upper,
            "user",
            CPU_GRAPH_SOURCE_VALUES,
            "cpu_graph_upper",
            &mut warnings,
        );
        validate_choice(
            &mut self.cpu_graph_lower,
            "system",
            CPU_GRAPH_SOURCE_VALUES,
            "cpu_graph_lower",
            &mut warnings,
        );
        validate_choice(
            &mut self.temp_scale,
            "celsius",
            TEMP_SCALE_VALUES,
            "temp_scale",
            &mut warnings,
        );
        validate_choice(
            &mut self.proc_sorting,
            "cpu lazy",
            crate::collect::process_display::SORT_OPTIONS,
            "proc_sorting",
            &mut warnings,
        );
        validate_choice(
            &mut self.log_level,
            "WARNING",
            LOG_LEVEL_VALUES,
            "log_level",
            &mut warnings,
        );

        // Validate shown_boxes: remove invalid box names
        let boxes: Vec<&str> = self.shown_boxes.split_whitespace().collect();
        let invalid: Vec<&str> = boxes
            .iter()
            .filter(|b| !is_valid_box_name(b))
            .copied()
            .collect();
        if !invalid.is_empty() {
            warnings.push(format!(
                "Invalid box name(s) in 'shown_boxes': {}",
                invalid.join(", ")
            ));
            let valid: Vec<&str> = boxes.into_iter().filter(|b| is_valid_box_name(b)).collect();
            self.shown_boxes = valid.join(" ");
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
        let preset0_parts: Vec<String> = source
            .split_whitespace()
            .map(|b| format!("{b}:0:default"))
            .collect();
        let mut list = vec![preset0_parts.join(",")];

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
        let shown = self.shown_boxes.clone();
        let cpu_bottom = if self.cpu_bottom { "1" } else { "0" };
        let mem_below_net = if self.mem_below_net { "1" } else { "0" };
        let proc_left = if self.proc_left { "1" } else { "0" };

        let mut parts = Vec::new();
        for box_name in shown.split_whitespace() {
            let pos = match box_name {
                "cpu" => cpu_bottom,
                "mem" => mem_below_net,
                "proc" => proc_left,
                _ => "0",
            };
            parts.push(format!("{box_name}:{pos}:default"));
        }
        let new_preset = parts.join(",");

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
            let _graph_sym = vals[2];

            boxes.push(box_name.to_string());

            match box_name {
                "cpu" => self.cpu_bottom = position != "0",
                "mem" => self.mem_below_net = position != "0",
                "proc" => self.proc_left = position != "0",
                _ => {}
            }
        }
        self.shown_boxes = boxes.join(" ");
    }

    /// Toggle a box's visibility in shown_boxes.
    pub fn toggle_box(&mut self, box_name: &str) -> bool {
        if !is_valid_box_name(box_name) {
            return false;
        }

        let current = self.shown_boxes.clone();
        let mut boxes: Vec<&str> = current.split_whitespace().collect();

        if let Some(pos) = boxes.iter().position(|b| *b == box_name) {
            boxes.remove(pos);
        } else {
            boxes.push(box_name);
        }

        self.shown_boxes = boxes.join(" ");
        true
    }
}

// ---------------------------------------------------------------------------
// ConfigKey — flat enum with one variant per config field
// ---------------------------------------------------------------------------

/// A flat enum identifying every config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
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
    ProcCpuGraphs,
    ProcLeft,

    ProcFollowDetailed,
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

    GpuMirrorGraph,
    DiskIoMode,

    // -- int variants --
    UpdateMs,
    NetDownload,
    NetUpload,
    DetailedPid,
    SelectedPid,
    FollowedPid,
    ProcStart,
    ProcSelected,
    CurrentPreset,

    // -- string variants --
    ColorTheme,
    ShownBoxes,
    GraphSymbol,
    GraphSymbolCpu,
    GraphSymbolGpu,
    GraphSymbolNet,
    GraphSymbolProc,
    GraphSymbolDisk,
    ProcSorting,
    CpuGraphUpper,
    CpuGraphLower,
    CpuSensor,

    TempScale,
    ClockFormat,
    CustomCpuName,
    DisksFilter,
    IoGraphSpeeds,
    NetIface,
    LogLevel,
    ProcFilter,
    Presets,
    CustomGpuName0,
    CustomGpuName1,
    CustomGpuName2,
    CustomGpuName3,
    CustomGpuName4,
    CustomGpuName5,
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
            Self::ProcCpuGraphs => "proc_cpu_graphs",
            Self::ProcLeft => "proc_left",

            Self::ProcFollowDetailed => "proc_follow_detailed",
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

            Self::GpuMirrorGraph => "gpu_mirror_graph",
            Self::DiskIoMode => "disk_io_mode",

            Self::UpdateMs => "update_ms",
            Self::NetDownload => "net_download",
            Self::NetUpload => "net_upload",
            Self::DetailedPid => "detailed_pid",
            Self::SelectedPid => "selected_pid",
            Self::FollowedPid => "followed_pid",
            Self::ProcStart => "proc_start",
            Self::ProcSelected => "proc_selected",
            Self::CurrentPreset => "current_preset",

            Self::ColorTheme => "color_theme",
            Self::ShownBoxes => "shown_boxes",
            Self::GraphSymbol => "graph_symbol",
            Self::GraphSymbolCpu => "graph_symbol_cpu",
            Self::GraphSymbolGpu => "graph_symbol_gpu",
            Self::GraphSymbolNet => "graph_symbol_net",
            Self::GraphSymbolProc => "graph_symbol_proc",
            Self::GraphSymbolDisk => "graph_symbol_disk",
            Self::ProcSorting => "proc_sorting",
            Self::CpuGraphUpper => "cpu_graph_upper",
            Self::CpuGraphLower => "cpu_graph_lower",
            Self::CpuSensor => "cpu_sensor",

            Self::TempScale => "temp_scale",
            Self::ClockFormat => "clock_format",
            Self::CustomCpuName => "custom_cpu_name",
            Self::DisksFilter => "disks_filter",
            Self::IoGraphSpeeds => "io_graph_speeds",
            Self::NetIface => "net_iface",
            Self::LogLevel => "log_level",
            Self::ProcFilter => "proc_filter",
            Self::Presets => "presets",
            Self::CustomGpuName0 => "custom_gpu_name0",
            Self::CustomGpuName1 => "custom_gpu_name1",
            Self::CustomGpuName2 => "custom_gpu_name2",
            Self::CustomGpuName3 => "custom_gpu_name3",
            Self::CustomGpuName4 => "custom_gpu_name4",
            Self::CustomGpuName5 => "custom_gpu_name5",
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
            | Self::ProcCpuGraphs
            | Self::ProcLeft
            | Self::ProcFollowDetailed
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
            | Self::GpuMirrorGraph
            | Self::DiskIoMode => KeyKind::Bool,

            Self::UpdateMs
            | Self::NetDownload
            | Self::NetUpload
            | Self::DetailedPid
            | Self::SelectedPid
            | Self::FollowedPid
            | Self::ProcStart
            | Self::ProcSelected
            | Self::CurrentPreset => KeyKind::Int,

            Self::ColorTheme
            | Self::ShownBoxes
            | Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolGpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolProc
            | Self::GraphSymbolDisk
            | Self::ProcSorting
            | Self::CpuGraphUpper
            | Self::CpuGraphLower
            | Self::CpuSensor
            | Self::TempScale
            | Self::ClockFormat
            | Self::CustomCpuName
            | Self::DisksFilter
            | Self::IoGraphSpeeds
            | Self::NetIface
            | Self::LogLevel
            | Self::ProcFilter
            | Self::Presets
            | Self::CustomGpuName0
            | Self::CustomGpuName1
            | Self::CustomGpuName2
            | Self::CustomGpuName3
            | Self::CustomGpuName4
            | Self::CustomGpuName5 => KeyKind::String,
        }
    }

    /// Parse a TOML field name into a ConfigKey.
    #[allow(dead_code)]
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
            "proc_cpu_graphs" => Some(Self::ProcCpuGraphs),
            "proc_left" => Some(Self::ProcLeft),

            "proc_follow_detailed" => Some(Self::ProcFollowDetailed),
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

            "gpu_mirror_graph" => Some(Self::GpuMirrorGraph),
            "disk_io_mode" => Some(Self::DiskIoMode),

            "update_ms" => Some(Self::UpdateMs),
            "net_download" => Some(Self::NetDownload),
            "net_upload" => Some(Self::NetUpload),
            "detailed_pid" => Some(Self::DetailedPid),
            "selected_pid" => Some(Self::SelectedPid),
            "followed_pid" => Some(Self::FollowedPid),
            "proc_start" => Some(Self::ProcStart),
            "proc_selected" => Some(Self::ProcSelected),
            "current_preset" => Some(Self::CurrentPreset),

            "color_theme" => Some(Self::ColorTheme),
            "shown_boxes" => Some(Self::ShownBoxes),
            "graph_symbol" => Some(Self::GraphSymbol),
            "graph_symbol_cpu" => Some(Self::GraphSymbolCpu),
            "graph_symbol_gpu" => Some(Self::GraphSymbolGpu),
            "graph_symbol_net" => Some(Self::GraphSymbolNet),
            "graph_symbol_proc" => Some(Self::GraphSymbolProc),
            "graph_symbol_disk" => Some(Self::GraphSymbolDisk),
            "proc_sorting" => Some(Self::ProcSorting),
            "cpu_graph_upper" => Some(Self::CpuGraphUpper),
            "cpu_graph_lower" => Some(Self::CpuGraphLower),
            "cpu_sensor" => Some(Self::CpuSensor),

            "temp_scale" => Some(Self::TempScale),
            "clock_format" => Some(Self::ClockFormat),
            "custom_cpu_name" => Some(Self::CustomCpuName),
            "disks_filter" => Some(Self::DisksFilter),
            "io_graph_speeds" => Some(Self::IoGraphSpeeds),
            "net_iface" => Some(Self::NetIface),
            "log_level" => Some(Self::LogLevel),
            "proc_filter" => Some(Self::ProcFilter),
            "presets" => Some(Self::Presets),
            "custom_gpu_name0" => Some(Self::CustomGpuName0),
            "custom_gpu_name1" => Some(Self::CustomGpuName1),
            "custom_gpu_name2" => Some(Self::CustomGpuName2),
            "custom_gpu_name3" => Some(Self::CustomGpuName3),
            "custom_gpu_name4" => Some(Self::CustomGpuName4),
            "custom_gpu_name5" => Some(Self::CustomGpuName5),

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
            Self::ProcCpuGraphs => bool_display(config.proc_cpu_graphs),
            Self::ProcLeft => bool_display(config.proc_left),

            Self::ProcFollowDetailed => bool_display(config.proc_follow_detailed),
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

            Self::GpuMirrorGraph => bool_display(config.gpu_mirror_graph),
            Self::DiskIoMode => bool_display(config.disk_io_mode),

            // ints
            Self::UpdateMs => config.update_ms.to_string(),
            Self::NetDownload => config.net_download.to_string(),
            Self::NetUpload => config.net_upload.to_string(),
            Self::DetailedPid => config.detailed_pid.to_string(),
            Self::SelectedPid => config.selected_pid.to_string(),
            Self::FollowedPid => config.followed_pid.to_string(),
            Self::ProcStart => config.proc_start.to_string(),
            Self::ProcSelected => config.proc_selected.to_string(),
            Self::CurrentPreset => config.current_preset.to_string(),

            // strings
            Self::ColorTheme => config.color_theme.clone(),
            Self::ShownBoxes => config.shown_boxes.clone(),
            Self::GraphSymbol => config.graph_symbol.clone(),
            Self::GraphSymbolCpu => config.graph_symbol_cpu.clone(),
            Self::GraphSymbolGpu => config.graph_symbol_gpu.clone(),
            Self::GraphSymbolNet => config.graph_symbol_net.clone(),
            Self::GraphSymbolProc => config.graph_symbol_proc.clone(),
            Self::GraphSymbolDisk => config.graph_symbol_disk.clone(),
            Self::ProcSorting => config.proc_sorting.clone(),
            Self::CpuGraphUpper => config.cpu_graph_upper.clone(),
            Self::CpuGraphLower => config.cpu_graph_lower.clone(),
            Self::CpuSensor => config.cpu_sensor.clone(),

            Self::TempScale => config.temp_scale.clone(),
            Self::ClockFormat => config.clock_format.clone(),
            Self::CustomCpuName => config.custom_cpu_name.clone(),
            Self::DisksFilter => config.disks_filter.clone(),
            Self::IoGraphSpeeds => config.io_graph_speeds.clone(),
            Self::NetIface => config.net_iface.clone(),
            Self::LogLevel => config.log_level.clone(),
            Self::ProcFilter => config.proc_filter.clone(),
            Self::Presets => config.presets.clone(),
            Self::CustomGpuName0 => config.custom_gpu_name0.clone(),
            Self::CustomGpuName1 => config.custom_gpu_name1.clone(),
            Self::CustomGpuName2 => config.custom_gpu_name2.clone(),
            Self::CustomGpuName3 => config.custom_gpu_name3.clone(),
            Self::CustomGpuName4 => config.custom_gpu_name4.clone(),
            Self::CustomGpuName5 => config.custom_gpu_name5.clone(),
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
            Self::ProcCpuGraphs => config.proc_cpu_graphs = !config.proc_cpu_graphs,
            Self::ProcLeft => config.proc_left = !config.proc_left,

            Self::ProcFollowDetailed => {
                config.proc_follow_detailed = !config.proc_follow_detailed;
            }
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

            Self::GpuMirrorGraph => config.gpu_mirror_graph = !config.gpu_mirror_graph,
            Self::DiskIoMode => config.disk_io_mode = !config.disk_io_mode,
            _ => panic!("toggle_bool called on non-bool key '{}'", self.name()),
        }
    }

    /// Get an integer value. Panics on non-int keys.
    pub fn get_int(self, config: &Config) -> i64 {
        match self {
            Self::UpdateMs => config.update_ms,
            Self::NetDownload => config.net_download,
            Self::NetUpload => config.net_upload,
            Self::DetailedPid => config.detailed_pid,
            Self::SelectedPid => config.selected_pid,
            Self::FollowedPid => config.followed_pid,
            Self::ProcStart => config.proc_start,
            Self::ProcSelected => config.proc_selected,
            Self::CurrentPreset => config.current_preset,
            _ => panic!("get_int called on non-int key '{}'", self.name()),
        }
    }

    /// Set an integer value. No clamping — caller should call validate().
    /// Panics on non-int keys.
    pub fn set_int(self, config: &mut Config, value: i64) {
        match self {
            Self::UpdateMs => config.update_ms = value,
            Self::NetDownload => config.net_download = value,
            Self::NetUpload => config.net_upload = value,
            Self::DetailedPid => config.detailed_pid = value,
            Self::SelectedPid => config.selected_pid = value,
            Self::FollowedPid => config.followed_pid = value,
            Self::ProcStart => config.proc_start = value,
            Self::ProcSelected => config.proc_selected = value,
            Self::CurrentPreset => config.current_preset = value,
            _ => panic!("set_int called on non-int key '{}'", self.name()),
        }
    }

    /// Get a string reference. Panics on non-string keys.
    pub fn get_string(self, config: &Config) -> &str {
        match self {
            Self::ColorTheme => &config.color_theme,
            Self::ShownBoxes => &config.shown_boxes,
            Self::GraphSymbol => &config.graph_symbol,
            Self::GraphSymbolCpu => &config.graph_symbol_cpu,
            Self::GraphSymbolGpu => &config.graph_symbol_gpu,
            Self::GraphSymbolNet => &config.graph_symbol_net,
            Self::GraphSymbolProc => &config.graph_symbol_proc,
            Self::GraphSymbolDisk => &config.graph_symbol_disk,
            Self::ProcSorting => &config.proc_sorting,
            Self::CpuGraphUpper => &config.cpu_graph_upper,
            Self::CpuGraphLower => &config.cpu_graph_lower,
            Self::CpuSensor => &config.cpu_sensor,

            Self::TempScale => &config.temp_scale,
            Self::ClockFormat => &config.clock_format,
            Self::CustomCpuName => &config.custom_cpu_name,
            Self::DisksFilter => &config.disks_filter,
            Self::IoGraphSpeeds => &config.io_graph_speeds,
            Self::NetIface => &config.net_iface,
            Self::LogLevel => &config.log_level,
            Self::ProcFilter => &config.proc_filter,
            Self::Presets => &config.presets,
            Self::CustomGpuName0 => &config.custom_gpu_name0,
            Self::CustomGpuName1 => &config.custom_gpu_name1,
            Self::CustomGpuName2 => &config.custom_gpu_name2,
            Self::CustomGpuName3 => &config.custom_gpu_name3,
            Self::CustomGpuName4 => &config.custom_gpu_name4,
            Self::CustomGpuName5 => &config.custom_gpu_name5,
            _ => panic!("get_string called on non-string key '{}'", self.name()),
        }
    }

    /// Set a string value. Panics on non-string keys.
    pub fn set_string(self, config: &mut Config, value: &str) {
        match self {
            Self::ColorTheme => config.color_theme = value.to_string(),
            Self::ShownBoxes => config.shown_boxes = value.to_string(),
            Self::GraphSymbol => config.graph_symbol = value.to_string(),
            Self::GraphSymbolCpu => config.graph_symbol_cpu = value.to_string(),
            Self::GraphSymbolGpu => config.graph_symbol_gpu = value.to_string(),
            Self::GraphSymbolNet => config.graph_symbol_net = value.to_string(),
            Self::GraphSymbolProc => config.graph_symbol_proc = value.to_string(),
            Self::GraphSymbolDisk => config.graph_symbol_disk = value.to_string(),
            Self::ProcSorting => config.proc_sorting = value.to_string(),
            Self::CpuGraphUpper => config.cpu_graph_upper = value.to_string(),
            Self::CpuGraphLower => config.cpu_graph_lower = value.to_string(),
            Self::CpuSensor => config.cpu_sensor = value.to_string(),

            Self::TempScale => config.temp_scale = value.to_string(),
            Self::ClockFormat => config.clock_format = value.to_string(),
            Self::CustomCpuName => config.custom_cpu_name = value.to_string(),
            Self::DisksFilter => config.disks_filter = value.to_string(),
            Self::IoGraphSpeeds => config.io_graph_speeds = value.to_string(),
            Self::NetIface => config.net_iface = value.to_string(),
            Self::LogLevel => config.log_level = value.to_string(),
            Self::ProcFilter => config.proc_filter = value.to_string(),
            Self::Presets => config.presets = value.to_string(),
            Self::CustomGpuName0 => config.custom_gpu_name0 = value.to_string(),
            Self::CustomGpuName1 => config.custom_gpu_name1 = value.to_string(),
            Self::CustomGpuName2 => config.custom_gpu_name2 = value.to_string(),
            Self::CustomGpuName3 => config.custom_gpu_name3 = value.to_string(),
            Self::CustomGpuName4 => config.custom_gpu_name4 = value.to_string(),
            Self::CustomGpuName5 => config.custom_gpu_name5 = value.to_string(),
            _ => panic!("set_string called on non-string key '{}'", self.name()),
        }
    }

    /// Returns the allowed values for constrained string keys, or None for free-form.
    pub fn choice_values(self) -> Option<&'static [&'static str]> {
        match self {
            Self::ColorTheme => Some(crate::theme::THEME_NAMES),
            Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolGpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolProc
            | Self::GraphSymbolDisk => Some(GRAPH_SYMBOL_VALUES),
            Self::CpuGraphUpper | Self::CpuGraphLower => Some(CPU_GRAPH_SOURCE_VALUES),
            Self::TempScale => Some(TEMP_SCALE_VALUES),
            Self::ProcSorting => Some(crate::collect::process_display::SORT_OPTIONS),
            Self::LogLevel => Some(LOG_LEVEL_VALUES),
            _ => None,
        }
    }

    /// Like choice_values but also returns `&["Auto"]` for sensor/battery/iface keys.
    /// Used by the options menu for browsable values.
    pub fn browsable_values(self) -> &'static [&'static str] {
        if let Some(values) = self.choice_values() {
            return values;
        }
        match self {
            Self::CpuSensor | Self::NetIface => &["Auto"],
            _ => &[],
        }
    }
}

fn bool_display(v: bool) -> String {
    if v {
        "True".to_string()
    } else {
        "False".to_string()
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
        assert_eq!(config.color_theme, "Default");
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
    fn load_invalid_string_values_generate_warnings() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_string_values.toml");
        fs::write(
            &tmp,
            "color_theme = \"foo\"\ngraph_symbol = \"ascii\"\nshown_boxes = \"cpu nope\"\n",
        )
        .unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 3);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("color_theme") && w.contains("foo"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("graph_symbol") && w.contains("ascii"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("shown_boxes") && w.contains("nope"))
        );
        assert_eq!(config.color_theme, "Default");
        assert_eq!(config.graph_symbol, "braille");
        // After removing invalid "nope", only "cpu" remains
        assert_eq!(config.shown_boxes, "cpu");
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
        assert_eq!(config.color_theme, "Default");
        assert_eq!(config.shown_boxes, "cpu mem net proc disk");
        assert_eq!(config.graph_symbol, "braille");
        assert_eq!(config.proc_sorting, "cpu lazy");
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
    fn toggle_box_adds_when_missing() {
        let mut config = Config::new();
        config.shown_boxes = "cpu mem".to_string();
        assert!(config.toggle_box("net"));
        assert!(config.shown_boxes.contains("net"));
    }

    #[test]
    fn toggle_box_removes_when_present() {
        let mut config = Config::new();
        config.shown_boxes = "cpu mem net".to_string();
        assert!(config.toggle_box("net"));
        assert!(!config.shown_boxes.contains("net"));
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
        assert_eq!(config.shown_boxes, "cpu proc");
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
        config.shown_boxes = "cpu proc".to_string();
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
        config.initial_shown_boxes = "cpu mem".to_string();
        assert_eq!(config.initial_shown_boxes, "cpu mem");
        // Should not appear in TOML output
        let output = toml::to_string_pretty(&config).unwrap();
        assert!(!output.contains("initial_shown_boxes"));
    }
}
