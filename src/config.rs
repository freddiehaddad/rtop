use std::collections::HashMap;
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
    // Accept "gpu0", "gpu1", etc. — any gpuN where N is a single digit
    if let Some(suffix) = name.strip_prefix("gpu") {
        return suffix.len() == 1 && suffix.chars().all(|c| c.is_ascii_digit());
    }
    false
}

/// All configuration state for rtop.
#[derive(Debug, Clone)]
pub struct Config {
    pub strings: HashMap<String, String>,
    pub bools: HashMap<String, bool>,
    pub ints: HashMap<String, i64>,
    conf_file: Option<PathBuf>,
}

impl Config {
    /// Create a new Config with all default values.
    pub fn new() -> Self {
        let mut c = Self {
            strings: HashMap::new(),
            bools: HashMap::new(),
            ints: HashMap::new(),
            conf_file: None,
        };
        c.apply_defaults();
        c
    }

    fn apply_defaults(&mut self) {
        // String defaults
        let string_defaults: &[(&str, &str)] = &[
            (sk::COLOR_THEME, "Default"),
            (sk::SHOWN_BOXES, "cpu mem net proc disk"),
            (sk::GRAPH_SYMBOL, "braille"),
            (sk::GRAPH_SYMBOL_CPU, "default"),
            (sk::GRAPH_SYMBOL_GPU, "default"),
            (sk::GRAPH_SYMBOL_MEM, "default"),
            (sk::GRAPH_SYMBOL_NET, "default"),
            (sk::GRAPH_SYMBOL_PROC, "default"),
            (sk::PROC_SORTING, "cpu lazy"),
            (sk::CPU_GRAPH_UPPER, "user"),
            (sk::CPU_GRAPH_LOWER, "system"),
            (sk::CPU_SENSOR, "Auto"),
            (sk::SELECTED_BATTERY, "Auto"),
            (sk::CPU_CORE_MAP, ""),
            (sk::TEMP_SCALE, "celsius"),
            (sk::CLOCK_FORMAT, "%X"),
            (sk::CUSTOM_CPU_NAME, ""),
            (sk::DISKS_FILTER, ""),
            (sk::IO_GRAPH_SPEEDS, ""),
            (sk::NET_IFACE, ""),
            (sk::LOG_LEVEL, "WARNING"),
            (sk::PROC_FILTER, ""),
            (
                sk::PRESETS,
                "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default",
            ),
            (sk::CUSTOM_GPU_NAME0, ""),
            (sk::CUSTOM_GPU_NAME1, ""),
            (sk::CUSTOM_GPU_NAME2, ""),
            (sk::CUSTOM_GPU_NAME3, ""),
            (sk::CUSTOM_GPU_NAME4, ""),
            (sk::CUSTOM_GPU_NAME5, ""),
        ];
        for (k, v) in string_defaults {
            self.strings
                .entry(k.to_string())
                .or_insert_with(|| v.to_string());
        }

        // Bool defaults
        let bool_defaults: &[(&str, bool)] = &[
            (bk::THEME_BACKGROUND, true),
            (bk::TRUECOLOR, true),
            (bk::ROUNDED_CORNERS, true),
            (bk::PROC_REVERSED, false),
            (bk::PROC_TREE, false),
            (bk::PROC_COLORS, true),
            (bk::PROC_GRADIENT, true),
            (bk::PROC_PER_CORE, false),
            (bk::PROC_MEM_BYTES, true),
            (bk::PROC_CPU_GRAPHS, true),
            (bk::PROC_LEFT, false),
            (bk::PROC_FILTER_KERNEL, false),
            (bk::PROC_FOLLOW_DETAILED, true),
            (bk::PROC_AGGREGATE, false),
            (bk::KEEP_DEAD_PROC_USAGE, false),
            (bk::CPU_INVERT_LOWER, true),
            (bk::CPU_SINGLE_GRAPH, false),
            (bk::CPU_BOTTOM, false),
            (bk::SHOW_UPTIME, true),
            (bk::SHOW_CPU_WATTS, true),
            (bk::CHECK_TEMP, true),
            (bk::SHOW_CORETEMP, true),
            (bk::SHOW_CPU_FREQ, true),
            (bk::MEM_GRAPHS, true),
            (bk::MEM_BELOW_NET, false),
            (bk::SHOW_SWAP, true),
            (bk::SWAP_DISK, true),
            (bk::SHOW_DISKS, true),
            (bk::ONLY_PHYSICAL, true),
            (bk::SHOW_IO_STAT, true),
            (bk::IO_MODE, false),
            (bk::IO_GRAPH_COMBINED, false),
            (bk::SWAP_UPLOAD_DOWNLOAD, false),
            (bk::BASE_10_SIZES, false),
            (bk::NET_AUTO, true),
            (bk::NET_SYNC, false),
            (bk::SHOW_BATTERY, true),
            (bk::SHOW_BATTERY_WATTS, true),
            (bk::VIM_KEYS, false),
            (bk::FORCE_TTY, false),
            (bk::LOWCOLOR, false),
            (bk::BACKGROUND_UPDATE, true),
            (bk::TERMINAL_SYNC, true),
            (bk::SAVE_CONFIG_ON_EXIT, true),
            (bk::DISABLE_MOUSE, false),
            (bk::DISK_FREE_PRIV, false),
            (bk::GPU_MIRROR_GRAPH, true),
            (bk::DISK_IO_MODE, false),
        ];
        for (k, v) in bool_defaults {
            self.bools.entry(k.to_string()).or_insert(*v);
        }

        // Int defaults
        let int_defaults: &[(&str, i64)] = &[
            (ik::UPDATE_MS, 2000),
            (ik::NET_DOWNLOAD, 100),
            (ik::NET_UPLOAD, 100),
            (ik::DETAILED_PID, 0),
            (ik::SELECTED_PID, 0),
            (ik::FOLLOWED_PID, 0),
            (ik::PROC_START, 0),
            (ik::PROC_SELECTED, 0),
            (ik::CURRENT_PRESET, 0),
        ];
        for (k, v) in int_defaults {
            self.ints.entry(k.to_string()).or_insert(*v);
        }
    }

    /// Get a boolean config value.
    pub fn get_bool(&self, name: &str) -> bool {
        self.bools.get(name).copied().unwrap_or(false)
    }

    /// Get an integer config value.
    pub fn get_int(&self, name: &str) -> i64 {
        self.ints.get(name).copied().unwrap_or(0)
    }

    /// Get a string config value.
    pub fn get_string(&self, name: &str) -> &str {
        self.strings.get(name).map(|s| s.as_str()).unwrap_or("")
    }

    /// Set a boolean config value.
    pub fn set_bool(&mut self, name: &str, value: bool) {
        self.bools.insert(name.to_string(), value);
    }

    /// Set an integer config value with range clamping.
    pub fn set_int(&mut self, name: &str, value: i64) {
        let clamped = match name {
            ik::UPDATE_MS => value.clamp(100, 86_400_000),
            ik::NET_DOWNLOAD | ik::NET_UPLOAD => value.clamp(0, 10_000_000),
            ik::DETAILED_PID => value,
            ik::CURRENT_PRESET => value.max(0),
            _ => value.max(0),
        };
        self.ints.insert(name.to_string(), clamped);
    }

    /// Set a string config value.
    pub fn set_string(&mut self, name: &str, value: &str) {
        self.strings.insert(name.to_string(), value.to_string());
    }

    /// Toggle a boolean config value.
    pub fn flip(&mut self, name: &str) {
        if let Some(val) = self.bools.get_mut(name) {
            *val = !*val;
        }
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

            if self.bools.contains_key(key) {
                match value.to_lowercase().as_str() {
                    "true" => self.bools.insert(key.to_string(), true),
                    "false" => self.bools.insert(key.to_string(), false),
                    _ => {
                        warnings.push(format!("Invalid boolean value for '{key}': {value}"));
                        continue;
                    }
                };
            } else if self.ints.contains_key(key) {
                match value.parse::<i64>() {
                    Ok(v) => {
                        self.set_int(key, v);
                    }
                    Err(_) => {
                        warnings.push(format!("Invalid integer value for '{key}': {value}"));
                    }
                }
            } else if self.strings.contains_key(key) {
                self.strings.insert(key.to_string(), value.to_string());
            } else {
                warnings.push(format!("Unknown config key: '{key}'"));
            }
        }

        warnings
    }

    /// Write config to the config file.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let content = self.to_config_string();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    /// Generate the full config file content as a string.
    pub fn to_config_string(&self) -> String {
        let mut out = String::new();
        out.push_str("#? Config file for rtop\n\n");

        // Write strings
        let mut skeys: Vec<_> = self.strings.keys().collect();
        skeys.sort();
        for key in skeys {
            let val = &self.strings[key];
            out.push_str(&format!("{key} = \"{val}\"\n"));
        }
        out.push('\n');

        // Write bools
        let mut bkeys: Vec<_> = self.bools.keys().collect();
        bkeys.sort();
        for key in bkeys {
            let val = if self.bools[key] { "True" } else { "False" };
            out.push_str(&format!("{key} = {val}\n"));
        }
        out.push('\n');

        // Write ints
        let mut ikeys: Vec<_> = self.ints.keys().collect();
        ikeys.sort();
        for key in ikeys {
            out.push_str(&format!("{key} = {}\n", self.ints[key]));
        }

        out
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
        let custom_idx = index - 1; // offset for the hardcoded preset 0
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
        // Adjust current_preset if needed
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
        assert_eq!(config.get_int("update_ms"), 2000); // default preserved
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
        assert_eq!(config.get_int("update_ms"), 100); // clamped to min
        config.set_int("update_ms", 100_000_000);
        assert_eq!(config.get_int("update_ms"), 86_400_000); // clamped to max
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
        assert!(!config.toggle_box("gpu10")); // two digits
        assert!(!config.toggle_box("gpuX")); // not a digit
    }

    #[test]
    fn default_has_all_expected_keys() {
        let config = Config::new();
        // Spot-check key categories
        assert!(config.strings.contains_key("color_theme"));
        assert!(config.strings.contains_key("proc_sorting"));
        assert!(config.bools.contains_key("truecolor"));
        assert!(config.bools.contains_key("show_battery"));
        assert!(config.ints.contains_key("update_ms"));
        assert!(config.ints.contains_key("net_download"));
    }

    #[test]
    fn preset_list_default_has_builtin_presets() {
        let config = Config::new();
        let list = config.preset_list();
        // Preset 0 (hardcoded default) + 3 built-in custom presets
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
        assert_eq!(list.len(), 3); // 1 hardcoded + 2 custom
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
}
