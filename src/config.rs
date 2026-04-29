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
const LOG_LEVEL_VALUES: &[&str] = &["ERROR", "WARNING", "INFO", "DEBUG"];

/// The kind of a config key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Bool,
    Int,
    String,
}

/// Boolean config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoolKey {
    name: &'static str,
}

impl BoolKey {
    /// External config file name for this key.
    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Integer config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntKey {
    name: &'static str,
}

impl IntKey {
    /// External config file name for this key.
    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// String config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringKey {
    name: &'static str,
}

impl StringKey {
    /// External config file name for this key.
    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Any config key, preserving its value kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    Bool(BoolKey),
    Int(IntKey),
    String(StringKey),
}

impl ConfigKey {
    /// External config file name for this key.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bool(key) => key.name(),
            Self::Int(key) => key.name(),
            Self::String(key) => key.name(),
        }
    }

    /// Value kind for this key.
    pub const fn kind(self) -> KeyKind {
        match self {
            Self::Bool(_) => KeyKind::Bool,
            Self::Int(_) => KeyKind::Int,
            Self::String(_) => KeyKind::String,
        }
    }
}

/// Generates typed config keys and the `Config` struct with named fields.
///
/// Each field is private; access is via typed `get_*`/`set_*` accessors.
macro_rules! define_config {
    (
        bools { $( $bconst:ident $bfield:ident : $bkey:literal => $bdefault:expr ),* $(,)? }
        ints { $( $iconst:ident $ifield:ident : $ikey:literal => $idefault:expr , $imin:expr , $imax:expr ),* $(,)? }
        strings { $( $sconst:ident $sfield:ident : $skey:literal => $sdefault:expr ),* $(,)? }
    ) => {
        /// Boolean config keys.
        pub mod bool_keys {
            use super::BoolKey;

            $( pub const $bconst: BoolKey = BoolKey { name: $bkey }; )*
        }

        /// Integer config keys.
        pub mod int_keys {
            use super::IntKey;

            $( pub const $iconst: IntKey = IntKey { name: $ikey }; )*
        }

        /// String config keys.
        pub mod str_keys {
            use super::StringKey;

            $( pub const $sconst: StringKey = StringKey { name: $skey }; )*
            pub const INITIAL_SHOWN_BOXES: StringKey = StringKey {
                name: "initial_shown_boxes",
            };
        }

        impl ConfigKey {
            /// Parse an external config file key name into a typed key.
            pub fn parse(name: &str) -> Option<Self> {
                match name {
                    $( $bkey => Some(Self::Bool(bool_keys::$bconst)), )*
                    $( $ikey => Some(Self::Int(int_keys::$iconst)), )*
                    $( $skey => Some(Self::String(str_keys::$sconst)), )*
                    _ => None,
                }
            }
        }

        impl IntKey {
            /// Return the valid range for this integer config key.
            pub fn bounds(self) -> (i64, i64) {
                match self.name() {
                    $( $ikey => ($imin, $imax), )*
                    _ => unreachable!("unknown integer config key '{}'", self.name()),
                }
            }
        }

        /// All configuration state for rtop.
        ///
        /// Fields are private; use the typed `get_*`/`set_*` accessors.
        #[derive(Debug, Clone)]
        pub struct Config {
            $( $bfield: bool, )*
            $( $ifield: i64, )*
            $( $sfield: String, )*
            /// Internal-only: the startup layout snapshot (not persisted).
            initial_shown_boxes: String,
            conf_file: Option<PathBuf>,
        }

        impl Config {
            /// Create a new Config with all default values.
            pub fn new() -> Self {
                Self {
                    $( $bfield: $bdefault, )*
                    $( $ifield: $idefault, )*
                    $( $sfield: $sdefault.to_string(), )*
                    initial_shown_boxes: String::new(),
                    conf_file: None,
                }
            }

            fn apply_defaults(&mut self) {
                $( self.$bfield = $bdefault; )*
                $( self.$ifield = $idefault; )*
                $( self.$sfield = $sdefault.to_string(); )*
                // initial_shown_boxes intentionally NOT reset
            }

            /// Get a boolean config value by typed key.
            pub fn get_bool(&self, key: BoolKey) -> bool {
                match key.name() {
                    $( $bkey => self.$bfield, )*
                    _ => unreachable!("unknown boolean config key '{}'", key.name()),
                }
            }

            /// Set a boolean config value by typed key.
            pub fn set_bool(&mut self, key: BoolKey, value: bool) {
                match key.name() {
                    $( $bkey => self.$bfield = value, )*
                    _ => unreachable!("unknown boolean config key '{}'", key.name()),
                }
            }

            /// Toggle a boolean config value.
            pub fn flip(&mut self, key: BoolKey) {
                match key.name() {
                    $( $bkey => self.$bfield = !self.$bfield, )*
                    _ => unreachable!("unknown boolean config key '{}'", key.name()),
                }
            }

            /// Get an integer config value by typed key.
            pub fn get_int(&self, key: IntKey) -> i64 {
                match key.name() {
                    $( $ikey => self.$ifield, )*
                    _ => unreachable!("unknown integer config key '{}'", key.name()),
                }
            }

            /// Set an integer config value by typed key, with range clamping.
            pub fn set_int(&mut self, key: IntKey, value: i64) {
                match key.name() {
                    $( $ikey => self.$ifield = value.clamp($imin, $imax), )*
                    _ => unreachable!("unknown integer config key '{}'", key.name()),
                }
            }

            /// Get a string config value by typed key.
            pub fn get_string(&self, key: StringKey) -> &str {
                match key.name() {
                    $( $skey => &self.$sfield, )*
                    "initial_shown_boxes" => &self.initial_shown_boxes,
                    _ => unreachable!("unknown string config key '{}'", key.name()),
                }
            }

            /// Set a string config value by typed key.
            pub fn set_string(&mut self, key: StringKey, value: &str) {
                match key.name() {
                    $( $skey => self.$sfield = value.to_string(), )*
                    "initial_shown_boxes" => self.initial_shown_boxes = value.to_string(),
                    _ => unreachable!("unknown string config key '{}'", key.name()),
                }
            }

            /// Generate the full config file content as a string.
            pub fn to_config_string(&self) -> String {
                let mut out = String::new();
                out.push_str("#? Config file for rtop\n\n");

                // Collect and sort string keys for deterministic output
                let mut skeys: Vec<(&str, &str)> = vec![
                    $( ($skey, &self.$sfield), )*
                ];
                skeys.sort_by_key(|(k, _)| *k);
                for (key, val) in &skeys {
                    out.push_str(&format!("{key} = \"{val}\"\n"));
                }
                out.push('\n');

                // Collect and sort bool keys
                let mut bkeys: Vec<(&str, bool)> = vec![
                    $( ($bkey, self.$bfield), )*
                ];
                bkeys.sort_by_key(|(k, _)| *k);
                for (key, val) in &bkeys {
                    let v = if *val { "True" } else { "False" };
                    out.push_str(&format!("{key} = {v}\n"));
                }
                out.push('\n');

                // Collect and sort int keys
                let mut ikeys: Vec<(&str, i64)> = vec![
                    $( ($ikey, self.$ifield), )*
                ];
                ikeys.sort_by_key(|(k, _)| *k);
                for (key, val) in &ikeys {
                    out.push_str(&format!("{key} = {val}\n"));
                }

                out
            }

            /// Load config from a file. Returns warnings for unknown keys and invalid values.
            pub fn load(&mut self, path: &Path) -> Vec<String> {
                let mut warnings = Vec::new();
                self.conf_file = Some(path.to_path_buf());

                let content = match fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => return warnings,
                };

                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }

                    let Some((key, value)) = trimmed.split_once('=') else {
                        continue;
                    };

                    let key = key.trim();
                    let value = value.trim().trim_matches('"');

                    match ConfigKey::parse(key) {
                        Some(ConfigKey::Bool(key)) => {
                            match value.to_lowercase().as_str() {
                                "true" => self.set_bool(key, true),
                                "false" => self.set_bool(key, false),
                                _ => warnings.push(format!(
                                    "Invalid boolean value for '{}': {value}",
                                    key.name()
                                )),
                            }
                        }
                        Some(ConfigKey::Int(key)) => {
                            match value.parse::<i64>() {
                                Ok(v) => {
                                    let (min, max) = key.bounds();
                                    if v < min || v > max {
                                        warnings.push(format!(
                                            "Integer value for '{}' out of range: {v} (expected {min}..={max})",
                                            key.name()
                                        ));
                                    }
                                    self.set_int(key, v);
                                }
                                Err(_) => warnings.push(format!(
                                    "Invalid integer value for '{}': {value}",
                                    key.name()
                                )),
                            }
                        }
                        Some(ConfigKey::String(key)) => {
                            if let Some(warning) = Self::validate_string_value(key, value) {
                                warnings.push(warning);
                            } else {
                                self.set_string(key, value);
                            }
                        }
                        None => {
                            warnings.push(format!("Unknown config key: '{key}'"));
                        }
                    }
                }

                warnings
            }
        }
    };
}

define_config! {
    bools {
        THEME_BACKGROUND theme_background:    "theme_background"    => true,
        ROUNDED_CORNERS rounded_corners:      "rounded_corners"     => true,
        PROC_REVERSED proc_reversed:          "proc_reversed"       => false,
        PROC_TREE proc_tree:                  "proc_tree"           => false,
        PROC_COLORS proc_colors:              "proc_colors"         => true,
        PROC_GRADIENT proc_gradient:          "proc_gradient"       => true,
        PROC_PER_CORE proc_per_core:          "proc_per_core"       => false,
        PROC_MEM_BYTES proc_mem_bytes:        "proc_mem_bytes"      => true,
        PROC_CPU_GRAPHS proc_cpu_graphs:      "proc_cpu_graphs"     => true,
        PROC_LEFT proc_left:                  "proc_left"           => false,
        PROC_FILTER_KERNEL proc_filter_kernel: "proc_filter_kernel"  => false,
        PROC_FOLLOW_DETAILED proc_follow_detailed: "proc_follow_detailed" => true,
        PROC_AGGREGATE proc_aggregate:        "proc_aggregate"      => false,
        KEEP_DEAD_PROC_USAGE keep_dead_proc_usage: "keep_dead_proc_usage" => false,
        CPU_INVERT_LOWER cpu_invert_lower:    "cpu_invert_lower"    => true,
        CPU_SINGLE_GRAPH cpu_single_graph:    "cpu_single_graph"    => false,
        CPU_BOTTOM cpu_bottom:                "cpu_bottom"          => false,
        SHOW_UPTIME show_uptime:              "show_uptime"         => true,
        SHOW_CPU_WATTS show_cpu_watts:        "show_cpu_watts"      => true,
        CHECK_TEMP check_temp:                "check_temp"          => true,
        SHOW_CORETEMP show_coretemp:          "show_coretemp"       => true,
        SHOW_CPU_FREQ show_cpu_freq:          "show_cpu_freq"       => true,
        MEM_GRAPHS mem_graphs:                "mem_graphs"          => true,
        MEM_BELOW_NET mem_below_net:          "mem_below_net"       => false,
        SHOW_SWAP show_swap:                  "show_swap"           => true,
        SWAP_DISK swap_disk:                  "swap_disk"           => true,
        SHOW_DISKS show_disks:                "show_disks"          => true,
        ONLY_PHYSICAL only_physical:          "only_physical"       => true,
        SHOW_IO_STAT show_io_stat:            "show_io_stat"        => true,
        IO_MODE io_mode:                      "io_mode"             => false,
        IO_GRAPH_COMBINED io_graph_combined:  "io_graph_combined"   => false,
        SWAP_UPLOAD_DOWNLOAD swap_upload_download: "swap_upload_download" => false,
        BASE_10_SIZES base_10_sizes:          "base_10_sizes"       => false,
        NET_AUTO net_auto:                    "net_auto"            => true,
        NET_SYNC net_sync:                    "net_sync"            => false,
        SHOW_BATTERY show_battery:            "show_battery"        => true,
        SHOW_BATTERY_WATTS show_battery_watts: "show_battery_watts"  => true,
        VIM_KEYS vim_keys:                    "vim_keys"            => false,
        BACKGROUND_UPDATE background_update:  "background_update"   => true,
        TERMINAL_SYNC terminal_sync:          "terminal_sync"       => true,
        SAVE_CONFIG_ON_EXIT save_config_on_exit: "save_config_on_exit" => true,
        DISK_FREE_PRIV disk_free_priv:        "disk_free_priv"      => false,
        GPU_MIRROR_GRAPH gpu_mirror_graph:    "gpu_mirror_graph"    => true,
        DISK_IO_MODE disk_io_mode:            "disk_io_mode"        => false,
    }
    ints {
        UPDATE_MS update_ms:             "update_ms"       => 2000,    100,         86_400_000,
        NET_DOWNLOAD net_download:       "net_download"    => 100,     0,           10_000_000,
        NET_UPLOAD net_upload:           "net_upload"      => 100,     0,           10_000_000,
        DETAILED_PID detailed_pid:       "detailed_pid"    => 0,       i64::MIN,    i64::MAX,
        SELECTED_PID selected_pid:       "selected_pid"    => 0,       0,           i64::MAX,
        FOLLOWED_PID followed_pid:       "followed_pid"    => 0,       0,           i64::MAX,
        PROC_START proc_start:           "proc_start"      => 0,       0,           i64::MAX,
        PROC_SELECTED proc_selected:     "proc_selected"   => 0,       0,           i64::MAX,
        CURRENT_PRESET current_preset:   "current_preset"  => 0,       0,           i64::MAX,
    }
    strings {
        COLOR_THEME color_theme:           "color_theme"      => "Default",
        SHOWN_BOXES shown_boxes:           "shown_boxes"      => "cpu mem net proc disk",
        GRAPH_SYMBOL graph_symbol:         "graph_symbol"     => "braille",
        GRAPH_SYMBOL_CPU graph_symbol_cpu: "graph_symbol_cpu" => "default",
        GRAPH_SYMBOL_GPU graph_symbol_gpu: "graph_symbol_gpu" => "default",
        GRAPH_SYMBOL_MEM graph_symbol_mem: "graph_symbol_mem" => "default",
        GRAPH_SYMBOL_NET graph_symbol_net: "graph_symbol_net" => "default",
        GRAPH_SYMBOL_PROC graph_symbol_proc: "graph_symbol_proc" => "default",
        GRAPH_SYMBOL_DISK graph_symbol_disk: "graph_symbol_disk" => "default",
        PROC_SORTING proc_sorting:         "proc_sorting"     => "cpu lazy",
        CPU_GRAPH_UPPER cpu_graph_upper:   "cpu_graph_upper"  => "user",
        CPU_GRAPH_LOWER cpu_graph_lower:   "cpu_graph_lower"  => "system",
        CPU_SENSOR cpu_sensor:             "cpu_sensor"       => "Auto",
        SELECTED_BATTERY selected_battery: "selected_battery" => "Auto",
        CPU_CORE_MAP cpu_core_map:         "cpu_core_map"     => "",
        TEMP_SCALE temp_scale:             "temp_scale"       => "celsius",
        CLOCK_FORMAT clock_format:         "clock_format"     => "%X",
        CUSTOM_CPU_NAME custom_cpu_name:   "custom_cpu_name"  => "",
        DISKS_FILTER disks_filter:         "disks_filter"     => "",
        IO_GRAPH_SPEEDS io_graph_speeds:   "io_graph_speeds"  => "",
        NET_IFACE net_iface:               "net_iface"        => "",
        LOG_LEVEL log_level:               "log_level"        => "WARNING",
        PROC_FILTER proc_filter:           "proc_filter"      => "",
        PRESETS presets:                   "presets"           => "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default",
        CUSTOM_GPU_NAME0 custom_gpu_name0: "custom_gpu_name0" => "",
        CUSTOM_GPU_NAME1 custom_gpu_name1: "custom_gpu_name1" => "",
        CUSTOM_GPU_NAME2 custom_gpu_name2: "custom_gpu_name2" => "",
        CUSTOM_GPU_NAME3 custom_gpu_name3: "custom_gpu_name3" => "",
        CUSTOM_GPU_NAME4 custom_gpu_name4: "custom_gpu_name4" => "",
        CUSTOM_GPU_NAME5 custom_gpu_name5: "custom_gpu_name5" => "",
    }
}

use self::{bool_keys as bk, int_keys as ik, str_keys as sk};

impl StringKey {
    /// Return strict choice values for string keys with constrained values.
    pub fn choice_values(self) -> Option<&'static [&'static str]> {
        match self {
            key if key == sk::COLOR_THEME => Some(crate::theme::THEME_NAMES),
            key if key == sk::GRAPH_SYMBOL
                || key == sk::GRAPH_SYMBOL_CPU
                || key == sk::GRAPH_SYMBOL_GPU
                || key == sk::GRAPH_SYMBOL_MEM
                || key == sk::GRAPH_SYMBOL_NET
                || key == sk::GRAPH_SYMBOL_PROC
                || key == sk::GRAPH_SYMBOL_DISK =>
            {
                Some(GRAPH_SYMBOL_VALUES)
            }
            key if key == sk::CPU_GRAPH_UPPER || key == sk::CPU_GRAPH_LOWER => {
                Some(CPU_GRAPH_SOURCE_VALUES)
            }
            key if key == sk::TEMP_SCALE => Some(TEMP_SCALE_VALUES),
            key if key == sk::PROC_SORTING => Some(crate::collect::process_display::SORT_OPTIONS),
            key if key == sk::LOG_LEVEL => Some(LOG_LEVEL_VALUES),
            _ => None,
        }
    }
}

impl Config {
    fn validate_string_value(key: StringKey, value: &str) -> Option<String> {
        if key == sk::SHOWN_BOXES {
            let invalid: Vec<&str> = value
                .split_whitespace()
                .filter(|name| !is_valid_box_name(name))
                .collect();
            if invalid.is_empty() {
                return None;
            }
            return Some(format!(
                "Invalid box name(s) for '{}': {}",
                key.name(),
                invalid.join(", ")
            ));
        }

        let choices = key.choice_values()?;
        if choices.contains(&value) {
            None
        } else {
            Some(format!(
                "Invalid value for '{}': {value} (expected one of: {})",
                key.name(),
                choices.join(", ")
            ))
        }
    }

    /// Write config to the config file.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let content = self.to_config_string();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    /// Reload config from the stored config file path.
    /// Returns warnings from parsing.
    pub fn reload(&mut self) -> Vec<String> {
        if let Some(path) = self.conf_file.clone() {
            self.apply_defaults();
            self.load(&path)
        } else {
            Vec::new()
        }
    }

    /// Parse the presets config string into a list of preset strings.
    /// Preset 0 is the startup layout stored in `initial_shown_boxes`.
    pub fn preset_list(&self) -> Vec<String> {
        let initial = self.get_string(sk::INITIAL_SHOWN_BOXES);
        let source = if initial.is_empty() {
            self.get_string(sk::SHOWN_BOXES)
        } else {
            initial
        };
        let preset0_parts: Vec<String> = source
            .split_whitespace()
            .map(|b| format!("{b}:0:default"))
            .collect();
        let mut list = vec![preset0_parts.join(",")];

        let presets_str = self.get_string(sk::PRESETS);
        if !presets_str.is_empty() {
            for preset in presets_str.split_whitespace() {
                if !preset.is_empty() {
                    list.push(preset.to_string());
                }
            }
        }
        list
    }

    /// Save the current layout as a new preset and return its index.
    pub fn save_preset(&mut self) -> usize {
        let shown = self.get_string(sk::SHOWN_BOXES).to_string();
        let cpu_bottom = if self.get_bool(bk::CPU_BOTTOM) {
            "1"
        } else {
            "0"
        };
        let mem_below_net = if self.get_bool(bk::MEM_BELOW_NET) {
            "1"
        } else {
            "0"
        };
        let proc_left = if self.get_bool(bk::PROC_LEFT) {
            "1"
        } else {
            "0"
        };

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

        let current = self.get_string(sk::PRESETS).to_string();
        let updated = if current.is_empty() {
            new_preset
        } else {
            format!("{current} {new_preset}")
        };
        self.set_string(sk::PRESETS, &updated);
        let idx = self.preset_list().len() - 1;
        self.set_int(ik::CURRENT_PRESET, idx as i64);
        idx
    }

    /// Delete the preset at the given index. Index 0 (the default) cannot be deleted.
    /// Returns true if a preset was deleted.
    pub fn delete_preset(&mut self, index: usize) -> bool {
        if index == 0 {
            return false;
        }
        let presets_str = self.get_string(sk::PRESETS).to_string();
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
        self.set_string(sk::PRESETS, &remaining.join(" "));
        let cur = self.get_int(ik::CURRENT_PRESET);
        let total = self.preset_list().len() as i64;
        if cur >= total {
            self.set_int(ik::CURRENT_PRESET, total - 1);
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
                "cpu" => self.set_bool(bk::CPU_BOTTOM, position != "0"),
                "mem" => self.set_bool(bk::MEM_BELOW_NET, position != "0"),
                "proc" => self.set_bool(bk::PROC_LEFT, position != "0"),
                _ => {}
            }
        }
        self.set_string(sk::SHOWN_BOXES, &boxes.join(" "));
    }

    /// Toggle a box's visibility in shown_boxes.
    pub fn toggle_box(&mut self, box_name: &str) -> bool {
        if !is_valid_box_name(box_name) {
            return false;
        }

        let current = self.get_string(sk::SHOWN_BOXES).to_string();
        let mut boxes: Vec<&str> = current.split_whitespace().collect();

        if let Some(pos) = boxes.iter().position(|b| *b == box_name) {
            boxes.remove(pos);
        } else {
            boxes.push(box_name);
        }

        self.set_string(sk::SHOWN_BOXES, &boxes.join(" "));
        true
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_empty_file_uses_defaults() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_empty.conf");
        fs::write(&tmp, "").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.get_int(ik::UPDATE_MS), 2000);
        assert_eq!(config.get_string(sk::COLOR_THEME), "Default");
        assert!(config.get_bool(bk::THEME_BACKGROUND));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_valid_config_parses_all_types() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_valid.conf");
        let mut f = fs::File::create(&tmp).unwrap();
        writeln!(f, "color_theme = \"dracula\"").unwrap();
        writeln!(f, "theme_background = False").unwrap();
        writeln!(f, "update_ms = 500").unwrap();
        drop(f);

        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.get_string(sk::COLOR_THEME), "dracula");
        assert!(!config.get_bool(bk::THEME_BACKGROUND));
        assert_eq!(config.get_int(ik::UPDATE_MS), 500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_preserves_comments() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_comments.conf");
        fs::write(&tmp, "# this is a comment\nupdate_ms = 1000\n").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.get_int(ik::UPDATE_MS), 1000);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_key_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_unknown.conf");
        fs::write(&tmp, "nonexistent_key = \"value\"\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Unknown"));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_value_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_badval.conf");
        fs::write(&tmp, "update_ms = not_a_number\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert_eq!(config.get_int(ik::UPDATE_MS), 2000);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_out_of_range_int_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_out_of_range.conf");
        fs::write(&tmp, "update_ms = 50\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("out of range"));
        assert_eq!(config.get_int(ik::UPDATE_MS), 100);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_string_values_generate_warnings() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_string_values.conf");
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
                .any(|warning| warning.contains("color_theme") && warning.contains("foo"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("graph_symbol") && warning.contains("ascii"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("shown_boxes") && warning.contains("nope"))
        );
        assert_eq!(config.get_string(sk::COLOR_THEME), "Default");
        assert_eq!(config.get_string(sk::GRAPH_SYMBOL), "braille");
        assert_eq!(config.get_string(sk::SHOWN_BOXES), "cpu mem net proc disk");
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn write_roundtrip_preserves_values() {
        let mut config = Config::new();
        config.set_string(sk::COLOR_THEME, "nord");
        config.set_bool(bk::VIM_KEYS, true);
        config.set_int(ik::UPDATE_MS, 1500);

        let tmp = std::env::temp_dir().join("rtop_test_roundtrip.conf");
        config.write(&tmp).unwrap();

        let mut config2 = Config::new();
        config2.load(&tmp);
        assert_eq!(config2.get_string(sk::COLOR_THEME), "nord");
        assert!(config2.get_bool(bk::VIM_KEYS));
        assert_eq!(config2.get_int(ik::UPDATE_MS), 1500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn get_bool_returns_default() {
        let config = Config::new();
        assert!(config.get_bool(bk::THEME_BACKGROUND));
        assert!(!config.get_bool(bk::VIM_KEYS));
    }

    #[test]
    fn set_int_clamps_to_range() {
        let mut config = Config::new();
        config.set_int(ik::UPDATE_MS, 50);
        assert_eq!(config.get_int(ik::UPDATE_MS), 100);
        config.set_int(ik::UPDATE_MS, 100_000_000);
        assert_eq!(config.get_int(ik::UPDATE_MS), 86_400_000);
    }

    #[test]
    fn flip_toggles_boolean() {
        let mut config = Config::new();
        assert!(config.get_bool(bk::THEME_BACKGROUND));
        config.flip(bk::THEME_BACKGROUND);
        assert!(!config.get_bool(bk::THEME_BACKGROUND));
        config.flip(bk::THEME_BACKGROUND);
        assert!(config.get_bool(bk::THEME_BACKGROUND));
    }

    #[test]
    fn toggle_box_adds_when_missing() {
        let mut config = Config::new();
        config.set_string(sk::SHOWN_BOXES, "cpu mem");
        assert!(config.toggle_box("net"));
        assert!(config.get_string(sk::SHOWN_BOXES).contains("net"));
    }

    #[test]
    fn toggle_box_removes_when_present() {
        let mut config = Config::new();
        config.set_string(sk::SHOWN_BOXES, "cpu mem net");
        assert!(config.toggle_box("net"));
        assert!(!config.get_string(sk::SHOWN_BOXES).contains("net"));
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
    fn default_has_all_expected_keys() {
        let config = Config::new();
        // Verify raw file keys parse to the correct typed key kinds.
        assert_eq!(
            ConfigKey::parse("color_theme").map(ConfigKey::kind),
            Some(KeyKind::String)
        );
        assert_eq!(
            ConfigKey::parse("proc_sorting").map(ConfigKey::kind),
            Some(KeyKind::String)
        );
        assert_eq!(
            ConfigKey::parse("theme_background").map(ConfigKey::kind),
            Some(KeyKind::Bool)
        );
        assert_eq!(
            ConfigKey::parse("show_battery").map(ConfigKey::kind),
            Some(KeyKind::Bool)
        );
        assert_eq!(
            ConfigKey::parse("update_ms").map(ConfigKey::kind),
            Some(KeyKind::Int)
        );
        assert_eq!(
            ConfigKey::parse("net_download").map(ConfigKey::kind),
            Some(KeyKind::Int)
        );
        // Verify defaults are accessible
        assert_eq!(config.get_string(sk::COLOR_THEME), "Default");
        assert!(config.get_bool(bk::THEME_BACKGROUND));
        assert_eq!(config.get_int(ik::UPDATE_MS), 2000);
    }

    #[test]
    fn config_key_parse_returns_none_for_unknown() {
        assert_eq!(ConfigKey::parse("nonexistent"), None);
    }

    #[test]
    fn typed_config_keys_roundtrip_through_parser() {
        let keys = [
            ConfigKey::String(sk::COLOR_THEME),
            ConfigKey::String(sk::PROC_SORTING),
            ConfigKey::Bool(bk::THEME_BACKGROUND),
            ConfigKey::Bool(bk::SHOW_BATTERY),
            ConfigKey::Int(ik::UPDATE_MS),
            ConfigKey::Int(ik::NET_DOWNLOAD),
        ];

        for key in keys {
            assert_eq!(ConfigKey::parse(key.name()), Some(key));
        }
        assert_eq!(ConfigKey::parse(sk::INITIAL_SHOWN_BOXES.name()), None);
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
        config.set_string(
            sk::PRESETS,
            "cpu:0:default,proc:0:default cpu:1:braille,mem:0:default",
        );
        let list = config.preset_list();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn apply_preset_sets_shown_boxes() {
        let mut config = Config::new();
        config.apply_preset("cpu:0:default,proc:1:default");
        assert_eq!(config.get_string(sk::SHOWN_BOXES), "cpu proc");
        assert!(config.get_bool(bk::PROC_LEFT));
    }

    #[test]
    fn apply_preset_cpu_bottom() {
        let mut config = Config::new();
        config.apply_preset("cpu:1:default,mem:0:default");
        assert!(config.get_bool(bk::CPU_BOTTOM));
        assert!(!config.get_bool(bk::MEM_BELOW_NET));
    }

    #[test]
    fn current_preset_default_is_zero() {
        let config = Config::new();
        assert_eq!(config.get_int(ik::CURRENT_PRESET), 0);
    }

    #[test]
    fn detailed_pid_allows_zero() {
        let mut config = Config::new();
        config.set_int(ik::DETAILED_PID, 0);
        assert_eq!(config.get_int(ik::DETAILED_PID), 0);
    }

    #[test]
    fn save_preset_appends_and_sets_current() {
        let mut config = Config::new();
        config.set_string(sk::PRESETS, "");
        config.set_string(sk::SHOWN_BOXES, "cpu proc");
        config.set_bool(bk::PROC_LEFT, true);
        let idx = config.save_preset();
        assert!(idx > 0);
        assert_eq!(config.get_int(ik::CURRENT_PRESET), idx as i64);
        let list = config.preset_list();
        let last = &list[idx];
        assert!(last.contains("cpu:0:default"));
        assert!(last.contains("proc:1:default"));
    }

    #[test]
    fn delete_preset_removes_custom() {
        let mut config = Config::new();
        config.set_string(
            sk::PRESETS,
            "cpu:0:default,proc:0:default mem:0:default,net:0:default",
        );
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
    fn initial_shown_boxes_is_internal() {
        let mut config = Config::new();
        config.set_string(sk::INITIAL_SHOWN_BOXES, "cpu mem");
        assert_eq!(config.get_string(sk::INITIAL_SHOWN_BOXES), "cpu mem");
        // Should not appear in serialized output
        let output = config.to_config_string();
        assert!(!output.contains("initial_shown_boxes"));
    }
}
