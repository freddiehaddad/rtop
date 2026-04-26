use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// All theme color data.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Color name → ANSI escape code string.
    pub colors: HashMap<String, String>,
    /// Color name → [R, G, B] decimal values.
    pub rgbs: HashMap<String, [u8; 3]>,
    /// Gradient name → 101-element array of ANSI escape codes (indices 0–100).
    pub gradients: HashMap<String, Vec<String>>,
}

/// Default theme color values (hex).
const DEFAULT_THEME: &[(&str, &str)] = &[
    ("main_bg", "#00"),
    ("main_fg", "#cc"),
    ("title", "#ee"),
    ("hi_fg", "#b54040"),
    ("selected_bg", "#6a2f2f"),
    ("selected_fg", "#ee"),
    ("inactive_fg", "#40"),
    ("graph_text", "#60"),
    ("meter_bg", "#40"),
    ("proc_misc", "#0de756"),
    ("cpu_box", "#556d59"),
    ("mem_box", "#6c6c4b"),
    ("net_box", "#5c588d"),
    ("proc_box", "#805252"),
    ("div_line", "#30"),
    ("temp_start", "#4897d4"),
    ("temp_mid", "#5474e8"),
    ("temp_end", "#ff40b6"),
    ("cpu_start", "#77ca9b"),
    ("cpu_mid", "#cbc06c"),
    ("cpu_end", "#dc4c4c"),
    ("free_start", "#384f21"),
    ("free_mid", "#b5e685"),
    ("free_end", "#dcff85"),
    ("cached_start", "#163350"),
    ("cached_mid", "#74e6fc"),
    ("cached_end", "#26c5ff"),
    ("available_start", "#4e3f0e"),
    ("available_mid", "#ffd77a"),
    ("available_end", "#ffb814"),
    ("used_start", "#592b26"),
    ("used_mid", "#d9626d"),
    ("used_end", "#ff4769"),
    ("download_start", "#291f75"),
    ("download_mid", "#4f43a3"),
    ("download_end", "#b0a9de"),
    ("upload_start", "#620665"),
    ("upload_mid", "#7d4180"),
    ("upload_end", "#dcafde"),
    ("process_start", "#80d0a3"),
    ("process_mid", "#dcd179"),
    ("process_end", "#d45454"),
    ("proc_pause_bg", "#b54040"),
    ("proc_follow_bg", "#4040b5"),
    ("proc_banner_bg", "#7b407b"),
    ("proc_banner_fg", "#ee"),
    ("followed_bg", "#4040b5"),
    ("followed_fg", "#ee"),
];

impl Theme {
    /// Create a new theme with default values.
    pub fn new() -> Self {
        let mut theme = Self {
            colors: HashMap::new(),
            rgbs: HashMap::new(),
            gradients: HashMap::new(),
        };
        theme.load_defaults();
        theme.generate_gradients();
        theme
    }

    /// Load a theme by name from the bundled themes list.
    /// Returns a new Theme. Falls back to default if name not found.
    pub fn from_name(name: &str) -> Self {
        if name == "Default" || name.is_empty() {
            return Self::new();
        }
        if let Some(content) = get_bundled_theme(name) {
            let mut theme = Self::new();
            theme.load_from_string(content);
            theme
        } else {
            Self::new()
        }
    }

    /// Load theme colors from a theme file content string.
    pub fn load_from_string(&mut self, content: &str) {
        self.load_defaults();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let Some(rest) = trimmed.strip_prefix("theme[") else {
                continue;
            };
            let Some((key, rest)) = rest.split_once(']') else {
                continue;
            };
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');

            if !DEFAULT_THEME.iter().any(|(k, _)| *k == key) {
                continue;
            }
            if value.is_empty() {
                continue;
            }

            let rgb = if value.starts_with('#') {
                parse_hex(value)
            } else {
                parse_decimal_rgb(value)
            };

            let escape = rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]);
            self.rgbs.insert(key.to_string(), rgb);
            self.colors.insert(key.to_string(), escape);
        }

        self.apply_fallbacks();
        self.generate_gradients();
    }

    fn load_defaults(&mut self) {
        for (key, hex) in DEFAULT_THEME {
            let rgb = parse_hex(hex);
            let escape = rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]);
            self.rgbs.insert(key.to_string(), rgb);
            self.colors.insert(key.to_string(), escape);
        }
    }

    /// Load a theme from a `.theme` file.
    #[allow(dead_code)] // will be used when file-based theme loading is wired up
    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read theme: {e}"))?;

        // Start with defaults
        self.load_defaults();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Parse theme[key]="value" format
            let Some(rest) = trimmed.strip_prefix("theme[") else {
                continue;
            };
            let Some((key, rest)) = rest.split_once(']') else {
                continue;
            };
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');

            // Only accept known keys
            if !DEFAULT_THEME.iter().any(|(k, _)| *k == key) {
                continue;
            }

            if value.is_empty() {
                continue;
            }

            let rgb = if value.starts_with('#') {
                parse_hex(value)
            } else {
                parse_decimal_rgb(value)
            };

            let escape = rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]);
            self.rgbs.insert(key.to_string(), rgb);
            self.colors.insert(key.to_string(), escape);
        }

        // Apply fallbacks
        self.apply_fallbacks();
        self.generate_gradients();
        Ok(())
    }

    fn apply_fallbacks(&mut self) {
        // meter_bg defaults to inactive_fg
        if !self.rgbs.contains_key("meter_bg") {
            if let Some(rgb) = self.rgbs.get("inactive_fg").copied() {
                self.rgbs.insert("meter_bg".to_string(), rgb);
                self.colors
                    .insert("meter_bg".to_string(), rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]));
            }
        }
        // process_* defaults to cpu_*
        for suffix in &["_start", "_mid", "_end"] {
            let proc_key = format!("process{suffix}");
            let cpu_key = format!("cpu{suffix}");
            if !self.rgbs.contains_key(&proc_key) {
                if let Some(rgb) = self.rgbs.get(&cpu_key).copied() {
                    self.rgbs.insert(proc_key.clone(), rgb);
                    self.colors
                        .insert(proc_key, rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]));
                }
            }
        }
        // graph_text defaults to inactive_fg
        if !self.rgbs.contains_key("graph_text") {
            if let Some(rgb) = self.rgbs.get("inactive_fg").copied() {
                self.rgbs.insert("graph_text".to_string(), rgb);
                self.colors
                    .insert("graph_text".to_string(), rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]));
            }
        }
    }

    fn generate_gradients(&mut self) {
        let gradient_names = [
            "temp", "cpu", "free", "cached", "available", "used", "download", "upload", "process",
        ];

        for name in gradient_names {
            let start_key = format!("{name}_start");
            let mid_key = format!("{name}_mid");
            let end_key = format!("{name}_end");

            let start = self.rgbs.get(&start_key).copied().unwrap_or([128, 128, 128]);
            let mid = self.rgbs.get(&mid_key).copied();
            let end = self.rgbs.get(&end_key).copied();

            let gradient = generate_gradient(start, mid, end);
            self.gradients.insert(name.to_string(), gradient);
        }
    }

    /// Get a color escape code by name.
    pub fn c(&self, name: &str) -> &str {
        self.colors.get(name).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get a gradient array by name (101 elements, indices 0–100).
    pub fn g(&self, name: &str) -> &[String] {
        self.gradients.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get RGB decimal values by name.
    #[allow(dead_code)] // used in tests and as UI grows
    pub fn dec(&self, name: &str) -> [u8; 3] {
        self.rgbs.get(name).copied().unwrap_or([0, 0, 0])
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

/// Discover available theme files.
#[allow(dead_code)] // will be used when theme selection UI is wired up
pub fn discover_themes(dirs: &[&Path]) -> Vec<PathBuf> {
    let mut themes = Vec::new();
    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "theme") {
                    themes.push(path);
                }
            }
        }
    }
    themes.sort();
    themes
}

/// Parse a hex color string to [R, G, B].
/// Supports "#RRGGBB" (6-char) and "#GG" (2-char grayscale).
pub fn parse_hex(hex: &str) -> [u8; 3] {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b]
        }
        2 => {
            let v = u8::from_str_radix(hex, 16).unwrap_or(0);
            [v, v, v]
        }
        _ => [0, 0, 0],
    }
}

/// Parse "R G B" decimal format to [R, G, B].
pub fn parse_decimal_rgb(s: &str) -> [u8; 3] {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 3 {
        let r = parts[0].parse::<u8>().unwrap_or(0);
        let g = parts[1].parse::<u8>().unwrap_or(0);
        let b = parts[2].parse::<u8>().unwrap_or(0);
        [r, g, b]
    } else {
        [0, 0, 0]
    }
}

/// Convert RGB to a foreground ANSI truecolor escape code.
pub fn rgb_to_fg_escape(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Convert RGB to a background ANSI truecolor escape code.
#[allow(dead_code)] // used in tests and as UI grows
pub fn rgb_to_bg_escape(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[48;2;{r};{g};{b}m")
}

/// Convert 24-bit RGB to 256-color index.
#[allow(dead_code)] // used in tests and for 256-color terminal fallback
pub fn truecolor_to_256(r: u8, g: u8, b: u8) -> u8 {
    if r / 11 == g / 11 && g / 11 == b / 11 {
        // Grayscale range (232-255)
        if r < 6 {
            16
        } else if r > 249 {
            231
        } else {
            (((r as u16 - 6) * 24 / 244) as u8) + 232
        }
    } else {
        // 6x6x6 color cube (16-231)
        let ri = ((r as f64 / 51.0).round() as u8).min(5);
        let gi = ((g as f64 / 51.0).round() as u8).min(5);
        let bi = ((b as f64 / 51.0).round() as u8).min(5);
        16 + 36 * ri + 6 * gi + bi
    }
}

/// Generate a 101-element gradient from start, optional mid, and optional end colors.
fn generate_gradient(
    start: [u8; 3],
    mid: Option<[u8; 3]>,
    end: Option<[u8; 3]>,
) -> Vec<String> {
    let mut result = Vec::with_capacity(101);

    match (mid, end) {
        (_, None) => {
            // Start only — fill all 101 with start color
            let esc = rgb_to_fg_escape(start[0], start[1], start[2]);
            result.resize(101, esc);
        }
        (None, Some(end)) => {
            // Start + end — linear interpolation across 0–100
            for i in 0..=100 {
                let r = interpolate(start[0], end[0], i, 100);
                let g = interpolate(start[1], end[1], i, 100);
                let b = interpolate(start[2], end[2], i, 100);
                result.push(rgb_to_fg_escape(r, g, b));
            }
        }
        (Some(mid), Some(end)) => {
            // Start + mid + end — two segments
            for i in 0..=50 {
                let r = interpolate(start[0], mid[0], i, 50);
                let g = interpolate(start[1], mid[1], i, 50);
                let b = interpolate(start[2], mid[2], i, 50);
                result.push(rgb_to_fg_escape(r, g, b));
            }
            for i in 1..=50 {
                let r = interpolate(mid[0], end[0], i, 50);
                let g = interpolate(mid[1], end[1], i, 50);
                let b = interpolate(mid[2], end[2], i, 50);
                result.push(rgb_to_fg_escape(r, g, b));
            }
        }
    }

    result
}

fn interpolate(start: u8, end: u8, step: usize, total: usize) -> u8 {
    if total == 0 {
        return start;
    }
    let s = start as i32;
    let e = end as i32;
    (s + (step as i32) * (e - s) / (total as i32)).clamp(0, 255) as u8
}

// --- Bundled themes ---

/// All available theme names (Default + bundled).
pub const THEME_NAMES: &[&str] = &[
    "Default",
    "adapta",
    "adwaita-dark",
    "adwaita",
    "ayu",
    "dracula",
    "dusklight",
    "elementarish",
    "everforest-dark-hard",
    "everforest-dark-medium",
    "everforest-light-medium",
    "flat-remix-light",
    "flat-remix",
    "flexoki-dark",
    "flexoki-light",
    "gotham",
    "greyscale",
    "gruvbox_dark",
    "gruvbox_dark_v2",
    "gruvbox_light",
    "gruvbox_material_dark",
    "horizon",
    "HotPurpleTrafficLight",
    "kanagawa-lotus",
    "kanagawa-wave",
    "kyli0x",
    "matcha-dark-sea",
    "monokai",
    "night-owl",
    "nord",
    "onedark",
    "orange",
    "paper",
    "phoenix-night",
    "solarized_dark",
    "solarized_light",
    "tokyo-night",
    "tokyo-storm",
    "tomorrow-night",
    "twilight",
    "whiteout",
];

/// Get the content of a bundled theme by name.
fn get_bundled_theme(name: &str) -> Option<&'static str> {
    match name {
        "adapta" => Some(include_str!("../themes/adapta.theme")),
        "adwaita-dark" => Some(include_str!("../themes/adwaita-dark.theme")),
        "adwaita" => Some(include_str!("../themes/adwaita.theme")),
        "ayu" => Some(include_str!("../themes/ayu.theme")),
        "dracula" => Some(include_str!("../themes/dracula.theme")),
        "dusklight" => Some(include_str!("../themes/dusklight.theme")),
        "elementarish" => Some(include_str!("../themes/elementarish.theme")),
        "everforest-dark-hard" => Some(include_str!("../themes/everforest-dark-hard.theme")),
        "everforest-dark-medium" => Some(include_str!("../themes/everforest-dark-medium.theme")),
        "everforest-light-medium" => Some(include_str!("../themes/everforest-light-medium.theme")),
        "flat-remix-light" => Some(include_str!("../themes/flat-remix-light.theme")),
        "flat-remix" => Some(include_str!("../themes/flat-remix.theme")),
        "flexoki-dark" => Some(include_str!("../themes/flexoki-dark.theme")),
        "flexoki-light" => Some(include_str!("../themes/flexoki-light.theme")),
        "gotham" => Some(include_str!("../themes/gotham.theme")),
        "greyscale" => Some(include_str!("../themes/greyscale.theme")),
        "gruvbox_dark" => Some(include_str!("../themes/gruvbox_dark.theme")),
        "gruvbox_dark_v2" => Some(include_str!("../themes/gruvbox_dark_v2.theme")),
        "gruvbox_light" => Some(include_str!("../themes/gruvbox_light.theme")),
        "gruvbox_material_dark" => Some(include_str!("../themes/gruvbox_material_dark.theme")),
        "horizon" => Some(include_str!("../themes/horizon.theme")),
        "HotPurpleTrafficLight" => Some(include_str!("../themes/HotPurpleTrafficLight.theme")),
        "kanagawa-lotus" => Some(include_str!("../themes/kanagawa-lotus.theme")),
        "kanagawa-wave" => Some(include_str!("../themes/kanagawa-wave.theme")),
        "kyli0x" => Some(include_str!("../themes/kyli0x.theme")),
        "matcha-dark-sea" => Some(include_str!("../themes/matcha-dark-sea.theme")),
        "monokai" => Some(include_str!("../themes/monokai.theme")),
        "night-owl" => Some(include_str!("../themes/night-owl.theme")),
        "nord" => Some(include_str!("../themes/nord.theme")),
        "onedark" => Some(include_str!("../themes/onedark.theme")),
        "orange" => Some(include_str!("../themes/orange.theme")),
        "paper" => Some(include_str!("../themes/paper.theme")),
        "phoenix-night" => Some(include_str!("../themes/phoenix-night.theme")),
        "solarized_dark" => Some(include_str!("../themes/solarized_dark.theme")),
        "solarized_light" => Some(include_str!("../themes/solarized_light.theme")),
        "tokyo-night" => Some(include_str!("../themes/tokyo-night.theme")),
        "tokyo-storm" => Some(include_str!("../themes/tokyo-storm.theme")),
        "tomorrow-night" => Some(include_str!("../themes/tomorrow-night.theme")),
        "twilight" => Some(include_str!("../themes/twilight.theme")),
        "whiteout" => Some(include_str!("../themes/whiteout.theme")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- hex parsing ---

    #[test]
    fn parse_hex_6char() {
        assert_eq!(parse_hex("#ff0000"), [255, 0, 0]);
        assert_eq!(parse_hex("#00ff00"), [0, 255, 0]);
        assert_eq!(parse_hex("#282a36"), [40, 42, 54]);
    }

    #[test]
    fn parse_hex_2char_grayscale() {
        assert_eq!(parse_hex("#cc"), [204, 204, 204]);
        assert_eq!(parse_hex("#00"), [0, 0, 0]);
        assert_eq!(parse_hex("#ff"), [255, 255, 255]);
    }

    #[test]
    fn parse_hex_invalid_returns_black() {
        assert_eq!(parse_hex("#xyz"), [0, 0, 0]);
        assert_eq!(parse_hex(""), [0, 0, 0]);
    }

    #[test]
    fn parse_decimal_rgb_valid() {
        assert_eq!(parse_decimal_rgb("255 128 0"), [255, 128, 0]);
    }

    // --- color conversion ---

    #[test]
    fn rgb_to_fg_escape_format() {
        assert_eq!(rgb_to_fg_escape(255, 0, 0), "\x1b[38;2;255;0;0m");
    }

    #[test]
    fn rgb_to_bg_escape_format() {
        assert_eq!(rgb_to_bg_escape(0, 255, 0), "\x1b[48;2;0;255;0m");
    }

    #[test]
    fn truecolor_to_256_grayscale() {
        // Near-black
        assert_eq!(truecolor_to_256(0, 0, 0), 16);
        // Near-white
        assert_eq!(truecolor_to_256(255, 255, 255), 231);
        // Mid-gray
        let idx = truecolor_to_256(128, 128, 128);
        assert!((232..=255).contains(&idx));
    }

    #[test]
    fn truecolor_to_256_color_cube() {
        // Pure red → should be in color cube
        let idx = truecolor_to_256(255, 0, 0);
        assert_eq!(idx, 16 + 36 * 5); // 196
    }

    // --- gradient generation ---

    #[test]
    fn gradient_start_only_fills_all_101() {
        let grad = generate_gradient([255, 0, 0], None, None);
        assert_eq!(grad.len(), 101);
        assert_eq!(grad[0], grad[50]);
        assert_eq!(grad[0], grad[100]);
    }

    #[test]
    fn gradient_start_end_linear_interpolation() {
        let grad = generate_gradient([0, 0, 0], None, Some([255, 255, 255]));
        assert_eq!(grad.len(), 101);
        // Start should be black
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        // End should be near-white (integer rounding)
        assert_eq!(grad[100], rgb_to_fg_escape(255, 255, 255));
    }

    #[test]
    fn gradient_start_mid_end_two_segment() {
        let grad = generate_gradient([0, 0, 0], Some([128, 128, 128]), Some([255, 255, 255]));
        assert_eq!(grad.len(), 101);
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        // Midpoint should be approximately mid color
        assert_eq!(grad[50], rgb_to_fg_escape(128, 128, 128));
        assert_eq!(grad[100], rgb_to_fg_escape(255, 255, 255));
    }

    #[test]
    fn gradient_101_elements_always() {
        let g1 = generate_gradient([100, 100, 100], None, None);
        let g2 = generate_gradient([0, 0, 0], None, Some([255, 255, 255]));
        let g3 = generate_gradient([0, 0, 0], Some([128, 0, 0]), Some([255, 0, 0]));
        assert_eq!(g1.len(), 101);
        assert_eq!(g2.len(), 101);
        assert_eq!(g3.len(), 101);
    }

    // --- Theme ---

    #[test]
    fn default_theme_has_all_keys() {
        let theme = Theme::new();
        assert!(theme.colors.contains_key("main_fg"));
        assert!(theme.colors.contains_key("cpu_start"));
        assert!(theme.colors.contains_key("proc_box"));
    }

    #[test]
    fn default_theme_has_gradients() {
        let theme = Theme::new();
        assert_eq!(theme.g("cpu").len(), 101);
        assert_eq!(theme.g("temp").len(), 101);
        assert_eq!(theme.g("download").len(), 101);
    }

    #[test]
    fn theme_c_accessor() {
        let theme = Theme::new();
        let color = theme.c("main_fg");
        assert!(color.starts_with("\x1b[38;2;"));
    }

    #[test]
    fn theme_dec_accessor() {
        let theme = Theme::new();
        let rgb = theme.dec("main_fg");
        assert_eq!(rgb, [204, 204, 204]); // #cc
    }

    #[test]
    fn theme_nonexistent_key_returns_empty() {
        let theme = Theme::new();
        assert_eq!(theme.c("nonexistent"), "");
        assert_eq!(theme.dec("nonexistent"), [0, 0, 0]);
    }

    #[test]
    fn gradient_values_at_boundaries() {
        let grad = generate_gradient([0, 0, 0], None, Some([200, 100, 50]));
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        assert_eq!(grad[100], rgb_to_fg_escape(200, 100, 50));
        // Midpoint should be approximately half
        assert_eq!(grad[50], rgb_to_fg_escape(100, 50, 25));
    }
}
