use std::collections::{HashMap, HashSet};

use crate::theme_keys::{ColorKey, GradientKey};

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

/// A declarative fallback rule for theme colors not provided by the theme file.
enum FallbackRule {
    /// Copy a single color: target ← source.
    Single {
        target: &'static str,
        source: &'static str,
    },
    /// Expand a gradient prefix: `{target}_start` ← `{source}_start`, etc.
    Gradient {
        target: &'static str,
        source: &'static str,
    },
    /// Use the first source that was explicitly provided by the theme file,
    /// falling back to the first source with a non-empty default value.
    FirstAvailable {
        target: &'static str,
        sources: &'static [&'static str],
    },
}

/// Theme color fallback rules, applied in order.
///
/// **Ordering matters** for chained dependencies: `followed_bg` depends on
/// `proc_follow_bg`, so `proc_follow_bg` must appear first.
const FALLBACK_RULES: &[FallbackRule] = &[
    FallbackRule::Single {
        target: "meter_bg",
        source: "inactive_fg",
    },
    FallbackRule::Gradient {
        target: "process",
        source: "cpu",
    },
    FallbackRule::Single {
        target: "graph_text",
        source: "inactive_fg",
    },
    FallbackRule::Single {
        target: "proc_tree_fg",
        source: "inactive_fg",
    },
    // GPU gradient fallbacks
    FallbackRule::Gradient {
        target: "gpu",
        source: "cpu",
    },
    FallbackRule::Gradient {
        target: "gpu_clock",
        source: "cpu",
    },
    FallbackRule::Gradient {
        target: "gpu_power",
        source: "used",
    },
    FallbackRule::Gradient {
        target: "gpu_vram",
        source: "cached",
    },
    // Disk IO gradient fallbacks
    FallbackRule::Gradient {
        target: "disk_read",
        source: "download",
    },
    FallbackRule::Gradient {
        target: "disk_write",
        source: "upload",
    },
    FallbackRule::Gradient {
        target: "disk_busy",
        source: "used",
    },
    FallbackRule::FirstAvailable {
        target: "proc_pause_bg",
        sources: &["used_end", "used_mid", "used_start", "hi_fg"],
    },
    FallbackRule::FirstAvailable {
        target: "proc_follow_bg",
        sources: &[
            "download_start",
            "download_mid",
            "net_box",
            "hi_fg",
            "download_end",
        ],
    },
    FallbackRule::Single {
        target: "proc_banner_bg",
        source: "selected_bg",
    },
    FallbackRule::Single {
        target: "proc_banner_fg",
        source: "selected_fg",
    },
    // followed_bg depends on proc_follow_bg resolved above
    FallbackRule::Single {
        target: "followed_bg",
        source: "proc_follow_bg",
    },
    FallbackRule::Single {
        target: "followed_fg",
        source: "selected_fg",
    },
];

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
    ("proc_tree_fg", "#505050"),
    ("cpu_box", "#556d59"),
    ("mem_box", "#6c6c4b"),
    ("net_box", "#5c588d"),
    ("proc_box", "#805252"),
    ("gpu_box", "#6b5673"),
    ("disk_box", "#5e7a5e"),
    ("help_box", "#4a7a99"),
    ("options_box", "#997a4a"),
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
    ("disk_read_start", "#291f75"),
    ("disk_read_mid", "#4f43a3"),
    ("disk_read_end", "#b0a9de"),
    ("disk_write_start", "#620665"),
    ("disk_write_mid", "#7d4180"),
    ("disk_write_end", "#dcafde"),
    ("disk_busy_start", "#592b26"),
    ("disk_busy_mid", "#d9626d"),
    ("disk_busy_end", "#ff4769"),
    ("process_start", "#80d0a3"),
    ("process_mid", "#dcd179"),
    ("process_end", "#d45454"),
    // GPU-specific meter gradients
    ("gpu_start", "#77ca9b"),
    ("gpu_mid", "#cbc06c"),
    ("gpu_end", "#dc4c4c"),
    ("gpu_clock_start", "#3a9a8e"),
    ("gpu_clock_mid", "#3db8e8"),
    ("gpu_clock_end", "#2196f3"),
    ("gpu_power_start", "#d4a748"),
    ("gpu_power_mid", "#e88c3d"),
    ("gpu_power_end", "#dc4c4c"),
    ("gpu_vram_start", "#6b4e8a"),
    ("gpu_vram_mid", "#a855f7"),
    ("gpu_vram_end", "#c084fc"),
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
    pub fn load_from_string(&mut self, content: &str) -> Vec<String> {
        self.load_defaults();
        let mut warnings = Vec::new();
        let mut provided = HashSet::new();

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
                warnings.push(format!("Unknown theme key: '{key}'"));
                continue;
            }
            provided.insert(key.to_string());

            if value.is_empty() {
                self.rgbs.remove(key);
                self.colors.insert(key.to_string(), String::new());
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

        self.apply_fallbacks(&provided);
        self.generate_gradients();
        warnings
    }

    fn load_defaults(&mut self) {
        for (key, hex) in DEFAULT_THEME {
            let rgb = parse_hex(hex);
            let escape = rgb_to_fg_escape(rgb[0], rgb[1], rgb[2]);
            self.rgbs.insert(key.to_string(), rgb);
            self.colors.insert(key.to_string(), escape);
        }
        for key in crate::theme_keys::COLOR_KEYS {
            debug_assert!(self.colors.contains_key(key.name()));
        }
    }

    fn apply_fallbacks(&mut self, provided: &HashSet<String>) {
        for rule in FALLBACK_RULES {
            match rule {
                FallbackRule::Single { target, source } => {
                    if !provided.contains(*target) {
                        self.copy_color(target, source);
                    }
                }
                FallbackRule::Gradient { target, source } => {
                    for suffix in ["_start", "_mid", "_end"] {
                        let key = format!("{target}{suffix}");
                        if !provided.contains(&key) {
                            let fb = format!("{source}{suffix}");
                            self.copy_color(&key, &fb);
                        }
                    }
                }
                FallbackRule::FirstAvailable { target, sources } => {
                    if !provided.contains(*target) {
                        self.copy_first_provided_color(target, sources, provided);
                    }
                }
            }
        }
    }

    fn copy_color(&mut self, target: &str, source: &str) {
        let color = self.colors.get(source).cloned().unwrap_or_default();
        self.colors.insert(target.to_string(), color);
        if let Some(rgb) = self.rgbs.get(source).copied() {
            self.rgbs.insert(target.to_string(), rgb);
        } else {
            self.rgbs.remove(target);
        }
    }

    fn copy_first_provided_color(
        &mut self,
        target: &str,
        sources: &[&str],
        provided: &HashSet<String>,
    ) {
        let source = sources
            .iter()
            .copied()
            .find(|source| {
                provided.contains(*source)
                    && self
                        .colors
                        .get(*source)
                        .is_some_and(|color| !color.is_empty())
            })
            .or_else(|| {
                sources.iter().copied().find(|source| {
                    self.colors
                        .get(*source)
                        .is_some_and(|color| !color.is_empty())
                })
            });
        let Some(source) = source else {
            self.colors.insert(target.to_string(), String::new());
            self.rgbs.remove(target);
            return;
        };
        self.copy_color(target, source);
    }

    fn generate_gradients(&mut self) {
        for key in crate::theme_keys::GRADIENT_KEYS {
            let name = key.name();
            let start_key = format!("{name}_start");
            let mid_key = format!("{name}_mid");
            let end_key = format!("{name}_end");

            let start = self.rgbs.get(&start_key).copied().unwrap_or_default();
            let mid = self.rgbs.get(&mid_key).copied();
            let end = self.rgbs.get(&end_key).copied();

            let gradient = generate_gradient(start, mid, end);
            self.gradients.insert(name.to_string(), gradient);
        }
    }

    /// Get a color escape code by typed key.
    ///
    /// Returns an empty string (default terminal color) if the key is missing.
    /// This should never happen — `load_defaults` + `apply_fallbacks` guarantee
    /// all keys exist — but a fallback is safer than a panic in release builds.
    pub fn color(&self, key: ColorKey) -> &str {
        match self.colors.get(key.name()) {
            Some(color) => color.as_str(),
            None => {
                tracing::warn!("missing theme color '{}', using fallback", key.name());
                ""
            }
        }
    }

    /// Get a background color escape string for a theme color name.
    /// Converts the foreground escape (38;2;r;g;b) to background (48;2;r;g;b).
    pub fn background(&self, key: ColorKey) -> String {
        self.color(key).replace("38;2", "48;2")
    }

    /// Get an RGB value for a typed color key.
    pub fn rgb(&self, key: ColorKey) -> [u8; 3] {
        self.rgbs.get(key.name()).copied().unwrap_or_default()
    }

    /// Base terminal style for normal text and background rendering.
    pub fn base_style(&self, theme_background: bool) -> String {
        let bg = if theme_background {
            self.background(crate::theme_keys::MAIN_BG)
        } else {
            "\x1b[49m".to_string()
        };
        format!("{}{}", self.color(crate::theme_keys::MAIN_FG), bg)
    }

    /// Prefix output with the base style and make hard resets return to it.
    pub fn style_output(&self, output: &str, theme_background: bool) -> String {
        let base = self.base_style(theme_background);
        let reset = format!("\x1b[0m{base}");
        format!("{base}{}", output.replace("\x1b[0m", &reset))
    }

    /// Get a gradient array by typed key (101 elements, indices 0–100).
    ///
    /// Returns a static fallback gradient if the key is missing.
    /// This should never happen — `generate_gradients` populates all keys —
    /// but a fallback is safer than a panic in release builds.
    pub fn gradient(&self, key: GradientKey) -> &[String] {
        match self.gradients.get(key.name()) {
            Some(gradient) => gradient.as_slice(),
            None => {
                tracing::warn!("missing theme gradient '{}', using fallback", key.name());
                static FALLBACK: std::sync::LazyLock<Vec<String>> =
                    std::sync::LazyLock::new(|| vec![String::new(); 101]);
                &FALLBACK
            }
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
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

/// Generate a 101-element gradient from start, optional mid, and optional end colors.
fn generate_gradient(start: [u8; 3], mid: Option<[u8; 3]>, end: Option<[u8; 3]>) -> Vec<String> {
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
    use crate::theme_keys as tc;

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
        for key in tc::COLOR_KEYS {
            assert!(
                theme.colors.contains_key(key.name()),
                "default theme missing color key '{}'",
                key.name()
            );
        }
    }

    #[test]
    fn default_theme_has_gradients() {
        let theme = Theme::new();
        for key in tc::GRADIENT_KEYS {
            assert_eq!(
                theme.gradient(*key).len(),
                101,
                "default theme missing gradient '{}'",
                key.name()
            );
        }
    }

    #[test]
    fn theme_color_accessor() {
        let theme = Theme::new();
        let color = theme.color(tc::MAIN_FG);
        assert!(color.starts_with("\x1b[38;2;"));
    }

    #[test]
    fn base_style_honors_theme_background() {
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[main_fg]="#111111"
            theme[main_bg]="#f6f5f4"
            "##,
        );

        assert!(theme.base_style(true).contains("\x1b[48;2;246;245;244m"));
        assert!(theme.base_style(false).contains("\x1b[49m"));
        assert!(!theme.base_style(false).contains("\x1b[48;2;246;245;244m"));
    }

    #[test]
    fn style_output_reapplies_base_after_reset() {
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[main_fg]="#111111"
            theme[main_bg]="#f6f5f4"
            "##,
        );
        let base = theme.base_style(true);
        let styled = theme.style_output("left\x1b[0mright", true);

        assert!(styled.starts_with(&base));
        assert!(styled.contains(&format!("\x1b[0m{base}right")));
    }

    #[test]
    fn empty_theme_main_bg_keeps_terminal_background() {
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[main_bg]=""
            "##,
        );

        assert_eq!(theme.background(tc::MAIN_BG), "");
        assert!(!theme.base_style(true).contains("\x1b[48;2;0;0;0m"));
    }

    #[test]
    fn empty_gradient_values_override_defaults() {
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[cpu_start]="#123456"
            theme[cpu_mid]=""
            theme[cpu_end]=""
            "##,
        );

        let cpu = theme.gradient(tc::GRAD_CPU);
        assert_eq!(cpu[0], rgb_to_fg_escape(0x12, 0x34, 0x56));
        assert_eq!(cpu[50], cpu[0]);
        assert_eq!(cpu[100], cpu[0]);
    }

    #[test]
    fn unknown_theme_keys_generate_warnings() {
        let mut theme = Theme::new();
        let warnings = theme.load_from_string(
            r##"
            theme[main_fg]="#111111"
            theme[not_a_real_key]="#222222"
            "##,
        );

        assert_eq!(warnings, vec!["Unknown theme key: 'not_a_real_key'"]);
        assert_eq!(theme.color(tc::MAIN_FG), rgb_to_fg_escape(0x11, 0x11, 0x11));
    }

    #[test]
    fn missing_theme_keys_fall_back_to_theme_palette() {
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[inactive_fg]="#112233"
            theme[selected_bg]="#445566"
            theme[selected_fg]="#ddeeff"
            theme[cpu_start]="#010203"
            theme[cpu_mid]="#040506"
            theme[cpu_end]="#070809"
            theme[used_end]="#aa0000"
            theme[download_end]="#0000aa"
            "##,
        );

        assert_eq!(
            theme.color(tc::METER_BG),
            rgb_to_fg_escape(0x11, 0x22, 0x33)
        );
        assert_eq!(
            theme.color(tc::GRAPH_TEXT),
            rgb_to_fg_escape(0x11, 0x22, 0x33)
        );
        assert_eq!(
            theme.color(tc::PROC_TREE_FG),
            rgb_to_fg_escape(0x11, 0x22, 0x33)
        );
        assert_eq!(
            theme.gradient(tc::GRAD_PROCESS)[0],
            rgb_to_fg_escape(1, 2, 3)
        );
        assert_eq!(
            theme.gradient(tc::GRAD_PROCESS)[50],
            rgb_to_fg_escape(4, 5, 6)
        );
        assert_eq!(
            theme.gradient(tc::GRAD_PROCESS)[100],
            rgb_to_fg_escape(7, 8, 9)
        );
        assert_eq!(theme.color(tc::PROC_PAUSE_BG), rgb_to_fg_escape(0xaa, 0, 0));
        assert_eq!(
            theme.color(tc::PROC_FOLLOW_BG),
            rgb_to_fg_escape(0, 0, 0xaa)
        );
        assert_eq!(
            theme.color(tc::PROC_BANNER_BG),
            rgb_to_fg_escape(0x44, 0x55, 0x66)
        );
        assert_eq!(
            theme.color(tc::PROC_BANNER_FG),
            rgb_to_fg_escape(0xdd, 0xee, 0xff)
        );
    }

    #[test]
    fn bundled_themes_declare_all_default_keys() {
        for theme_name in THEME_NAMES
            .iter()
            .copied()
            .filter(|name| *name != "Default")
        {
            let content = get_bundled_theme(theme_name).expect("theme should be bundled");
            let declared: HashSet<&str> = content
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    let rest = trimmed.strip_prefix("theme[")?;
                    let (key, _) = rest.split_once(']')?;
                    Some(key)
                })
                .collect();
            let missing: Vec<&str> = DEFAULT_THEME
                .iter()
                .map(|(key, _)| *key)
                .filter(|key| !declared.contains(key))
                .collect();

            assert!(
                missing.is_empty(),
                "{theme_name} is missing theme keys: {missing:?}"
            );
        }
    }

    #[test]
    fn gradient_values_at_boundaries() {
        let grad = generate_gradient([0, 0, 0], None, Some([200, 100, 50]));
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        assert_eq!(grad[100], rgb_to_fg_escape(200, 100, 50));
        // Midpoint should be approximately half
        assert_eq!(grad[50], rgb_to_fg_escape(100, 50, 25));
    }

    #[test]
    fn followed_bg_inherits_from_proc_follow_bg_via_chained_fallback() {
        // proc_follow_bg is resolved from download_start (FirstAvailable),
        // then followed_bg copies from proc_follow_bg (Single).
        // This only works because FALLBACK_RULES processes proc_follow_bg first.
        let mut theme = Theme::new();
        theme.load_from_string(
            r##"
            theme[download_start]="#112233"
            "##,
        );

        let expected = rgb_to_fg_escape(0x11, 0x22, 0x33);
        assert_eq!(
            theme.color(tc::PROC_FOLLOW_BG),
            expected,
            "proc_follow_bg should fall back to download_start"
        );
        assert_eq!(
            theme.color(tc::FOLLOWED_BG),
            expected,
            "followed_bg should inherit from proc_follow_bg"
        );
    }
}
