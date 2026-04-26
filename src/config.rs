use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
            ("color_theme", "Default"),
            ("shown_boxes", "cpu mem net proc disk"),
            ("graph_symbol", "braille"),
            ("graph_symbol_cpu", "default"),
            ("graph_symbol_gpu", "default"),
            ("graph_symbol_mem", "default"),
            ("graph_symbol_net", "default"),
            ("graph_symbol_proc", "default"),
            ("proc_sorting", "cpu lazy"),
            ("cpu_graph_upper", "Auto"),
            ("cpu_graph_lower", "Auto"),
            ("cpu_sensor", "Auto"),
            ("selected_battery", "Auto"),
            ("cpu_core_map", ""),
            ("temp_scale", "celsius"),
            ("clock_format", "%X"),
            ("custom_cpu_name", ""),
            ("disks_filter", ""),
            ("io_graph_speeds", ""),
            ("net_iface", ""),
            ("log_level", "WARNING"),
            ("proc_filter", ""),
            ("presets", "cpu:0:default,proc:0:default cpu:0:default,mem:0:default,disk:0:default cpu:0:default,net:0:default,proc:0:default"),
            ("custom_gpu_name0", ""),
            ("custom_gpu_name1", ""),
            ("custom_gpu_name2", ""),
            ("custom_gpu_name3", ""),
            ("custom_gpu_name4", ""),
            ("custom_gpu_name5", ""),
        ];
        for (k, v) in string_defaults {
            self.strings.entry(k.to_string()).or_insert_with(|| v.to_string());
        }

        // Bool defaults
        let bool_defaults: &[(&str, bool)] = &[
            ("theme_background", true),
            ("truecolor", true),
            ("rounded_corners", true),
            ("proc_reversed", false),
            ("proc_tree", false),
            ("proc_colors", true),
            ("proc_gradient", true),
            ("proc_per_core", false),
            ("proc_mem_bytes", true),
            ("proc_cpu_graphs", true),
            ("proc_left", false),
            ("proc_filter_kernel", false),
            ("proc_follow_detailed", true),
            ("proc_aggregate", false),
            ("keep_dead_proc_usage", false),
            ("cpu_invert_lower", true),
            ("cpu_single_graph", false),
            ("cpu_bottom", false),
            ("show_uptime", true),
            ("show_cpu_watts", true),
            ("check_temp", true),
            ("show_coretemp", true),
            ("show_cpu_freq", true),
            ("mem_graphs", true),
            ("mem_below_net", false),
            ("show_swap", true),
            ("swap_disk", true),
            ("show_disks", true),
            ("only_physical", true),
            ("show_io_stat", true),
            ("io_mode", false),
            ("io_graph_combined", false),
            ("swap_upload_download", false),
            ("base_10_sizes", false),
            ("net_auto", true),
            ("net_sync", true),
            ("show_battery", true),
            ("show_battery_watts", true),
            ("vim_keys", false),
            ("force_tty", false),
            ("lowcolor", false),
            ("background_update", true),
            ("terminal_sync", true),
            ("save_config_on_exit", true),
            ("disable_mouse", false),
            ("disk_free_priv", false),
            ("gpu_mirror_graph", true),
        ];
        for (k, v) in bool_defaults {
            self.bools.entry(k.to_string()).or_insert(*v);
        }

        // Int defaults
        let int_defaults: &[(&str, i64)] = &[
            ("update_ms", 2000),
            ("net_download", 100),
            ("net_upload", 100),
            ("detailed_pid", 0),
            ("selected_pid", 0),
            ("followed_pid", 0),
            ("proc_start", 0),
            ("proc_selected", 0),
            ("current_preset", 0),
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
            "update_ms" => value.clamp(100, 86_400_000),
            "net_download" | "net_upload" => value.clamp(0, 10_000_000),
            "detailed_pid" => value,
            "current_preset" => value.max(0),
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
    /// Preset 0 is always all boxes shown with default settings.
    pub fn preset_list(&self) -> Vec<String> {
        let mut list = vec!["cpu:0:default,mem:0:default,net:0:default,proc:0:default,disk:0:default".to_string()];
        let presets_str = self.get_string("presets");
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
        let shown = self.get_string("shown_boxes").to_string();
        let cpu_bottom = if self.get_bool("cpu_bottom") { "1" } else { "0" };
        let mem_below_net = if self.get_bool("mem_below_net") { "1" } else { "0" };
        let proc_left = if self.get_bool("proc_left") { "1" } else { "0" };

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

        let current = self.get_string("presets").to_string();
        let updated = if current.is_empty() {
            new_preset
        } else {
            format!("{current} {new_preset}")
        };
        self.set_string("presets", &updated);
        let idx = self.preset_list().len() - 1;
        self.set_int("current_preset", idx as i64);
        idx
    }

    /// Delete the preset at the given index. Index 0 (the default) cannot be deleted.
    /// Returns true if a preset was deleted.
    pub fn delete_preset(&mut self, index: usize) -> bool {
        if index == 0 {
            return false;
        }
        let presets_str = self.get_string("presets").to_string();
        let custom: Vec<&str> = presets_str.split_whitespace().collect();
        let custom_idx = index - 1; // offset for the hardcoded preset 0
        if custom_idx >= custom.len() {
            return false;
        }
        let remaining: Vec<&str> = custom.iter().enumerate()
            .filter(|(i, _)| *i != custom_idx)
            .map(|(_, s)| *s)
            .collect();
        self.set_string("presets", &remaining.join(" "));
        // Adjust current_preset if needed
        let cur = self.get_int("current_preset");
        let total = self.preset_list().len() as i64;
        if cur >= total {
            self.set_int("current_preset", total - 1);
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
                "cpu" => self.set_bool("cpu_bottom", position != "0"),
                "mem" => self.set_bool("mem_below_net", position != "0"),
                "proc" => self.set_bool("proc_left", position != "0"),
                _ => {}
            }
        }
        self.set_string("shown_boxes", &boxes.join(" "));
    }

    /// Toggle a box's visibility in shown_boxes.
    pub fn toggle_box(&mut self, box_name: &str) -> bool {
        let valid = [
            "cpu", "mem", "net", "proc", "disk",
            "gpu0", "gpu1", "gpu2", "gpu3", "gpu4", "gpu5",
        ];
        if !valid.contains(&box_name) {
            return false;
        }

        let current = self.get_string("shown_boxes").to_string();
        let mut boxes: Vec<&str> = current.split_whitespace().collect();

        if let Some(pos) = boxes.iter().position(|b| *b == box_name) {
            boxes.remove(pos);
        } else {
            boxes.push(box_name);
        }

        self.set_string("shown_boxes", &boxes.join(" "));
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
        config.set_string("presets", "cpu:0:default,proc:0:default cpu:1:braille,mem:0:default");
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
        config.set_string("presets", "cpu:0:default,proc:0:default mem:0:default,net:0:default");
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
