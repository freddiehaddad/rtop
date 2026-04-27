use std::fs;
use std::path::{Path, PathBuf};

use crate::config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk};

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

/// The kind of a config key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Bool,
    Int,
    String,
}

/// Generates the `Config` struct with named fields and string-based accessors.
///
/// Each field is private; access is via `get_bool`/`set_bool`/`get_int`/`set_int`/
/// `get_string`/`set_string` which dispatch by key name.
macro_rules! define_config {
    (
        bools { $( $bfield:ident : $bkey:literal => $bdefault:expr ),* $(,)? }
        ints { $( $ifield:ident : $ikey:literal => $idefault:expr , $imin:expr , $imax:expr ),* $(,)? }
        strings { $( $sfield:ident : $skey:literal => $sdefault:expr ),* $(,)? }
    ) => {
        /// All configuration state for rtop.
        ///
        /// Fields are private; use the `get_*`/`set_*` accessors for type-safe access
        /// by config key name.
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

            /// Get a boolean config value by key name.
            pub fn get_bool(&self, name: &str) -> bool {
                match name {
                    $( $bkey => self.$bfield, )*
                    _ => false,
                }
            }

            /// Set a boolean config value by key name.
            pub fn set_bool(&mut self, name: &str, value: bool) {
                match name {
                    $( $bkey => self.$bfield = value, )*
                    _ => {}
                }
            }

            /// Toggle a boolean config value.
            pub fn flip(&mut self, name: &str) {
                match name {
                    $( $bkey => self.$bfield = !self.$bfield, )*
                    _ => {}
                }
            }

            /// Get an integer config value by key name.
            pub fn get_int(&self, name: &str) -> i64 {
                match name {
                    $( $ikey => self.$ifield, )*
                    _ => 0,
                }
            }

            /// Set an integer config value by key name, with range clamping.
            pub fn set_int(&mut self, name: &str, value: i64) {
                match name {
                    $( $ikey => self.$ifield = value.clamp($imin, $imax), )*
                    _ => {}
                }
            }

            /// Get a string config value by key name.
            pub fn get_string(&self, name: &str) -> &str {
                match name {
                    $( $skey => &self.$sfield, )*
                    sk::INITIAL_SHOWN_BOXES => &self.initial_shown_boxes,
                    _ => "",
                }
            }

            /// Set a string config value by key name.
            pub fn set_string(&mut self, name: &str, value: &str) {
                match name {
                    $( $skey => self.$sfield = value.to_string(), )*
                    sk::INITIAL_SHOWN_BOXES => self.initial_shown_boxes = value.to_string(),
                    _ => {}
                }
            }

            /// Determine the kind of a config key, or `None` if unknown.
            pub fn key_kind(name: &str) -> Option<KeyKind> {
                match name {
                    $( $bkey => Some(KeyKind::Bool), )*
                    $( $ikey => Some(KeyKind::Int), )*
                    $( $skey => Some(KeyKind::String), )*
                    _ => None,
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

            /// Load config from a file. Returns a list of warnings for invalid values.
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

                    match Self::key_kind(key) {
                        Some(KeyKind::Bool) => {
                            match value.to_lowercase().as_str() {
                                "true" => self.set_bool(key, true),
                                "false" => self.set_bool(key, false),
                                _ => {
                                    warnings.push(format!(
                                        "Invalid boolean value for '{key}': {value}"
                                    ));
                                }
                            }
                        }
                        Some(KeyKind::Int) => {
                            match value.parse::<i64>() {
                                Ok(v) => self.set_int(key, v),
                                Err(_) => {
                                    warnings.push(format!(
                                        "Invalid integer value for '{key}': {value}"
                                    ));
                                }
                            }
                        }
                        Some(KeyKind::String) => {
                            self.set_string(key, value);
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
        theme_background:    "theme_background"    => true,
        truecolor:           "truecolor"           => true,
        rounded_corners:     "rounded_corners"     => true,
        proc_reversed:       "proc_reversed"       => false,
        proc_tree:           "proc_tree"           => false,
        proc_colors:         "proc_colors"         => true,
        proc_gradient:       "proc_gradient"       => true,
        proc_per_core:       "proc_per_core"       => false,
        proc_mem_bytes:      "proc_mem_bytes"      => true,
        proc_cpu_graphs:     "proc_cpu_graphs"     => true,
        proc_left:           "proc_left"           => false,
        proc_filter_kernel:  "proc_filter_kernel"  => false,
        proc_follow_detailed: "proc_follow_detailed" => true,
        proc_aggregate:      "proc_aggregate"      => false,
        keep_dead_proc_usage: "keep_dead_proc_usage" => false,
        cpu_invert_lower:    "cpu_invert_lower"    => true,
        cpu_single_graph:    "cpu_single_graph"    => false,
        cpu_bottom:          "cpu_bottom"          => false,
        show_uptime:         "show_uptime"         => true,
        show_cpu_watts:      "show_cpu_watts"      => true,
        check_temp:          "check_temp"          => true,
        show_coretemp:       "show_coretemp"       => true,
        show_cpu_freq:       "show_cpu_freq"       => true,
        mem_graphs:          "mem_graphs"          => true,
        mem_below_net:       "mem_below_net"       => false,
        show_swap:           "show_swap"           => true,
        swap_disk:           "swap_disk"           => true,
        show_disks:          "show_disks"          => true,
        only_physical:       "only_physical"       => true,
        show_io_stat:        "show_io_stat"        => true,
        io_mode:             "io_mode"             => false,
        io_graph_combined:   "io_graph_combined"   => false,
        swap_upload_download: "swap_upload_download" => false,
        base_10_sizes:       "base_10_sizes"       => false,
        net_auto:            "net_auto"            => true,
        net_sync:            "net_sync"            => false,
        show_battery:        "show_battery"        => true,
        show_battery_watts:  "show_battery_watts"  => true,
        vim_keys:            "vim_keys"            => false,
        force_tty:           "force_tty"           => false,
        lowcolor:            "lowcolor"            => false,
        background_update:   "background_update"   => true,
        terminal_sync:       "terminal_sync"       => true,
        save_config_on_exit: "save_config_on_exit" => true,
        disable_mouse:       "disable_mouse"       => false,
        disk_free_priv:      "disk_free_priv"      => false,
        gpu_mirror_graph:    "gpu_mirror_graph"    => true,
        disk_io_mode:        "disk_io_mode"        => false,
    }
    ints {
        update_ms:       "update_ms"       => 2000,    100,         86_400_000,
        net_download:    "net_download"    => 100,     0,           10_000_000,
        net_upload:      "net_upload"      => 100,     0,           10_000_000,
        detailed_pid:    "detailed_pid"    => 0,       i64::MIN,    i64::MAX,
        selected_pid:    "selected_pid"    => 0,       0,           i64::MAX,
        followed_pid:    "followed_pid"    => 0,       0,           i64::MAX,
        proc_start:      "proc_start"      => 0,       0,           i64::MAX,
        proc_selected:   "proc_selected"   => 0,       0,           i64::MAX,
        current_preset:  "current_preset"  => 0,       0,           i64::MAX,
    }
    strings {
        color_theme:      "color_theme"      => "Default",
        shown_boxes:      "shown_boxes"      => "cpu mem net proc disk",
        graph_symbol:     "graph_symbol"     => "braille",
        graph_symbol_cpu: "graph_symbol_cpu" => "default",
        graph_symbol_gpu: "graph_symbol_gpu" => "default",
        graph_symbol_mem: "graph_symbol_mem" => "default",
        graph_symbol_net: "graph_symbol_net" => "default",
        graph_symbol_proc: "graph_symbol_proc" => "default",
        proc_sorting:     "proc_sorting"     => "cpu lazy",
        cpu_graph_upper:  "cpu_graph_upper"  => "user",
        cpu_graph_lower:  "cpu_graph_lower"  => "system",
        cpu_sensor:       "cpu_sensor"       => "Auto",
        selected_battery: "selected_battery" => "Auto",
        cpu_core_map:     "cpu_core_map"     => "",
        temp_scale:       "temp_scale"       => "celsius",
        clock_format:     "clock_format"     => "%X",
        custom_cpu_name:  "custom_cpu_name"  => "",
        disks_filter:     "disks_filter"     => "",
        io_graph_speeds:  "io_graph_speeds"  => "",
        net_iface:        "net_iface"        => "",
        log_level:        "log_level"        => "WARNING",
        proc_filter:      "proc_filter"      => "",
        presets:          "presets"           => "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default",
        custom_gpu_name0: "custom_gpu_name0" => "",
        custom_gpu_name1: "custom_gpu_name1" => "",
        custom_gpu_name2: "custom_gpu_name2" => "",
        custom_gpu_name3: "custom_gpu_name3" => "",
        custom_gpu_name4: "custom_gpu_name4" => "",
        custom_gpu_name5: "custom_gpu_name5" => "",
    }
}

impl Config {
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
        assert_eq!(config.get_int("update_ms"), 2000);
        assert_eq!(config.get_string("color_theme"), "Default");
        assert!(config.get_bool("truecolor"));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_valid_config_parses_all_types() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_valid.conf");
        let mut f = fs::File::create(&tmp).unwrap();
        writeln!(f, "color_theme = \"dracula\"").unwrap();
        writeln!(f, "truecolor = False").unwrap();
        writeln!(f, "update_ms = 500").unwrap();
        drop(f);

        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.get_string("color_theme"), "dracula");
        assert!(!config.get_bool("truecolor"));
        assert_eq!(config.get_int("update_ms"), 500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_preserves_comments() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_comments.conf");
        fs::write(&tmp, "# this is a comment\nupdate_ms = 1000\n").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.get_int("update_ms"), 1000);
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
        assert_eq!(config.get_int("update_ms"), 2000);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn write_roundtrip_preserves_values() {
        let mut config = Config::new();
        config.set_string("color_theme", "nord");
        config.set_bool("vim_keys", true);
        config.set_int("update_ms", 1500);

        let tmp = std::env::temp_dir().join("rtop_test_roundtrip.conf");
        config.write(&tmp).unwrap();

        let mut config2 = Config::new();
        config2.load(&tmp);
        assert_eq!(config2.get_string("color_theme"), "nord");
        assert!(config2.get_bool("vim_keys"));
        assert_eq!(config2.get_int("update_ms"), 1500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn get_bool_returns_default() {
        let config = Config::new();
        assert!(config.get_bool("truecolor"));
        assert!(!config.get_bool("vim_keys"));
    }

    #[test]
    fn set_int_clamps_to_range() {
        let mut config = Config::new();
        config.set_int("update_ms", 50);
        assert_eq!(config.get_int("update_ms"), 100);
        config.set_int("update_ms", 100_000_000);
        assert_eq!(config.get_int("update_ms"), 86_400_000);
    }

    #[test]
    fn flip_toggles_boolean() {
        let mut config = Config::new();
        assert!(config.get_bool("truecolor"));
        config.flip("truecolor");
        assert!(!config.get_bool("truecolor"));
        config.flip("truecolor");
        assert!(config.get_bool("truecolor"));
    }

    #[test]
    fn toggle_box_adds_when_missing() {
        let mut config = Config::new();
        config.set_string("shown_boxes", "cpu mem");
        assert!(config.toggle_box("net"));
        assert!(config.get_string("shown_boxes").contains("net"));
    }

    #[test]
    fn toggle_box_removes_when_present() {
        let mut config = Config::new();
        config.set_string("shown_boxes", "cpu mem net");
        assert!(config.toggle_box("net"));
        assert!(!config.get_string("shown_boxes").contains("net"));
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
        // Verify key_kind returns correct types for known keys
        assert_eq!(Config::key_kind("color_theme"), Some(KeyKind::String));
        assert_eq!(Config::key_kind("proc_sorting"), Some(KeyKind::String));
        assert_eq!(Config::key_kind("truecolor"), Some(KeyKind::Bool));
        assert_eq!(Config::key_kind("show_battery"), Some(KeyKind::Bool));
        assert_eq!(Config::key_kind("update_ms"), Some(KeyKind::Int));
        assert_eq!(Config::key_kind("net_download"), Some(KeyKind::Int));
        // Verify defaults are accessible
        assert_eq!(config.get_string("color_theme"), "Default");
        assert!(config.get_bool("truecolor"));
        assert_eq!(config.get_int("update_ms"), 2000);
    }

    #[test]
    fn key_kind_returns_none_for_unknown() {
        assert_eq!(Config::key_kind("nonexistent"), None);
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
            "presets",
            "cpu:0:default,proc:0:default cpu:1:braille,mem:0:default",
        );
        let list = config.preset_list();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn apply_preset_sets_shown_boxes() {
        let mut config = Config::new();
        config.apply_preset("cpu:0:default,proc:1:default");
        assert_eq!(config.get_string("shown_boxes"), "cpu proc");
        assert!(config.get_bool("proc_left"));
    }

    #[test]
    fn apply_preset_cpu_bottom() {
        let mut config = Config::new();
        config.apply_preset("cpu:1:default,mem:0:default");
        assert!(config.get_bool("cpu_bottom"));
        assert!(!config.get_bool("mem_below_net"));
    }

    #[test]
    fn current_preset_default_is_zero() {
        let config = Config::new();
        assert_eq!(config.get_int("current_preset"), 0);
    }

    #[test]
    fn detailed_pid_allows_zero() {
        let mut config = Config::new();
        config.set_int("detailed_pid", 0);
        assert_eq!(config.get_int("detailed_pid"), 0);
    }

    #[test]
    fn save_preset_appends_and_sets_current() {
        let mut config = Config::new();
        config.set_string("presets", "");
        config.set_string("shown_boxes", "cpu proc");
        config.set_bool("proc_left", true);
        let idx = config.save_preset();
        assert!(idx > 0);
        assert_eq!(config.get_int("current_preset"), idx as i64);
        let list = config.preset_list();
        let last = &list[idx];
        assert!(last.contains("cpu:0:default"));
        assert!(last.contains("proc:1:default"));
    }

    #[test]
    fn delete_preset_removes_custom() {
        let mut config = Config::new();
        config.set_string(
            "presets",
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
        config.set_string("initial_shown_boxes", "cpu mem");
        assert_eq!(config.get_string("initial_shown_boxes"), "cpu mem");
        // Should not appear in serialized output
        let output = config.to_config_string();
        assert!(!output.contains("initial_shown_boxes"));
    }
}
