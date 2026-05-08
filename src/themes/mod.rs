//! Typed theme palette definitions.
//!
//! Each theme is a TOML file parsed into a [`ThemePalette`] via serde.
//! Bundled themes are embedded with `include_str!()` and parsed at startup.

use serde::Deserialize;
use std::fmt;

/// An RGB color parsed from a `"#RRGGBB"` hex string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Rgb::parse_hex(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid hex color '{s}': expected \"#RRGGBB\" format"
            ))
        })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl Rgb {
    /// Parse a `"#RRGGBB"` hex string into an `Rgb` value.
    pub fn parse_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Rgb(r, g, b))
    }

    /// Convert to a foreground ANSI truecolor escape code.
    pub fn to_fg_escape(self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.0, self.1, self.2)
    }
}

/// Three-stop gradient definition (start → mid → end).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GradientDef {
    pub start: Rgb,
    pub mid: Rgb,
    pub end: Rgb,
}

/// Complete color palette for a theme.
///
/// Every field is required except `main_bg`, which may be omitted for
/// terminal transparency. Parsed from TOML via serde — a missing or
/// invalid field produces a deserialization error.
#[derive(Debug, Clone, Deserialize)]
pub struct ThemePalette {
    pub colors: PaletteColors,
    pub gradients: PaletteGradients,
}

/// Direct color values for a theme.
#[derive(Debug, Clone, Deserialize)]
pub struct PaletteColors {
    /// Main background. `None` = use terminal default (transparency).
    #[serde(default)]
    pub main_bg: Option<Rgb>,
    pub main_fg: Rgb,
    pub title: Rgb,
    pub hi_fg: Rgb,
    pub selected_bg: Rgb,
    pub selected_fg: Rgb,
    pub graph_text: Rgb,
    pub meter_bg: Rgb,
    pub proc_tree_fg: Rgb,
    pub cpu_widget: Rgb,
    pub mem_widget: Rgb,
    pub net_widget: Rgb,
    pub proc_widget: Rgb,
    pub gpu_widget: Rgb,
    pub disk_widget: Rgb,
    pub help_box: Rgb,
    pub options_box: Rgb,
    pub followed_bg: Rgb,
    pub followed_fg: Rgb,
    /// Foreground color for dead-process rows in the paused list.
    /// Each theme picks a palette-coherent muted value (typically a
    /// 50% blend of `main_fg` toward `main_bg`, or the theme's
    /// existing secondary-text color).
    pub dead_proc_fg: Rgb,
}

/// Gradient definitions for a theme.
#[derive(Debug, Clone, Deserialize)]
pub struct PaletteGradients {
    pub cpu_upper: GradientDef,
    pub cpu_lower: GradientDef,
    pub temp: GradientDef,
    pub free: GradientDef,
    pub cached: GradientDef,
    pub available: GradientDef,
    pub used: GradientDef,
    pub download: GradientDef,
    pub upload: GradientDef,
    pub process: GradientDef,
    pub gpu: GradientDef,
    pub gpu_clock: GradientDef,
    pub gpu_power: GradientDef,
    pub gpu_vram: GradientDef,
    pub disk_read: GradientDef,
    pub disk_write: GradientDef,
    pub disk_busy: GradientDef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_parse_hex_valid() {
        assert_eq!(Rgb::parse_hex("#ff0000"), Some(Rgb(255, 0, 0)));
        assert_eq!(Rgb::parse_hex("#00ff00"), Some(Rgb(0, 255, 0)));
        assert_eq!(Rgb::parse_hex("#282a36"), Some(Rgb(0x28, 0x2a, 0x36)));
    }

    #[test]
    fn rgb_parse_hex_rejects_short() {
        assert_eq!(Rgb::parse_hex("#cc"), None);
        assert_eq!(Rgb::parse_hex("#abc"), None);
    }

    #[test]
    fn rgb_parse_hex_rejects_invalid() {
        assert_eq!(Rgb::parse_hex(""), None);
        assert_eq!(Rgb::parse_hex("#xyz123"), None);
        assert_eq!(Rgb::parse_hex("ff0000"), None);
    }

    #[test]
    fn rgb_to_fg_escape_format() {
        assert_eq!(Rgb(255, 0, 0).to_fg_escape(), "\x1b[38;2;255;0;0m");
    }

    #[test]
    fn rgb_display() {
        assert_eq!(format!("{}", Rgb(0x28, 0x2a, 0x36)), "#282a36");
    }

    #[test]
    fn palette_deserializes_from_toml() {
        let toml_str = r##"
[colors]
main_bg = "#282a36"
main_fg = "#f8f8f2"
title = "#f8f8f2"
hi_fg = "#6272a4"
graph_text = "#f8f8f2"
meter_bg = "#44475a"
selected_bg = "#ff79c6"
selected_fg = "#f8f8f2"
followed_bg = "#bd93f9"
followed_fg = "#f8f8f2"
dead_proc_fg = "#6272a4"
cpu_widget = "#bd93f9"
mem_widget = "#50fa7b"
net_widget = "#ff5555"
proc_widget = "#8be9fd"
gpu_widget = "#f1fa8c"
disk_widget = "#ffb86c"
help_box = "#6272a4"
options_box = "#ffb86c"
proc_tree_fg = "#44475a"

[gradients.cpu_upper]
start = "#bd93f9"
mid = "#8be9fd"
end = "#50fa7b"

[gradients.cpu_lower]
start = "#4897d4"
mid = "#5474e8"
end = "#ff40b6"

[gradients.used]
start = "#96faaf"
mid = "#50fa7b"
end = "#0dfa49"

[gradients.available]
start = "#ffd4a6"
mid = "#ffb86c"
end = "#ff9c33"

[gradients.cached]
start = "#b1f0fd"
mid = "#8be9fd"
end = "#26d7fd"

[gradients.free]
start = "#ffa6d9"
mid = "#ff79c6"
end = "#ff33a8"

[gradients.download]
start = "#bd93f9"
mid = "#50fa7b"
end = "#8be9fd"

[gradients.upload]
start = "#8c42ab"
mid = "#ff79c6"
end = "#ff33a8"

[gradients.gpu]
start = "#bd93f9"
mid = "#8be9fd"
end = "#50fa7b"

[gradients.gpu_clock]
start = "#8be9fd"
mid = "#6272a4"
end = "#44475a"

[gradients.gpu_power]
start = "#ffb86c"
mid = "#ff79c6"
end = "#ff5555"

[gradients.gpu_vram]
start = "#bd93f9"
mid = "#ff79c6"
end = "#f1fa8c"

[gradients.disk_read]
start = "#bd93f9"
mid = "#50fa7b"
end = "#8be9fd"

[gradients.disk_write]
start = "#8c42ab"
mid = "#ff79c6"
end = "#ff33a8"

[gradients.disk_busy]
start = "#96faaf"
mid = "#50fa7b"
end = "#0dfa49"

[gradients.temp]
start = "#bd93f9"
mid = "#ff79c6"
end = "#ff33a8"

[gradients.process]
start = "#50fa7b"
mid = "#59b690"
end = "#6272a4"
"##;
        let palette: ThemePalette = toml::from_str(toml_str).unwrap();
        assert_eq!(palette.colors.main_bg, Some(Rgb(0x28, 0x2a, 0x36)));
        assert_eq!(palette.colors.hi_fg, Rgb(0x62, 0x72, 0xa4));
        assert_eq!(palette.gradients.cpu_upper.start, Rgb(0xbd, 0x93, 0xf9));
        assert_eq!(palette.gradients.cpu_upper.end, Rgb(0x50, 0xfa, 0x7b));
    }

    #[test]
    fn palette_main_bg_optional() {
        let toml_str = r##"
[colors]
main_fg = "#f8f8f2"
title = "#f8f8f2"
hi_fg = "#6272a4"
graph_text = "#f8f8f2"
meter_bg = "#44475a"
selected_bg = "#ff79c6"
selected_fg = "#f8f8f2"
followed_bg = "#bd93f9"
followed_fg = "#f8f8f2"
dead_proc_fg = "#6272a4"
cpu_widget = "#bd93f9"
mem_widget = "#50fa7b"
net_widget = "#ff5555"
proc_widget = "#8be9fd"
gpu_widget = "#f1fa8c"
disk_widget = "#ffb86c"
help_box = "#6272a4"
options_box = "#ffb86c"
proc_tree_fg = "#44475a"

[gradients.cpu_upper]
start = "#000000"
mid = "#808080"
end = "#ffffff"

[gradients.cpu_lower]
start = "#4897d4"
mid = "#5474e8"
end = "#ff40b6"

[gradients.used]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.available]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.cached]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.free]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.download]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.upload]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.gpu]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.gpu_clock]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.gpu_power]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.gpu_vram]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.disk_read]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.disk_write]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.disk_busy]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.temp]
start = "#000000"
mid = "#808080"
end = "#ffffff"
[gradients.process]
start = "#000000"
mid = "#808080"
end = "#ffffff"
"##;
        let palette: ThemePalette = toml::from_str(toml_str).unwrap();
        assert_eq!(palette.colors.main_bg, None);
    }

    #[test]
    fn palette_rejects_missing_required_field() {
        let toml_str = r##"
[colors]
main_fg = "#f8f8f2"
# title is missing — should fail

[gradients.cpu_upper]
start = "#000000"
mid = "#808080"
end = "#ffffff"

[gradients.cpu_lower]
start = "#4897d4"
mid = "#5474e8"
end = "#ff40b6"

"##;
        let result = toml::from_str::<ThemePalette>(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn palette_rejects_invalid_hex() {
        let toml_str = r##"
[colors]
main_fg = "#xyz123"
title = "#f8f8f2"
"##;
        let result = toml::from_str::<ThemePalette>(toml_str);
        assert!(result.is_err());
    }
}
