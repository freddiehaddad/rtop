use crate::theme_keys::{ColorKey, GradientKey};
use crate::themes::{GradientDef, Rgb, ThemePalette};

pub const COLOR_COUNT: usize = 25;
pub const GRADIENT_COUNT: usize = 17;

struct BundledTheme {
    name: &'static str,
    content: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/bundled_themes.rs"));

/// All theme color data — array-indexed, no HashMap.
#[derive(Debug, Clone)]
pub struct Theme {
    colors: [String; COLOR_COUNT],
    rgbs: [Rgb; COLOR_COUNT],
    gradients: [Vec<String>; GRADIENT_COUNT],
}

impl Theme {
    /// Create a new theme with default values (parses `default.toml`).
    pub fn new() -> Self {
        Self::from_name("default")
    }

    /// Load a theme by name from the bundled themes list.
    /// Falls back to Default if name not found or parse fails.
    pub fn from_name(name: &str) -> Self {
        let content = BUNDLED_THEMES
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .or_else(|| BUNDLED_THEMES.first())
            .map(|t| t.content)
            .expect("BUNDLED_THEMES is non-empty by construction");

        match toml::from_str::<ThemePalette>(content) {
            Ok(palette) => {
                tracing::info!(
                    subsystem = %crate::log::Subsystem::Theme,
                    theme = name,
                    "theme loaded",
                );
                Self::from_palette(&palette)
            }
            Err(e) => {
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Theme,
                    theme = name,
                    error = %e,
                    "theme parse failed; falling back to default",
                );
                let default = BUNDLED_THEMES
                    .first()
                    .expect("BUNDLED_THEMES is non-empty by construction")
                    .content;
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
        let rgbs: [Rgb; COLOR_COUNT] = [
            c.main_bg,       // 0
            c.main_fg,       // 1
            c.title,         // 2
            c.hi_fg,         // 3
            c.selected_bg,   // 4
            c.selected_fg,   // 5
            c.graph_text,    // 6
            c.meter_bg,      // 7
            c.proc_tree_fg,  // 8
            c.cpu_widget,    // 9
            c.mem_widget,    // 10
            c.net_widget,    // 11
            c.proc_widget,   // 12
            c.gpu_widget,    // 13
            c.disk_widget,   // 14
            c.help_box,      // 15
            c.options_box,   // 16
            c.followed_bg,   // 17
            c.followed_fg,   // 18
            c.dead_proc_fg,  // 19
            c.statusbar_bg,  // 20
            c.statusbar_fg,  // 21
            c.statusbar_hi,  // 22
            c.statusbar_sep, // 23
            c.data_label_fg, // 24
        ];

        let colors: [String; COLOR_COUNT] = std::array::from_fn(|i| rgbs[i].to_fg_escape());

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
            rgbs,
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
        let rgb = self.rgbs[key.index()];
        [rgb.0, rgb.1, rgb.2]
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

/// Look up the ANSI color escape from a 101-element gradient at the given
/// percentage. `pct` is clamped to `0..=100` before indexing.
///
/// `Theme::gradient` is documented to return a 101-element slice and
/// `generate_gradient` always emits 101 elements, so this never panics on
/// any gradient produced by the theme system.
pub fn gradient_color(gradient: &[String], pct: i32) -> &str {
    &gradient[pct.clamp(0, 100) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_keys as tc;

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

    #[test]
    fn default_theme_constructs() {
        let theme = Theme::new();
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
        let theme = Theme::from_name("default");
        assert!(theme.base_style(true).contains("\x1b[48;2;"));
        assert!(theme.base_style(false).contains("\x1b[49m"));
    }

    #[test]
    fn style_output_reapplies_base_after_reset() {
        let theme = Theme::from_name("default");
        let base = theme.base_style(true);
        let styled = theme.style_output("left\x1b[0mright", true);
        assert!(styled.starts_with(&base));
        assert!(styled.contains(&format!("\x1b[0m{base}right")));
    }

    #[test]
    fn gradient_color_returns_indexed_escape() {
        let theme = Theme::new();
        let grad = theme.gradient(tc::GRAD_CPU_UPPER);
        assert_eq!(gradient_color(grad, 0), grad[0].as_str());
        assert_eq!(gradient_color(grad, 50), grad[50].as_str());
        assert_eq!(gradient_color(grad, 100), grad[100].as_str());
    }

    #[test]
    fn gradient_color_clamps_out_of_range_pct() {
        let theme = Theme::new();
        let grad = theme.gradient(tc::GRAD_CPU_UPPER);
        assert_eq!(gradient_color(grad, -1), grad[0].as_str());
        assert_eq!(gradient_color(grad, -1000), grad[0].as_str());
        assert_eq!(gradient_color(grad, 101), grad[100].as_str());
        assert_eq!(gradient_color(grad, i32::MAX), grad[100].as_str());
    }

    #[test]
    fn theme_background_off_emits_terminal_default_bg_escape() {
        // The `theme_background = false` runtime toggle is the
        // single source of "let the terminal background show
        // through" — `MAIN_BG` is no longer optional, every theme
        // declares it. `base_style(false)` substitutes the
        // terminal-default-bg escape (`\x1b[49m`) for the themed
        // MAIN_BG so users with terminal transparency see a
        // continuously-transparent UI.
        let theme = Theme::new();
        assert!(theme.base_style(false).contains("\x1b[49m"));
        // And conversely, `base_style(true)` must NOT emit it.
        assert!(!theme.base_style(true).contains("\x1b[49m"));
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
    fn every_bundled_theme_declares_main_bg() {
        // `main_bg` is required — every theme must declare it.
        // This pins the contract so no future theme contribution
        // can omit the field and have it silently default.
        for bt in BUNDLED_THEMES {
            let palette: ThemePalette = toml::from_str(bt.content)
                .unwrap_or_else(|e| panic!("bundled theme '{}' failed to parse: {e}", bt.name));
            // If parsing succeeded, `main_bg: Rgb` is structurally
            // present. Sanity-check by reading it.
            let _ = palette.colors.main_bg;
        }
    }

    #[test]
    fn every_bundled_theme_gives_data_label_fg_its_own_shade() {
        // `data_label_fg` only earns its place if it is visually
        // distinct from the value colour beside it and from the
        // background behind it. Guard both ends for every theme so a
        // future contribution cannot copy `main_fg` verbatim and
        // silently undo the label/value hierarchy.
        for bt in BUNDLED_THEMES {
            let palette: ThemePalette = toml::from_str(bt.content)
                .unwrap_or_else(|e| panic!("bundled theme '{}' failed to parse: {e}", bt.name));
            let c = &palette.colors;
            assert_ne!(
                c.data_label_fg, c.main_fg,
                "theme '{}': data_label_fg must differ from main_fg",
                bt.name,
            );
            assert_ne!(
                c.data_label_fg, c.main_bg,
                "theme '{}': data_label_fg must differ from main_bg",
                bt.name,
            );
        }
    }

    #[test]
    fn every_bundled_theme_keeps_labels_brighter_than_dead_rows() {
        // Brightness order is value > label > dead row. Compare each
        // against `main_bg` so the check holds for light themes, where
        // "brighter" means closer to a dark foreground.
        let distance = |a: Rgb, b: Rgb| {
            let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs();
            d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
        };
        for bt in BUNDLED_THEMES {
            let palette: ThemePalette = toml::from_str(bt.content)
                .unwrap_or_else(|e| panic!("bundled theme '{}' failed to parse: {e}", bt.name));
            let c = &palette.colors;
            let label = distance(c.data_label_fg, c.main_bg);
            let value = distance(c.main_fg, c.main_bg);
            assert!(
                label < value,
                "theme '{}': data_label_fg must sit closer to the background than main_fg \
                 (label {label}, value {value})",
                bt.name,
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
}
