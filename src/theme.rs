use crate::theme_keys::{ColorKey, GradientKey};
use crate::themes::{GradientDef, Rgb, ThemePalette};

pub const COLOR_COUNT: usize = 20;
pub const GRADIENT_COUNT: usize = 17;

struct BundledTheme {
    name: &'static str,
    content: &'static str,
}

const BUNDLED_THEMES: &[BundledTheme] = &[
    BundledTheme {
        name: "Default",
        content: include_str!("../themes/default.toml"),
    },
    BundledTheme {
        name: "adapta",
        content: include_str!("../themes/adapta.toml"),
    },
    BundledTheme {
        name: "adwaita-dark",
        content: include_str!("../themes/adwaita-dark.toml"),
    },
    BundledTheme {
        name: "adwaita",
        content: include_str!("../themes/adwaita.toml"),
    },
    BundledTheme {
        name: "ayu",
        content: include_str!("../themes/ayu.toml"),
    },
    BundledTheme {
        name: "dracula",
        content: include_str!("../themes/dracula.toml"),
    },
    BundledTheme {
        name: "dusklight",
        content: include_str!("../themes/dusklight.toml"),
    },
    BundledTheme {
        name: "elementarish",
        content: include_str!("../themes/elementarish.toml"),
    },
    BundledTheme {
        name: "everforest-dark-hard",
        content: include_str!("../themes/everforest-dark-hard.toml"),
    },
    BundledTheme {
        name: "everforest-dark-medium",
        content: include_str!("../themes/everforest-dark-medium.toml"),
    },
    BundledTheme {
        name: "everforest-light-medium",
        content: include_str!("../themes/everforest-light-medium.toml"),
    },
    BundledTheme {
        name: "flat-remix-light",
        content: include_str!("../themes/flat-remix-light.toml"),
    },
    BundledTheme {
        name: "flat-remix",
        content: include_str!("../themes/flat-remix.toml"),
    },
    BundledTheme {
        name: "flexoki-dark",
        content: include_str!("../themes/flexoki-dark.toml"),
    },
    BundledTheme {
        name: "flexoki-light",
        content: include_str!("../themes/flexoki-light.toml"),
    },
    BundledTheme {
        name: "gotham",
        content: include_str!("../themes/gotham.toml"),
    },
    BundledTheme {
        name: "greyscale",
        content: include_str!("../themes/greyscale.toml"),
    },
    BundledTheme {
        name: "gruvbox_dark",
        content: include_str!("../themes/gruvbox_dark.toml"),
    },
    BundledTheme {
        name: "gruvbox_dark_v2",
        content: include_str!("../themes/gruvbox_dark_v2.toml"),
    },
    BundledTheme {
        name: "gruvbox_light",
        content: include_str!("../themes/gruvbox_light.toml"),
    },
    BundledTheme {
        name: "gruvbox_material_dark",
        content: include_str!("../themes/gruvbox_material_dark.toml"),
    },
    BundledTheme {
        name: "horizon",
        content: include_str!("../themes/horizon.toml"),
    },
    BundledTheme {
        name: "HotPurpleTrafficLight",
        content: include_str!("../themes/HotPurpleTrafficLight.toml"),
    },
    BundledTheme {
        name: "kanagawa-lotus",
        content: include_str!("../themes/kanagawa-lotus.toml"),
    },
    BundledTheme {
        name: "kanagawa-wave",
        content: include_str!("../themes/kanagawa-wave.toml"),
    },
    BundledTheme {
        name: "kyli0x",
        content: include_str!("../themes/kyli0x.toml"),
    },
    BundledTheme {
        name: "matcha-dark-sea",
        content: include_str!("../themes/matcha-dark-sea.toml"),
    },
    BundledTheme {
        name: "monokai",
        content: include_str!("../themes/monokai.toml"),
    },
    BundledTheme {
        name: "night-owl",
        content: include_str!("../themes/night-owl.toml"),
    },
    BundledTheme {
        name: "nord",
        content: include_str!("../themes/nord.toml"),
    },
    BundledTheme {
        name: "onedark",
        content: include_str!("../themes/onedark.toml"),
    },
    BundledTheme {
        name: "orange",
        content: include_str!("../themes/orange.toml"),
    },
    BundledTheme {
        name: "paper",
        content: include_str!("../themes/paper.toml"),
    },
    BundledTheme {
        name: "phoenix-night",
        content: include_str!("../themes/phoenix-night.toml"),
    },
    BundledTheme {
        name: "solarized_dark",
        content: include_str!("../themes/solarized_dark.toml"),
    },
    BundledTheme {
        name: "solarized_light",
        content: include_str!("../themes/solarized_light.toml"),
    },
    BundledTheme {
        name: "tokyo-night",
        content: include_str!("../themes/tokyo-night.toml"),
    },
    BundledTheme {
        name: "tokyo-storm",
        content: include_str!("../themes/tokyo-storm.toml"),
    },
    BundledTheme {
        name: "tomorrow-night",
        content: include_str!("../themes/tomorrow-night.toml"),
    },
    BundledTheme {
        name: "twilight",
        content: include_str!("../themes/twilight.toml"),
    },
    BundledTheme {
        name: "whiteout",
        content: include_str!("../themes/whiteout.toml"),
    },
];

/// All available theme names.
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

/// All theme color data — array-indexed, no HashMap.
#[derive(Debug, Clone)]
pub struct Theme {
    colors: [String; COLOR_COUNT],
    rgbs: [Option<Rgb>; COLOR_COUNT],
    gradients: [Vec<String>; GRADIENT_COUNT],
}

impl Theme {
    /// Create a new theme with default values (parses `default.toml`).
    pub fn new() -> Self {
        Self::from_name("Default")
    }

    /// Load a theme by name from the bundled themes list.
    /// Falls back to Default if name not found or parse fails.
    pub fn from_name(name: &str) -> Self {
        let content = BUNDLED_THEMES
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .or_else(|| BUNDLED_THEMES.first())
            .map(|t| t.content)
            .unwrap();

        match toml::from_str::<ThemePalette>(content) {
            Ok(palette) => Self::from_palette(&palette),
            Err(e) => {
                tracing::warn!("failed to parse theme '{name}': {e}, using Default");
                let default = BUNDLED_THEMES[0].content;
                let palette: ThemePalette =
                    toml::from_str(default).expect("default theme must parse");
                Self::from_palette(&palette)
            }
        }
    }

    /// Build a `Theme` from a parsed `ThemePalette`.
    pub fn from_palette(palette: &ThemePalette) -> Self {
        let c = &palette.colors;

        // Extract colors in index order matching ColorKey constants.
        let rgb_opts: [Option<Rgb>; COLOR_COUNT] = [
            c.main_bg,              // 0
            Some(c.main_fg),        // 1
            Some(c.title),          // 2
            Some(c.hi_fg),          // 3
            Some(c.selected_bg),    // 4
            Some(c.selected_fg),    // 5
            Some(c.graph_text),     // 6
            Some(c.meter_bg),       // 7
            Some(c.proc_tree_fg),   // 8
            Some(c.cpu_box),        // 9
            Some(c.mem_box),        // 10
            Some(c.net_box),        // 11
            Some(c.proc_box),       // 12
            Some(c.gpu_box),        // 13
            Some(c.disk_box),       // 14
            Some(c.help_box),       // 15
            Some(c.options_box),    // 16
            Some(c.proc_follow_bg), // 17
            Some(c.followed_bg),    // 18
            Some(c.followed_fg),    // 19
        ];

        let colors: [String; COLOR_COUNT] = std::array::from_fn(|i| match rgb_opts[i] {
            Some(rgb) => rgb.to_fg_escape(),
            None => String::new(),
        });

        let g = &palette.gradients;
        let gradient_defs: [&GradientDef; GRADIENT_COUNT] = [
            &g.cpu_upper,  // 0  GRAD_CPU_UPPER
            &g.cpu_lower,  // 1  GRAD_CPU_LOWER
            &g.used,       // 2  GRAD_USED
            &g.available,  // 3  GRAD_AVAILABLE
            &g.cached,     // 4  GRAD_CACHED
            &g.free,       // 5  GRAD_FREE
            &g.download,   // 6  GRAD_DOWNLOAD
            &g.upload,     // 7  GRAD_UPLOAD
            &g.gpu,        // 8  GRAD_GPU
            &g.gpu_clock,  // 9  GRAD_GPU_CLOCK
            &g.gpu_power,  // 10 GRAD_GPU_POWER
            &g.gpu_vram,   // 11 GRAD_GPU_VRAM
            &g.disk_read,  // 12 GRAD_DISK_READ
            &g.disk_write, // 13 GRAD_DISK_WRITE
            &g.disk_busy,  // 14 GRAD_DISK_BUSY
            &g.temp,       // 15 GRAD_TEMP
            &g.process,    // 16 GRAD_PROCESS
        ];

        let gradients: [Vec<String>; GRADIENT_COUNT] = std::array::from_fn(|i| {
            let def = gradient_defs[i];
            generate_gradient(
                [def.start.0, def.start.1, def.start.2],
                Some([def.mid.0, def.mid.1, def.mid.2]),
                Some([def.end.0, def.end.1, def.end.2]),
            )
        });

        Self {
            colors,
            rgbs: rgb_opts,
            gradients,
        }
    }

    /// Get a color escape code by typed key.
    pub fn color(&self, key: ColorKey) -> &str {
        &self.colors[key.index()]
    }

    /// Get a background color escape string for a theme color.
    /// Converts the foreground escape (38;2;r;g;b) to background (48;2;r;g;b).
    pub fn background(&self, key: ColorKey) -> String {
        self.color(key).replace("38;2", "48;2")
    }

    /// Get an RGB value for a typed color key.
    pub fn rgb(&self, key: ColorKey) -> [u8; 3] {
        match self.rgbs[key.index()] {
            Some(rgb) => [rgb.0, rgb.1, rgb.2],
            None => [0, 0, 0],
        }
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
    pub fn gradient(&self, key: GradientKey) -> &[String] {
        &self.gradients[key.index()]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert RGB to a foreground ANSI truecolor escape code.
fn rgb_to_fg_escape(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Generate a 101-element gradient from start, optional mid, and optional end colors.
fn generate_gradient(start: [u8; 3], mid: Option<[u8; 3]>, end: Option<[u8; 3]>) -> Vec<String> {
    let mut result = Vec::with_capacity(101);

    match (mid, end) {
        (_, None) => {
            let esc = rgb_to_fg_escape(start[0], start[1], start[2]);
            result.resize(101, esc);
        }
        (None, Some(end)) => {
            for i in 0..=100 {
                let r = interpolate(start[0], end[0], i, 100);
                let g = interpolate(start[1], end[1], i, 100);
                let b = interpolate(start[2], end[2], i, 100);
                result.push(rgb_to_fg_escape(r, g, b));
            }
        }
        (Some(mid), Some(end)) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_keys as tc;

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
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        assert_eq!(grad[100], rgb_to_fg_escape(255, 255, 255));
    }

    #[test]
    fn gradient_start_mid_end_two_segment() {
        let grad = generate_gradient([0, 0, 0], Some([128, 128, 128]), Some([255, 255, 255]));
        assert_eq!(grad.len(), 101);
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
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
    fn default_theme_constructs() {
        let theme = Theme::new();
        // main_fg should be a valid ANSI escape
        let color = theme.color(tc::MAIN_FG);
        assert!(color.starts_with("\x1b[38;2;"));
    }

    #[test]
    fn default_theme_has_all_gradients() {
        let theme = Theme::new();
        assert_eq!(theme.gradient(tc::GRAD_CPU_UPPER).len(), 101);
        assert_eq!(theme.gradient(tc::GRAD_TEMP).len(), 101);
        assert_eq!(theme.gradient(tc::GRAD_DISK_BUSY).len(), 101);
    }

    #[test]
    fn from_name_dracula() {
        let theme = Theme::from_name("dracula");
        let color = theme.color(tc::MAIN_FG);
        assert!(color.starts_with("\x1b[38;2;"));
    }

    #[test]
    fn from_name_unknown_falls_back_to_default() {
        let theme = Theme::from_name("nonexistent_theme_xyz");
        let default = Theme::new();
        assert_eq!(theme.color(tc::MAIN_FG), default.color(tc::MAIN_FG));
    }

    #[test]
    fn base_style_honors_theme_background() {
        let theme = Theme::from_name("Default");
        assert!(theme.base_style(true).contains("\x1b[48;2;"));
        assert!(theme.base_style(false).contains("\x1b[49m"));
    }

    #[test]
    fn style_output_reapplies_base_after_reset() {
        let theme = Theme::from_name("Default");
        let base = theme.base_style(true);
        let styled = theme.style_output("left\x1b[0mright", true);
        assert!(styled.starts_with(&base));
        assert!(styled.contains(&format!("\x1b[0m{base}right")));
    }

    #[test]
    fn empty_main_bg_keeps_terminal_background() {
        // Use a theme TOML where main_bg is omitted
        let toml_str = include_str!("../themes/whiteout.toml");
        let palette: ThemePalette = toml::from_str(toml_str).unwrap();
        // whiteout has main_bg, but we can test the mechanism with a palette
        // that has None for main_bg
        let mut palette_no_bg = palette;
        palette_no_bg.colors.main_bg = None;
        let theme = Theme::from_palette(&palette_no_bg);

        assert_eq!(theme.background(tc::MAIN_BG), "");
        assert!(!theme.base_style(true).contains("\x1b[48;2;0;0;0m"));
    }

    #[test]
    fn gradient_values_at_boundaries() {
        let grad = generate_gradient([0, 0, 0], None, Some([200, 100, 50]));
        assert_eq!(grad[0], rgb_to_fg_escape(0, 0, 0));
        assert_eq!(grad[100], rgb_to_fg_escape(200, 100, 50));
        assert_eq!(grad[50], rgb_to_fg_escape(100, 50, 25));
    }

    #[test]
    fn all_bundled_themes_parse_successfully() {
        for bt in BUNDLED_THEMES {
            let result = toml::from_str::<ThemePalette>(bt.content);
            assert!(
                result.is_ok(),
                "bundled theme '{}' failed to parse: {}",
                bt.name,
                result.unwrap_err()
            );
        }
    }

    #[test]
    fn theme_names_matches_bundled_themes() {
        assert_eq!(
            THEME_NAMES.len(),
            BUNDLED_THEMES.len(),
            "THEME_NAMES and BUNDLED_THEMES must have the same length"
        );
        for (i, bt) in BUNDLED_THEMES.iter().enumerate() {
            assert_eq!(
                THEME_NAMES[i], bt.name,
                "THEME_NAMES[{i}] does not match BUNDLED_THEMES"
            );
        }
    }

    #[test]
    fn rgb_accessor_returns_values() {
        let theme = Theme::new();
        let rgb = theme.rgb(tc::MAIN_FG);
        // Default main_fg is #cccccc
        assert_eq!(rgb, [0xcc, 0xcc, 0xcc]);
    }

    #[test]
    fn rgb_accessor_returns_zero_for_none_bg() {
        let toml_str = include_str!("../themes/default.toml");
        let mut palette: ThemePalette = toml::from_str(toml_str).unwrap();
        palette.colors.main_bg = None;
        let theme = Theme::from_palette(&palette);
        assert_eq!(theme.rgb(tc::MAIN_BG), [0, 0, 0]);
    }
}
